// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/runtime/runtime_controller.h"

#include <utility>

#include "flutter/fml/logging.h"
#include "flutter/fml/trace_event.h"
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"

namespace flutter {

RuntimeController::RuntimeController(RuntimeDelegate& client,
                                     const TaskRunners& task_runners,
                                     const PlatformData& platform_data)
    : client_(client),
      task_runners_(task_runners),
      platform_data_(platform_data) {}

RuntimeController::~RuntimeController() {
  if (app_ != nullptr) {
    rf_app_destroy(app_);
    app_ = nullptr;
  }
}

bool RuntimeController::LaunchApplication() {
  if (app_ != nullptr) {
    FML_LOG(ERROR) << "The application is already running.";
    return false;
  }

  RfAppHost host = {};
  host.user_data = this;
  host.render = &RuntimeController::OnRender;
  host.schedule_frame = &RuntimeController::OnScheduleFrame;

  app_ = rf_app_create(&host);
  if (app_ == nullptr) {
    FML_LOG(ERROR) << "Could not create the Rust application instance.";
    return false;
  }

  // Replay everything the platform told us before the app existed. Upstream
  // this is FlushRuntimeStateToIsolate, called for the same reason: the
  // embedder configures the shell before there is anything to configure.
  for (const auto& [view_id, metrics] : platform_data_.viewport_metrics_for_views) {
    RfViewMetrics rf_metrics = ToRfViewMetrics(metrics);
    rf_app_add_view(app_, view_id, &rf_metrics);
  }

  if (rf_app_launch(app_) != 0) {
    FML_LOG(ERROR) << "The Rust application entry point failed.";
    rf_app_destroy(app_);
    app_ = nullptr;
    return false;
  }

  return true;
}

bool RuntimeController::IsRunning() const {
  return app_ != nullptr;
}

// -- Views --------------------------------------------------------------------

RfViewMetrics RuntimeController::ToRfViewMetrics(
    const ViewportMetrics& metrics) {
  RfViewMetrics rf = {};
  rf.device_pixel_ratio = metrics.device_pixel_ratio;
  rf.width = metrics.physical_width;
  rf.height = metrics.physical_height;
  rf.padding_top = metrics.physical_padding_top;
  rf.padding_right = metrics.physical_padding_right;
  rf.padding_bottom = metrics.physical_padding_bottom;
  rf.padding_left = metrics.physical_padding_left;
  rf.view_inset_top = metrics.physical_view_inset_top;
  rf.view_inset_right = metrics.physical_view_inset_right;
  rf.view_inset_bottom = metrics.physical_view_inset_bottom;
  rf.view_inset_left = metrics.physical_view_inset_left;
  return rf;
}

void RuntimeController::AddView(int64_t view_id,
                                const ViewportMetrics& view_metrics,
                                AddViewCallback callback) {
  auto [_, inserted] =
      platform_data_.viewport_metrics_for_views.emplace(view_id, view_metrics);
  if (!inserted) {
    FML_LOG(ERROR) << "View #" << view_id << " already exists.";
    if (callback) {
      callback(false);
    }
    return;
  }

  if (app_ != nullptr) {
    RfViewMetrics rf_metrics = ToRfViewMetrics(view_metrics);
    rf_app_add_view(app_, view_id, &rf_metrics);
  }

  // Upstream this callback is deferred until the isolate acknowledges the view.
  // The Rust app is synchronous, so by here the view exists.
  if (callback) {
    callback(true);
  }
}

bool RuntimeController::RemoveView(int64_t view_id) {
  if (platform_data_.viewport_metrics_for_views.erase(view_id) == 0) {
    FML_LOG(ERROR) << "View #" << view_id << " does not exist.";
    return false;
  }
  rendered_views_during_frame_.erase(view_id);

  if (app_ != nullptr) {
    rf_app_remove_view(app_, view_id);
  }
  return true;
}

bool RuntimeController::ViewExists(int64_t view_id) const {
  return platform_data_.viewport_metrics_for_views.count(view_id) != 0;
}

bool RuntimeController::SetViewportMetrics(int64_t view_id,
                                           const ViewportMetrics& metrics) {
  auto found = platform_data_.viewport_metrics_for_views.find(view_id);
  if (found == platform_data_.viewport_metrics_for_views.end()) {
    return false;
  }
  found->second = metrics;

  if (app_ != nullptr) {
    RfViewMetrics rf_metrics = ToRfViewMetrics(metrics);
    rf_app_set_view_metrics(app_, view_id, &rf_metrics);
    return true;
  }
  return false;
}

bool RuntimeController::SendViewFocusEvent(const ViewFocusEvent& event) {
  return app_ != nullptr && ViewExists(event.view_id());
}

// -- Platform state -----------------------------------------------------------

bool RuntimeController::SetDisplays(const std::vector<DisplayData>& displays) {
  platform_data_.displays = displays;
  return app_ != nullptr;
}

bool RuntimeController::SetLocales(
    const std::vector<std::string>& locale_data) {
  platform_data_.locale_data = locale_data;
  return app_ != nullptr;
}

bool RuntimeController::SetUserSettingsData(const std::string& data) {
  platform_data_.user_settings_data = data;
  return app_ != nullptr;
}

bool RuntimeController::SetInitialLifecycleState(const std::string& data) {
  platform_data_.lifecycle_state = data;
  return app_ != nullptr;
}

bool RuntimeController::SetSemanticsEnabled(bool enabled) {
  platform_data_.semantics_enabled = enabled;
  return app_ != nullptr;
}

bool RuntimeController::SetAccessibilityFeatures(int32_t flags) {
  platform_data_.accessibility_feature_flags_ = flags;
  return app_ != nullptr;
}

// -- Frames -------------------------------------------------------------------

bool RuntimeController::BeginFrame(fml::TimePoint frame_time,
                                   uint64_t frame_number) {
  if (app_ == nullptr) {
    return false;
  }

  TRACE_EVENT0("flutter", "RuntimeController::BeginFrame");

  rendered_views_during_frame_.clear();
  frame_in_progress_ = true;

  if (frame_number < last_frame_number_) {
    FML_LOG(ERROR) << "Frame number is out of order: " << frame_number << " < "
                   << last_frame_number_;
  }
  last_frame_number_ = frame_number;

  // A frame is targeted at a vsync deadline, and that deadline is derived from
  // the display's refresh interval -- which VsyncWaiterWin re-reads about once
  // a second, because the rate genuinely changes. When it changes upwards the
  // grid gets finer, and the next target can land *before* the previous one:
  // at 32 Hz a frame is aimed at 156.25 ms, and the 60 Hz frame that follows it
  // is aimed at 150.0 ms.
  //
  // The framework reads this as a clock. AnimationSet::tick already refuses to
  // run backwards, but anything that derives a phase from the raw time -- the
  // gallery's cycle() and ping_pong() do -- would step back for one frame.
  // Upstream clamps at this same boundary, in PlatformConfiguration::BeginFrame,
  // and for the same reason. Clamping here rather than in each consumer means
  // there is one place that decides what "now" is.
  int64_t frame_micros = frame_time.ToEpochDelta().ToMicroseconds();
  if (frame_micros < last_frame_micros_) {
    // Not FML_LOG(WARNING): that level is filtered out of a release build, and
    // this is a condition worth seeing in one -- it means the display changed
    // rate under a running application.
    FML_LOG(ERROR) << "Reported frame time is older than the last one; "
                      "clamping. "
                   << frame_micros << " < " << last_frame_micros_
                   << " ~= " << (last_frame_micros_ - frame_micros);
    frame_micros = last_frame_micros_;
  }
  last_frame_micros_ = frame_micros;

  // Upstream this is two separate hops into Dart: onBeginFrame runs tickers and
  // transitions, then onDrawFrame builds, lays out and paints. Keeping them
  // apart matters -- an animation that starts during onBeginFrame must be
  // visible to the build that follows it in the same frame.
  //
  // Upstream drains the microtask queue between the two (FlushMicrotasksNow),
  // and that position is load-bearing: dart:ui's own scheduleWarmUpFrame uses
  // two timers rather than one "to ensure that microtasks flush in between".
  // There is no async runtime here yet, so nothing is queued and nothing is
  // drained -- but this is where it would go.
  rf_app_begin_frame(app_, frame_micros, frame_number);
  rf_app_draw_frame(app_);

  frame_in_progress_ = false;
  return true;
}

bool RuntimeController::ReportTimings(std::vector<int64_t> timings) {
  // The Rust framework has no equivalent of dart:developer Timeline reporting
  // yet; the timings are still gathered by the shell for the tracing UI.
  return app_ != nullptr;
}

bool RuntimeController::NotifyIdle(fml::TimeDelta deadline) {
  // Upstream this hands the remaining frame budget to the Dart GC. Rust has no
  // GC to run, so there is nothing to do but report that the app is alive.
  return app_ != nullptr;
}

// -- Input --------------------------------------------------------------------

bool RuntimeController::DispatchPlatformMessage(
    std::unique_ptr<PlatformMessage> message) {
  if (app_ == nullptr) {
    return false;
  }
  // Platform channels are M4 work; the message is dropped rather than leaked.
  return true;
}

bool RuntimeController::DispatchPointerDataPacket(
    const PointerDataPacket& packet) {
  if (app_ == nullptr) {
    return false;
  }
  TRACE_EVENT0("flutter", "RuntimeController::DispatchPointerDataPacket");

  // Narrow flutter::PointerData to the fields the framework uses. Doing it
  // here rather than in Rust keeps the struct layout in one language: a
  // mirrored #[repr(C)] would silently drift the first time upstream adds a
  // field in the middle.
  const size_t count = packet.GetLength();
  std::vector<RfPointerEvent> events;
  events.reserve(count);
  for (size_t i = 0; i < count; ++i) {
    const PointerData data = packet.GetPointerData(i);
    RfPointerEvent event = {};
    event.view_id = data.view_id;
    event.device = data.device;
    event.pointer_id = data.pointer_identifier;
    event.change = static_cast<int32_t>(data.change);
    event.kind = static_cast<int32_t>(data.kind);
    event.signal_kind = static_cast<int32_t>(data.signal_kind);
    event.buttons = static_cast<int32_t>(data.buttons);
    event.time_stamp_micros = data.time_stamp;
    event.physical_x = data.physical_x;
    event.physical_y = data.physical_y;
    event.delta_x = data.physical_delta_x;
    event.delta_y = data.physical_delta_y;
    event.scroll_delta_x = data.scroll_delta_x;
    event.scroll_delta_y = data.scroll_delta_y;
    event.pressure = data.pressure;
    events.push_back(event);
  }

  if (!events.empty()) {
    rf_app_dispatch_pointers(app_, events.data(), events.size());
  }
  return true;
}

HitTestResponse RuntimeController::HitTest(int64_t view_id,
                                           const PointData offset) {
  // This answers one question only -- whether a platform view is under the
  // pointer, so the embedder knows whether to hand the gesture to a native
  // view. There are no platform views yet, so the answer is always no. The
  // framework's own hit testing happens on the Rust side, against the render
  // tree, and does not come through here.
  return HitTestResponse{.has_platform_view = false};
}

bool RuntimeController::DispatchSemanticsAction(int64_t view_id,
                                                int32_t node_id,
                                                SemanticsAction action,
                                                fml::MallocMapping args) {
  return app_ != nullptr;
}

// -- Callbacks from the framework ---------------------------------------------

void RuntimeController::OnRender(void* user_data,
                                 int64_t view_id,
                                 RfLayerTree* tree,
                                 double device_pixel_ratio) {
  auto* self = static_cast<RuntimeController*>(user_data);
  std::unique_ptr<LayerTree> layer_tree = RfLayerTreeTake(tree);
  if (layer_tree == nullptr) {
    FML_LOG(ERROR) << "The framework produced an empty layer tree for view #"
                   << view_id << ".";
    return;
  }

  self->rendered_views_during_frame_.insert(view_id);
  self->client_.Render(view_id, std::move(layer_tree),
                       static_cast<float>(device_pixel_ratio));
  self->CheckIfAllViewsRendered();
}

void RuntimeController::OnScheduleFrame(void* user_data) {
  auto* self = static_cast<RuntimeController*>(user_data);
  self->client_.ScheduleFrame(true);
}

void RuntimeController::CheckIfAllViewsRendered() {
  if (!frame_in_progress_) {
    return;
  }
  if (rendered_views_during_frame_.size() !=
      platform_data_.viewport_metrics_for_views.size()) {
    return;
  }
  client_.OnAllViewsRendered();
  rendered_views_during_frame_.clear();
}

}  // namespace flutter
