// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/runtime/runtime_controller.h"

#include <cstring>
#include <utility>
#include <vector>

#include "flutter/fml/logging.h"
#include "flutter/fml/make_copyable.h"
#include "flutter/fml/trace_event.h"
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"

namespace flutter {
namespace {

/// The channel every Flutter embedder sends key events on. Defined in
/// embedder.cc as kFlutterKeyDataChannel, in platform_dispatcher.dart as
/// _kFlutterKeyDataChannel, and in KeyData.java as KeyData.CHANNEL. Four
/// copies of one string upstream; this is the fifth, and it has to match.
constexpr char kKeyDataChannel[] = "flutter/keydata";

//------------------------------------------------------------------------------
/// The reply to a message the framework sent.
///
/// Upstream's counterpart is PlatformMessageResponseDart, and the two problems
/// it solves are the two solved here. First, `Complete` is callable from any
/// thread -- the embedder answers wherever it happens to be, and the framework
/// may only be touched on the UI thread -- so the answer is posted rather than
/// delivered. Second, by the time it lands the controller may be gone: an
/// application that shuts down with a clipboard read outstanding is ordinary,
/// so the reference is weak and the task simply does nothing if it has expired.
///
/// The framework's id travels with it because the C ABI cannot carry a
/// ref-counted C++ object; see RuntimeController::pending_responses_ for the
/// same trick in the other direction.
class RustPlatformMessageResponse : public PlatformMessageResponse {
 public:
  void Complete(std::unique_ptr<fml::Mapping> data) override {
    Post(std::move(data));
  }

  void CompleteEmpty() override { Post(nullptr); }

 private:
  RustPlatformMessageResponse(fml::RefPtr<fml::TaskRunner> ui_task_runner,
                              fml::WeakPtr<RuntimeController> controller,
                              int64_t response_id)
      : ui_task_runner_(std::move(ui_task_runner)),
        controller_(std::move(controller)),
        response_id_(response_id) {}

  void Post(std::unique_ptr<fml::Mapping> data) {
    if (is_complete_) {
      // A response completed twice would deliver a reply to a caller that has
      // already been answered and released.
      FML_LOG(ERROR) << "Platform message response completed more than once.";
      return;
    }
    is_complete_ = true;
    if (!ui_task_runner_) {
      return;
    }
    ui_task_runner_->PostTask(fml::MakeCopyable(
        [controller = controller_, response_id = response_id_,
         data = std::move(data)]() mutable {
          if (controller) {
            controller->CompletePlatformMessageReply(response_id,
                                                     std::move(data));
          }
        }));
  }

  fml::RefPtr<fml::TaskRunner> ui_task_runner_;
  fml::WeakPtr<RuntimeController> controller_;
  int64_t response_id_ = 0;

