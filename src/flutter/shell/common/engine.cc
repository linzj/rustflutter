// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/shell/common/engine.h"

#include <cstring>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "flutter/common/settings.h"
#include "flutter/fml/trace_event.h"
#include "flutter/lib/ui/text/font_collection.h"
#include "flutter/shell/common/animator.h"
#include "rapidjson/document.h"

namespace flutter {

static constexpr char kAssetChannel[] = "flutter/assets";
static constexpr char kLifecycleChannel[] = "flutter/lifecycle";
static constexpr char kNavigationChannel[] = "flutter/navigation";
static constexpr char kLocalizationChannel[] = "flutter/localization";
static constexpr char kSettingsChannel[] = "flutter/settings";

Engine::Engine(Delegate& delegate,
               const PointerDataDispatcherMaker& dispatcher_maker,
               const TaskRunners& task_runners,
               const Settings& settings,
               std::unique_ptr<Animator> animator,
               const std::shared_ptr<FontCollection>& font_collection,
               std::unique_ptr<RuntimeController> runtime_controller)
    : delegate_(delegate),
      settings_(settings),
      animator_(std::move(animator)),
      runtime_controller_(std::move(runtime_controller)),
      font_collection_(font_collection),
      task_runners_(task_runners),
      weak_factory_(this) {
  pointer_data_dispatcher_ = dispatcher_maker(*this);
}

Engine::Engine(Delegate& delegate,
               const PointerDataDispatcherMaker& dispatcher_maker,
               const TaskRunners& task_runners,
               const PlatformData& platform_data,
               const Settings& settings,
               std::unique_ptr<Animator> animator)
    : Engine(delegate,
             dispatcher_maker,
             task_runners,
             settings,
             std::move(animator),
             std::make_shared<FontCollection>(),
             nullptr) {
  // Upstream this constructor also threaded a DartVM, an isolate snapshot, the
  // isolate create/shutdown callbacks and a UIDartState::Context through to the
  // runtime controller. None of that survives a statically linked framework:
  // what the controller needs is the delegate, the task runners, and whatever
  // the platform has told us so far.
  runtime_controller_ =
      std::make_unique<RuntimeController>(*this, task_runners_, platform_data);
}

Engine::~Engine() = default;

fml::TaskRunnerAffineWeakPtr<Engine> Engine::GetWeakPtr() const {
  return weak_factory_.GetWeakPtr();
}

void Engine::SetupDefaultFontManager() {
  TRACE_EVENT0("flutter", "Engine::SetupDefaultFontManager");
  font_collection_->SetupDefaultFontManager(settings_.font_initialization_data);
}

std::shared_ptr<AssetManager> Engine::GetAssetManager() {
  return asset_manager_;
}

fml::TaskRunnerAffineWeakPtr<ImageGeneratorRegistry>
Engine::GetImageGeneratorRegistry() {
  return image_generator_registry_.GetWeakPtr();
}

bool Engine::UpdateAssetManager(
    const std::shared_ptr<AssetManager>& new_asset_manager) {
  if (asset_manager_ && new_asset_manager &&
      *asset_manager_ == *new_asset_manager) {
    return false;
  }

  asset_manager_ = new_asset_manager;

  if (!asset_manager_) {
    return false;
  }

  // Using libTXT as the text engine.
  if (settings_.use_asset_fonts) {
    font_collection_->RegisterFonts(asset_manager_);
  }

  if (settings_.use_test_fonts) {
    font_collection_->RegisterTestFonts();
  }

  return true;
}

bool Engine::Restart(RunConfiguration configuration) {
  TRACE_EVENT0("flutter", "Engine::Restart");
  if (!configuration.IsValid()) {
    FML_LOG(ERROR) << "Engine run configuration was invalid.";
    return false;
  }
  delegate_.OnPreEngineRestart();

  // Upstream cloned the runtime controller so the new isolate inherited the
  // old one's platform state. Rebuilding it from the state it already holds
  // gets the same result without a VM to clone inside of.
  PlatformData platform_data = runtime_controller_->GetPlatformData();
  runtime_controller_ =
      std::make_unique<RuntimeController>(*this, task_runners_, platform_data);

  UpdateAssetManager(nullptr);
  return Run(std::move(configuration)) == Engine::RunStatus::Success;
}

Engine::RunStatus Engine::Run(RunConfiguration configuration) {
  if (!configuration.IsValid()) {
    FML_LOG(ERROR) << "Engine run configuration was invalid.";
    return RunStatus::Failure;
  }

  last_entry_point_ = configuration.GetEntrypoint();
#if (FLUTTER_RUNTIME_MODE == FLUTTER_RUNTIME_MODE_DEBUG)
  // This is only used to support restart.
  last_entry_point_args_ = configuration.GetEntrypointArgs();
#endif

  last_engine_id_ = configuration.GetEngineId();

  UpdateAssetManager(configuration.GetAssetManager());

  if (runtime_controller_->IsRunning()) {
    return RunStatus::FailureAlreadyRunning;
  }

  // If the embedding prefetched the default font manager, set it up before the
  // framework can ask for a typeface.
  if (settings_.prefetched_default_font_manager) {
    SetupDefaultFontManager();
  }

  if (!runtime_controller_->LaunchApplication()) {
    return RunStatus::Failure;
  }

  if (settings_.merged_platform_ui_thread ==
      Settings::MergedPlatformUIThread::kMergeAfterLaunch) {
    // Move the UI task runner to the platform thread.
    bool success = fml::MessageLoopTaskQueues::GetInstance()->Merge(
        task_runners_.GetPlatformTaskRunner()->GetTaskQueueId(),
        task_runners_.GetUITaskRunner()->GetTaskQueueId());
    if (!success) {
      FML_LOG(ERROR)
          << "Unable to move the UI task runner to the platform thread";
    }
  }

  return Engine::RunStatus::Success;
}

void Engine::BeginFrame(fml::TimePoint frame_time, uint64_t frame_number) {
  runtime_controller_->BeginFrame(frame_time, frame_number);
}

void Engine::ReportTimings(std::vector<int64_t> timings) {
  runtime_controller_->ReportTimings(std::move(timings));
}

void Engine::NotifyIdle(fml::TimeDelta deadline) {
  runtime_controller_->NotifyIdle(deadline);
}

void Engine::AddView(int64_t view_id,
                     const ViewportMetrics& view_metrics,
                     std::function<void(bool added)> callback) {
  runtime_controller_->AddView(view_id, view_metrics, std::move(callback));
}

bool Engine::RemoveView(int64_t view_id) {
  return runtime_controller_->RemoveView(view_id);
}

bool Engine::SendViewFocusEvent(const ViewFocusEvent& event) {
  return runtime_controller_->SendViewFocusEvent(event);
}

void Engine::SetViewportMetrics(int64_t view_id,
                                const ViewportMetrics& metrics) {
  runtime_controller_->SetViewportMetrics(view_id, metrics);
  ScheduleFrame();
}

void Engine::DispatchPlatformMessage(std::unique_ptr<PlatformMessage> message) {
  std::string channel = message->channel();
  if (channel == kLifecycleChannel) {
    if (HandleLifecyclePlatformMessage(message.get())) {
      return;
    }
  } else if (channel == kLocalizationChannel) {
    if (HandleLocalizationPlatformMessage(message.get())) {
      return;
    }
  } else if (channel == kSettingsChannel) {
    HandleSettingsPlatformMessage(message.get());
    return;
  } else if (!runtime_controller_->IsRunning() &&
             channel == kNavigationChannel) {
    // If the framework is not up yet, we may still need to set the initial
    // route.
    HandleNavigationPlatformMessage(std::move(message));
    return;
  }

  if (runtime_controller_->IsRunning() &&
      runtime_controller_->DispatchPlatformMessage(std::move(message))) {
    return;
  }

  FML_DLOG(WARNING) << "Dropping platform message on channel: " << channel;
}

bool Engine::HandleLifecyclePlatformMessage(PlatformMessage* message) {
  const auto& data = message->data();
  std::string state(reinterpret_cast<const char*>(data.GetMapping()),
                    data.GetSize());

  // Always schedule a frame when the app does become active as per API
  // recommendation
  // https://developer.apple.com/documentation/uikit/uiapplicationdelegate/1622956-applicationdidbecomeactive?language=objc
  if (state == "AppLifecycleState.resumed" ||
      state == "AppLifecycleState.inactive") {
    ScheduleFrame();
  }
  runtime_controller_->SetInitialLifecycleState(state);
  // Always forward these messages to the framework by returning false.
  return false;
}

bool Engine::HandleNavigationPlatformMessage(
    std::unique_ptr<PlatformMessage> message) {
  const auto& data = message->data();

  rapidjson::Document document;
  document.Parse(reinterpret_cast<const char*>(data.GetMapping()),
                 data.GetSize());
  if (document.HasParseError() || !document.IsObject()) {
    return false;
  }
  auto root = document.GetObj();
  auto method = root.FindMember("method");
  if (method->value != "setInitialRoute") {
    return false;
  }
  auto route = root.FindMember("args");
  initial_route_ = route->value.GetString();
  return true;
}

bool Engine::HandleLocalizationPlatformMessage(PlatformMessage* message) {
  const auto& data = message->data();

  rapidjson::Document document;
  document.Parse(reinterpret_cast<const char*>(data.GetMapping()),
                 data.GetSize());
  if (document.HasParseError() || !document.IsObject()) {
    return false;
  }
  auto root = document.GetObj();
  auto method = root.FindMember("method");
  if (method == root.MemberEnd()) {
    return false;
  }
  const size_t strings_per_locale = 4;
  if (method->value == "setLocale") {
    // Decode and pass the list of locale data onwards to the framework.
    auto args = root.FindMember("args");
    if (args == root.MemberEnd() || !args->value.IsArray()) {
      return false;
    }

    if (args->value.Size() % strings_per_locale != 0) {
      return false;
    }
    std::vector<std::string> locale_data;
    for (size_t locale_index = 0; locale_index < args->value.Size();
         locale_index += strings_per_locale) {
      if (!args->value[locale_index].IsString() ||
          !args->value[locale_index + 1].IsString()) {
        return false;
      }
      locale_data.push_back(args->value[locale_index].GetString());
      locale_data.push_back(args->value[locale_index + 1].GetString());
      locale_data.push_back(args->value[locale_index + 2].GetString());
      locale_data.push_back(args->value[locale_index + 3].GetString());
    }

    return runtime_controller_->SetLocales(locale_data);
  }
  return false;
}

void Engine::HandleSettingsPlatformMessage(PlatformMessage* message) {
  const auto& data = message->data();
  std::string jsonData(reinterpret_cast<const char*>(data.GetMapping()),
                       data.GetSize());
  if (runtime_controller_->SetUserSettingsData(jsonData)) {
    ScheduleFrame();
  }
}

void Engine::DispatchPointerDataPacket(
    std::unique_ptr<PointerDataPacket> packet,
    uint64_t trace_flow_id) {
  TRACE_EVENT0_WITH_FLOW_IDS("flutter", "Engine::DispatchPointerDataPacket",
                             /*flow_id_count=*/1,
                             /*flow_ids=*/&trace_flow_id);
  TRACE_FLOW_STEP("flutter", "PointerEvent", trace_flow_id);
  pointer_data_dispatcher_->DispatchPacket(std::move(packet), trace_flow_id);
}

HitTestResponse Engine::HitTest(int64_t view_id,
                                const flutter::PointData offset) {
  if (runtime_controller_) {
    return runtime_controller_->HitTest(view_id, offset);
  }
  return {.has_platform_view = false};
}

void Engine::DispatchSemanticsAction(int64_t view_id,
                                     int node_id,
                                     SemanticsAction action,
                                     fml::MallocMapping args) {
  runtime_controller_->DispatchSemanticsAction(view_id, node_id, action,
                                               std::move(args));
}

void Engine::SetSemanticsEnabled(bool enabled) {
  runtime_controller_->SetSemanticsEnabled(enabled);
}

void Engine::SetAccessibilityFeatures(int32_t flags) {
  runtime_controller_->SetAccessibilityFeatures(flags);
}

std::string Engine::DefaultRouteName() {
  if (!initial_route_.empty()) {
    return initial_route_;
  }
  return "/";
}

void Engine::ScheduleFrame(bool regenerate_layer_trees) {
  animator_->RequestFrame(regenerate_layer_trees);
}

void Engine::OnAllViewsRendered() {
  animator_->OnAllViewsRendered();
}

void Engine::Render(int64_t view_id,
                    std::unique_ptr<flutter::LayerTree> layer_tree,
                    float device_pixel_ratio) {
  if (!layer_tree) {
    return;
  }

  // Ensure frame dimensions are sane.
  if (layer_tree->frame_size().IsEmpty() || device_pixel_ratio <= 0.0f) {
    return;
  }

  animator_->Render(view_id, std::move(layer_tree), device_pixel_ratio);
}

void Engine::UpdateSemantics(int64_t view_id,
                             SemanticsNodeUpdates update,
                             CustomAccessibilityActionUpdates actions) {
  delegate_.OnEngineUpdateSemantics(view_id, std::move(update),
                                    std::move(actions));
}

void Engine::SetApplicationLocale(std::string locale) {
  delegate_.OnEngineSetApplicationLocale(std::move(locale));
}

void Engine::SetSemanticsTreeEnabled(bool enabled) {
  delegate_.OnEngineSetSemanticsTreeEnabled(enabled);
}

void Engine::HandlePlatformMessage(std::unique_ptr<PlatformMessage> message) {
  if (message->channel() == kAssetChannel) {
    HandleAssetPlatformMessage(std::move(message));
  } else {
    delegate_.OnEngineHandlePlatformMessage(std::move(message));
  }
}

std::unique_ptr<std::vector<std::string>> Engine::ComputePlatformResolvedLocale(
    const std::vector<std::string>& supported_locale_data) {
  return delegate_.ComputePlatformResolvedLocale(supported_locale_data);
}

double Engine::GetScaledFontSize(double unscaled_font_size,
                                 int configuration_id) const {
  return delegate_.GetScaledFontSize(unscaled_font_size, configuration_id);
}

void Engine::RequestViewFocusChange(const ViewFocusChangeRequest& request) {
  delegate_.RequestViewFocusChange(request);
}

void Engine::SetNeedsReportTimings(bool needs_reporting) {
  delegate_.SetNeedsReportTimings(needs_reporting);
}

FontCollection& Engine::GetFontCollection() {
  return *font_collection_;
}

void Engine::DoDispatchPacket(std::unique_ptr<PointerDataPacket> packet,
                              uint64_t trace_flow_id) {
  animator_->EnqueueTraceFlowId(trace_flow_id);
  if (runtime_controller_) {
    runtime_controller_->DispatchPointerDataPacket(*packet);
  }
}

void Engine::ScheduleSecondaryVsyncCallback(uintptr_t id,
                                            const fml::closure& callback) {
  animator_->ScheduleSecondaryVsyncCallback(id, callback);
}

void Engine::HandleAssetPlatformMessage(
    std::unique_ptr<PlatformMessage> message) {
  fml::RefPtr<PlatformMessageResponse> response = message->response();
  if (!response) {
    return;
  }
  const auto& data = message->data();
  std::string asset_name(reinterpret_cast<const char*>(data.GetMapping()),
                         data.GetSize());

  if (asset_manager_) {
    std::unique_ptr<fml::Mapping> asset_mapping =
        asset_manager_->GetAsMapping(asset_name);
    if (asset_mapping) {
      response->Complete(std::move(asset_mapping));
      return;
    }
  }

  response->CompleteEmpty();
}

const std::string& Engine::GetLastEntrypoint() const {
  return last_entry_point_;
}

const std::vector<std::string>& Engine::GetLastEntrypointArgs() const {
  return last_entry_point_args_;
}

std::optional<int64_t> Engine::GetLastEngineId() const {
  return last_engine_id_;
}

std::weak_ptr<PlatformMessageHandler> Engine::GetPlatformMessageHandler()
    const {
  return delegate_.GetPlatformMessageHandler();
}

void Engine::SendChannelUpdate(std::string name, bool listening) {
  delegate_.OnEngineChannelUpdate(std::move(name), listening);
}

const std::weak_ptr<VsyncWaiter> Engine::GetVsyncWaiter() const {
  return animator_->GetVsyncWaiter();
}

void Engine::SetDisplays(const std::vector<DisplayData>& displays) {
  runtime_controller_->SetDisplays(displays);
  ScheduleFrame();
}

}  // namespace flutter