  FML_FRIEND_MAKE_REF_COUNTED(RustPlatformMessageResponse);
  FML_FRIEND_REF_COUNTED_THREAD_SAFE(RustPlatformMessageResponse);
  FML_DISALLOW_COPY_AND_ASSIGN(RustPlatformMessageResponse);
};

}  // namespace

RuntimeController::RuntimeController(RuntimeDelegate& client,
                                     const TaskRunners& task_runners,
                                     const PlatformData& platform_data)
    : client_(client),
      task_runners_(task_runners),
      platform_data_(platform_data),
      weak_factory_(this) {}

RuntimeController::~RuntimeController() {
  // Anything the framework was still holding is answered with nothing. The
  // embedder is waiting on these, and an unanswered response handle is a caller
  // that never learns its message went nowhere -- on Windows that is a platform
  // thread task that never runs.
  for (auto& [response_id, response] : pending_responses_) {
    response->CompleteEmpty();
  }
  pending_responses_.clear();

  if (app_ != nullptr) {
    rf_app_destroy(app_);
    app_ = nullptr;
  }
}

// The other half of the check in `rust/rustflutter/src/app.rs`. Two
// hand-written mirrors of one ABI; a field added to one side and not the other
// would otherwise be read as the next field's bytes.
static_assert(sizeof(RfAppHost) == sizeof(void*) * 9,
              "RfAppHost has drifted from its Rust mirror in app.rs");

bool RuntimeController::LaunchApplication() {
  if (app_ != nullptr) {
    FML_LOG(ERROR) << "The application is already running.";
    return false;
  }

  RfAppHost host = {};
  host.user_data = this;
  host.render = &RuntimeController::OnRender;
  host.schedule_frame = &RuntimeController::OnScheduleFrame;
  host.send_platform_message = &RuntimeController::OnSendPlatformMessage;
  host.respond_to_platform_message =
      &RuntimeController::OnRespondToPlatformMessage;
  host.send_channel_update = &RuntimeController::OnSendChannelUpdate;
  host.update_semantics = &RuntimeController::OnUpdateSemantics;
  host.post_task = &RuntimeController::OnPostTask;
  host.post_delayed_task = &RuntimeController::OnPostDelayedTask;

  // Taken here, on the UI thread, because OnPostTask may run on any other one
  // and the factory is not thread-safe to ask. Copying the result is.
  weak_for_tasks_ = weak_factory_.GetWeakPtr();

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
  if (!platform_data_.user_settings_data.empty()) {
    rf_app_set_user_settings(app_, platform_data_.user_settings_data.data(),
                             platform_data_.user_settings_data.size());
  }
  PushLocales();

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
  if (app_ == nullptr) {
    return false;
  }
  PushLocales();
  return true;
}

bool RuntimeController::SetUserSettingsData(const std::string& data) {
  platform_data_.user_settings_data = data;
  if (app_ == nullptr) {
    return false;
  }
  rf_app_set_user_settings(app_, data.data(), data.size());
  return true;
}

void RuntimeController::PushLocales() {
  // Four strings per locale, which is the shape `flutter/localization` already
  // carries; a trailing partial group would mean the message was malformed, and
  // Engine::HandleLocalizationPlatformMessage rejects those before they get
  // here.
  constexpr size_t kStringsPerLocale = 4;
  const auto& data = platform_data_.locale_data;
  const size_t count = data.size() / kStringsPerLocale;
  if (count == 0) {
    return;
  }
  std::vector<const char*> pointers;
  pointers.reserve(count * kStringsPerLocale);
  for (size_t i = 0; i < count * kStringsPerLocale; ++i) {
    pointers.push_back(data[i].c_str());
  }
  rf_app_set_locales(app_, pointers.data(), count);
}

bool RuntimeController::SetInitialLifecycleState(const std::string& data) {
  platform_data_.lifecycle_state = data;
  return app_ != nullptr;
}

bool RuntimeController::SetSemanticsEnabled(bool enabled) {
  platform_data_.semantics_enabled = enabled;
  if (app_ == nullptr) {
    return false;
  }
  // The framework builds no semantics tree until it is told one is being read,
  // which is upstream's arrangement (`SemanticsBinding.semanticsEnabled`) and
  // is why this is a message rather than a flag the shell keeps to itself.
  rf_app_set_semantics_enabled(app_, enabled);
  return true;
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
  // rf_app_run_tasks is that drain -- a task that completes while tickers are
  // advancing has to be visible to the build that follows it, for the same
  // reason an animation started in onBeginFrame does.
  rf_app_begin_frame(app_, frame_micros, frame_number);
  rf_app_run_tasks(app_);
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
  if (message->channel() == kKeyDataChannel) {
    // Not handed to the messenger, and upstream does not hand it over either:
    // `flutter/keydata` is the one channel dart:ui reads directly, in
    // PlatformDispatcher rather than through a MethodChannel, precisely so that
    // input does not queue behind the plugin system. Its payload is a packed
    // KeyDataPacket rather than a codec's output, so there is nothing a channel
    // could do with it anyway.
    DispatchKeyDataPacket(*message);
    return true;
  }

  if (app_ == nullptr) {
    return false;
  }

  // A response handle is a ref-counted C++ object and cannot cross a C ABI, so
  // it stays here and an integer goes over in its place. Zero means the
  // embedder wants no reply, which is most messages.
  int64_t response_id = 0;
  if (message->response()) {
    response_id = next_response_id_++;
    pending_responses_[response_id] = message->response();
  }

  const auto& data = message->data();
  rf_app_dispatch_platform_message(app_, message->channel().c_str(),
                                   data.GetMapping(), data.GetSize(),
                                   response_id);
  return true;
}

void RuntimeController::CompletePlatformMessageReply(
    int64_t response_id,
    std::unique_ptr<fml::Mapping> data) {
  if (app_ == nullptr) {
    return;
  }
  if (data == nullptr) {
    // Nothing handled the message. The framework tells this apart from an
    // empty reply -- it is what raises MissingPluginException upstream -- so
    // the null has to survive the crossing rather than becoming zero bytes.
    rf_app_complete_platform_message_reply(app_, response_id, nullptr, 0);
    return;
  }
  rf_app_complete_platform_message_reply(app_, response_id, data->GetMapping(),
                                         data->GetSize());
}

void RuntimeController::OnSendPlatformMessage(void* user_data,
                                              const char* channel,
                                              const uint8_t* message,
                                              size_t length,
                                              int64_t response_id) {
  auto* controller = static_cast<RuntimeController*>(user_data);
  if (controller == nullptr || channel == nullptr) {
    return;
  }

  fml::RefPtr<PlatformMessageResponse> response;
  if (response_id != 0) {
    response = fml::MakeRefCounted<RustPlatformMessageResponse>(
        controller->task_runners_.GetUITaskRunner(),
        controller->weak_factory_.GetWeakPtr(), response_id);
  }

  auto platform_message = std::make_unique<PlatformMessage>(
      std::string(channel),
      length == 0 ? fml::MallocMapping()
                  : fml::MallocMapping::Copy(message, length),
      std::move(response));

  // Straight to the delegate, which is Engine::HandlePlatformMessage: it
  // answers `flutter/assets` itself and forwards everything else out through
  // the shell to the embedder. The same route dart:ui's sendPlatformMessage
  // takes, minus the isolate.
  controller->client_.HandlePlatformMessage(std::move(platform_message));
}

void RuntimeController::OnRespondToPlatformMessage(void* user_data,
                                                   int64_t response_id,
                                                   const uint8_t* reply,
                                                   size_t length) {
  auto* controller = static_cast<RuntimeController*>(user_data);
  if (controller == nullptr) {
    return;
  }
  auto found = controller->pending_responses_.find(response_id);
  if (found == controller->pending_responses_.end()) {
    // Answered twice, or answered after the message was abandoned. Not fatal;
    // the handle has already been released either way.
    return;
  }
  auto response = found->second;
  controller->pending_responses_.erase(found);

  if (reply == nullptr) {
    response->CompleteEmpty();
    return;
  }
  response->Complete(std::make_unique<fml::DataMapping>(
      std::vector<uint8_t>(reply, reply + length)));
}

void RuntimeController::OnUpdateSemantics(void* user_data,
                                          int64_t view_id,
                                          const RfSemanticsNode* nodes,
                                          size_t count) {
  auto* controller = static_cast<RuntimeController*>(user_data);
  if (controller == nullptr || (nodes == nullptr && count > 0)) {
    return;
  }
  TRACE_EVENT0("flutter", "RuntimeController::OnUpdateSemantics");

  // Copied rather than referenced: everything the framework passed is valid
  // only for the duration of this call, and what is built here travels to the
  // platform thread. Upstream's SemanticsUpdate does the same copying, one
  // layer further out.
  SemanticsNodeUpdates update;
  update.reserve(count);
  for (size_t i = 0; i < count; ++i) {
    const RfSemanticsNode& in = nodes[i];
    SemanticsNode out;
    out.id = in.id;
    out.actions = in.actions;

    out.flags.isButton = (in.flags & kRfSemanticsIsButton) != 0;
    out.flags.isTextField = (in.flags & kRfSemanticsIsTextField) != 0;
    out.flags.isHeader = (in.flags & kRfSemanticsIsHeader) != 0;
    out.flags.isImage = (in.flags & kRfSemanticsIsImage) != 0;
    out.flags.isLink = (in.flags & kRfSemanticsIsLink) != 0;
    out.flags.isSlider = (in.flags & kRfSemanticsIsSlider) != 0;
    out.flags.isObscured = (in.flags & kRfSemanticsIsObscured) != 0;
    out.flags.isReadOnly = (in.flags & kRfSemanticsIsReadOnly) != 0;
    out.flags.isLiveRegion = (in.flags & kRfSemanticsIsLiveRegion) != 0;
    // The tristates say three things, and "has no checked state at all" is one
    // of them: kNone is what stops a screen reader announcing "not checked"
    // about a thing that was never checkable.
    if ((in.flags & kRfSemanticsHasCheckedState) != 0) {
      out.flags.isChecked = (in.flags & kRfSemanticsIsChecked) != 0
                                ? SemanticsCheckState::kTrue
                                : SemanticsCheckState::kFalse;
    }
    if ((in.flags & kRfSemanticsHasEnabledState) != 0) {
      out.flags.isEnabled = (in.flags & kRfSemanticsIsEnabled) != 0
                                ? SemanticsTristate::kTrue
                                : SemanticsTristate::kFalse;
    }
    out.flags.isSelected = (in.flags & kRfSemanticsIsSelected) != 0
                               ? SemanticsTristate::kTrue
                               : SemanticsTristate::kNone;
    out.flags.isFocused = (in.flags & kRfSemanticsIsFocused) != 0
                              ? SemanticsTristate::kTrue
                              : SemanticsTristate::kNone;

    const auto text = [](const char* value) {
      return value == nullptr ? std::string() : std::string(value);
    };
    out.label = text(in.label);
    out.value = text(in.value);
    out.hint = text(in.hint);
    out.increasedValue = text(in.increased_value);
    out.decreasedValue = text(in.decreased_value);

    out.rect = SkRect::MakeLTRB(in.left, in.top, in.right, in.bottom);
    out.scrollPosition = in.scroll_position;
    out.scrollExtentMin = in.scroll_extent_min;
    out.scrollExtentMax = in.scroll_extent_max;
    // The framework's reading direction for everything this node says, in
    // the embedder's encoding (0=unknown, 1=rtl, 2=ltr). The framework gives
    // every node it labels the direction that label was built in, and a node
    // with nothing to read crosses as unknown -- the same null that upstream
    // sends through SemanticsUpdateBuilder.updateNode.
    out.textDirection = in.text_direction;

    if (in.children != nullptr) {
      out.childrenInTraversalOrder.assign(in.children,
                                          in.children + in.child_count);
      // The same order. They differ upstream only where a widget deliberately
      // separates reading order from hit-test order, which nothing here does.
      out.childrenInHitTestOrder = out.childrenInTraversalOrder;
    }
    update.emplace(out.id, std::move(out));
  }

  controller->client_.UpdateSemantics(view_id, std::move(update), {});
}

void RuntimeController::OnSendChannelUpdate(void* user_data,
                                            const char* channel,
                                            bool listening) {
  auto* controller = static_cast<RuntimeController*>(user_data);
  if (controller == nullptr || channel == nullptr) {
    return;
  }
  controller->client_.SendChannelUpdate(std::string(channel), listening);
}

bool RuntimeController::DispatchKeyDataPacket(const PlatformMessage& message) {
  TRACE_EVENT0("flutter", "RuntimeController::DispatchKeyDataPacket");

  // | char size | KeyData | char bytes |, as built by KeyDataPacket and
  // unpacked by _unpackKeyData in platform_dispatcher.dart. The two ends are
  // the same file upstream; here one end is this and the other is the host's
  // KeyDataPacket, which is the same class, so the layout cannot drift.
  constexpr size_t kHeader = sizeof(uint64_t) + sizeof(KeyData);

  // Whatever happens below, the embedder is told something. It is waiting on
  // this answer before it decides whether to give the key back to the system,
  // so a dropped reply is not a lost log line -- it is a key that never
  // reaches anything.
  const auto answer = [&message](bool handled) {
    if (const auto& response = message.response()) {
      std::vector<uint8_t> reply{static_cast<uint8_t>(handled ? 1 : 0)};
      response->Complete(std::make_unique<fml::DataMapping>(std::move(reply)));
    }
  };

  const uint8_t* bytes = message.data().GetMapping();
  const size_t size = message.data().GetSize();
  if (bytes == nullptr || size < kHeader) {
    FML_LOG(ERROR) << "Malformed key packet: " << size << " bytes.";
    answer(false);
    return false;
  }

  uint64_t character_size = 0;
  memcpy(&character_size, bytes, sizeof(character_size));
  if (character_size > size - kHeader) {
    FML_LOG(ERROR) << "Key packet claims " << character_size
                   << " character bytes but holds " << (size - kHeader) << ".";
    answer(false);
    return false;
  }

  KeyData data = {};
  memcpy(&data, bytes + sizeof(uint64_t), sizeof(KeyData));

  // Copied rather than pointed at, because the framework is handed a
  // NUL-terminated C string and the packet's bytes are not terminated.
  const std::string character(
      reinterpret_cast<const char*>(bytes + kHeader),
      static_cast<size_t>(character_size));

  // Narrowed the same way, and for the same reason, as PointerData above.
  // `device_type` is dropped: every key this shell sees comes from a keyboard.
  RfKeyEvent event = {};
  event.time_stamp_micros = static_cast<int64_t>(data.timestamp);
  event.change = static_cast<int32_t>(data.type);
  event.physical = data.physical;
  event.logical = data.logical;
  event.synthesized = data.synthesized != 0;
  event.character = character.empty() ? nullptr : character.c_str();

  // An application that has not started yet cannot have an opinion, and
  // `rf_app_dispatch_key` says so for a null app; the answer still has to go
  // back, because the embedder is holding the key until it arrives.
  const bool handled = rf_app_dispatch_key(app_, &event);

  // One byte, 1 for handled -- the same reply `_keyDataListener` writes. The
  // Windows host reads it: a key the framework declines is put back into the
  // system's queue, which is what lets the framework genuinely consume one.
  answer(handled);
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
  if (app_ == nullptr) {
    return false;
  }
  // `args` is dropped. Upstream it carries the payload of the two actions that
  // have one -- `setSelection` and `setText` -- and neither has an equivalent
  // on this side yet; every action the framework offers is an action with no
  // arguments. When one grows arguments this is where they arrive.
  return rf_app_dispatch_semantics_action(app_, node_id,
                                          static_cast<int32_t>(action));
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

void RuntimeController::OnPostTask(void* user_data) {
  auto* self = static_cast<RuntimeController*>(user_data);
  auto ui_task_runner = self->task_runners_.GetUITaskRunner();
  if (!ui_task_runner) {
    return;
  }
  // The copy is taken here and dereferenced there: this function may be on a
  // decode worker, and the task runs on the UI thread where the factory lives.
  ui_task_runner->PostTask([weak = self->weak_for_tasks_]() {
    if (weak && weak->app_ != nullptr) {
      rf_app_run_tasks(weak->app_);
    }
  });
}

void RuntimeController::OnPostDelayedTask(void* user_data, int64_t delay_micros) {
  auto* self = static_cast<RuntimeController*>(user_data);
  auto ui_task_runner = self->task_runners_.GetUITaskRunner();
  if (!ui_task_runner) {
    return;
  }
  ui_task_runner->PostDelayedTask(
      [weak = self->weak_for_tasks_]() {
        if (weak && weak->app_ != nullptr) {
          rf_app_run_tasks(weak->app_);
        }
      },
      fml::TimeDelta::FromMicroseconds(delay_micros));
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
