// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// The Android host: an Activity's Surface, the engine's own thread model, and a
// real Shell driving the Rust framework.
//
// The same job rustflutter_host_win.cc does, with three structural differences
// that are worth stating before the code, because everything else follows from
// them.
//
//   * **The platform thread is the Android main thread.** Upstream's Android
//     embedder does this too, and it is the opposite of the Windows host, where
//     the window thread and the platform thread are deliberately separate. It
//     works here because fml has an ALooper-backed message loop: initialising
//     it on the main thread hangs a timerfd off the Looper that Android is
//     already polling, so a task posted to the platform runner runs on the UI
//     thread with no second loop and no interleaving. That in turn means every
//     JNI call this file makes lands on the thread Android insists UI work
//     happens on, which removes a whole category of hop.
//
//   * **The host does not own the loop.** On Windows `rf_host_run` opens a
//     window and pumps messages until it closes. There is no equivalent here:
//     Android owns the Activity, the Looper and the Surface, so `rf_host_run`
//     sets the shell up against the Surface that already exists and returns.
//     What would have been the message loop is the Activity's lifecycle, and it
//     arrives through the JNI entry points at the bottom of this file.
//
//   * **What the platform knows, Java knows first.** Brightness, text scale,
//     the 24-hour preference and the locale list are read from
//     `Configuration`, not from the OS by hand, so they arrive as JSON from
//     Java rather than being assembled here. The channels and their payloads
//     are the same ones the Windows host sends, because the framework reading
//     them is the same.
//
// Rendering is Impeller on the device's own GLES, through the same
// rustflutter_gl.cc the Windows host uses -- that file names no platform, which
// is why it is shared rather than copied.

#include "flutter/rust/host/rustflutter_host.h"

#include <android/choreographer.h>
#include <android/log.h>
#include <android/native_window.h>
#include <android/native_window_jni.h>
#include <jni.h>
#include <pthread.h>
#include <stdlib.h>
#include <unistd.h>

#include <algorithm>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <vector>

#include "flutter/common/constants.h"
#include "flutter/common/settings.h"
#include "flutter/common/task_runners.h"
#include "flutter/fml/logging.h"
#include "flutter/fml/make_copyable.h"
#include "flutter/fml/message_loop.h"
#include "flutter/fml/platform/android/jni_util.h"
#include "flutter/fml/platform/android/scoped_java_ref.h"
#include "flutter/fml/synchronization/waitable_event.h"
#include "flutter/fml/task_runner.h"
#include "flutter/fml/time/time_point.h"
#include "flutter/impeller/renderer/context.h"
#include "flutter/lib/ui/window/platform_message.h"
#include "flutter/lib/ui/window/pointer_data.h"
#include "flutter/lib/ui/window/pointer_data_packet.h"
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"
#include "flutter/rust/host/rustflutter_gl.h"
#include "flutter/shell/common/display.h"
#include "flutter/shell/common/platform_view.h"
#include "flutter/shell/common/rasterizer.h"
#include "flutter/shell/common/run_configuration.h"
#include "flutter/shell/common/shell.h"
#include "flutter/shell/common/thread_host.h"
#include "flutter/shell/common/vsync_waiter.h"
#include "flutter/shell/gpu/gpu_surface_gl_impeller.h"
#include "flutter/shell/gpu/gpu_surface_software.h"
#include "flutter/shell/gpu/gpu_surface_software_delegate.h"
#include "flutter/shell/platform/common/text_input_model.h"
#include "flutter/shell/platform/common/text_range.h"
#include "rapidjson/document.h"
#include "rapidjson/stringbuffer.h"
#include "rapidjson/writer.h"
#include "third_party/skia/include/core/SkSurface.h"

namespace flutter {
namespace {

constexpr char kPlatformChannel[] = "flutter/platform";
constexpr char kLifecycleChannel[] = "flutter/lifecycle";
constexpr char kNavigationChannel[] = "flutter/navigation";
constexpr char kSettingsChannel[] = "flutter/settings";
constexpr char kLocalizationChannel[] = "flutter/localization";
constexpr char kTextInputChannel[] = "flutter/textinput";

constexpr char kClipboardError[] = "Clipboard error";
constexpr char kUnknownClipboardFormatMessage[] = "Unknown clipboard format";
constexpr char kTextPlainFormat[] = "text/plain";

constexpr char kExitRequestError[] = "ExitApplication error";
constexpr char kInvalidExitRequestMessage[] = "Invalid application exit request";
constexpr char kExitTypeCancelable[] = "cancelable";
constexpr char kExitTypeRequired[] = "required";

//------------------------------------------------------------------------------
// JSON envelopes.
//
// The same three shapes the Windows host writes, because they are the codec's
// and not the platform's: a success is the result inside a one-element array,
// an error is code / message / details inside a three-element one.

std::string SuccessEnvelope(
    const std::function<void(rapidjson::Writer<rapidjson::StringBuffer>&)>&
        write_result) {
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartArray();
  write_result(writer);
  writer.EndArray();
  return std::string(buffer.GetString(), buffer.GetSize());
}

std::string NullEnvelope() {
  return SuccessEnvelope([](auto& writer) { writer.Null(); });
}

std::string ErrorEnvelope(const char* code, const char* message) {
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartArray();
  writer.String(code);
  writer.String(message);
  writer.Null();
  writer.EndArray();
  return std::string(buffer.GetString(), buffer.GetSize());
}

//------------------------------------------------------------------------------
/// Sends whatever the process writes to stdout and stderr to logcat.
///
/// On a desktop an application's `println!` lands in the terminal it was run
/// from. An Android process has no terminal: its stdout goes to /dev/null, so
/// every diagnostic an example prints -- and every Rust panic message, which
/// goes to stderr -- disappears. Reading it back is the difference between a
/// device being debuggable and not.
///
/// A pipe, the two descriptors pointed at its write end, and a thread draining
/// the read end. Upstream does the same thing for Dart's `print`, one layer
/// further up.
void SendOutputToLogcat() {
  static bool started = false;
  if (started) {
    return;
  }
  started = true;

  static int pipe_fds[2];
  if (pipe(pipe_fds) != 0) {
    return;
  }
  // Line buffered: without this, a `print!` with no newline would sit in libc's
  // buffer until the buffer filled, which for a diagnostic is never.
  setvbuf(stdout, nullptr, _IOLBF, 0);
  setvbuf(stderr, nullptr, _IONBF, 0);
  dup2(pipe_fds[1], STDOUT_FILENO);
  dup2(pipe_fds[1], STDERR_FILENO);

  pthread_t thread;
  pthread_create(
      &thread, nullptr,
      [](void*) -> void* {
        std::string line;
        char buffer[512];
        ssize_t count;
        while ((count = read(pipe_fds[0], buffer, sizeof(buffer))) > 0) {
          for (ssize_t index = 0; index < count; ++index) {
            if (buffer[index] == '\n') {
              __android_log_write(ANDROID_LOG_INFO, "rustflutter", line.c_str());
              line.clear();
            } else if (buffer[index] != '\r') {
              line.push_back(buffer[index]);
            }
          }
        }
        return nullptr;
      },
      nullptr);
  pthread_detach(thread);
}

//------------------------------------------------------------------------------
// The Java side.
//
// One static method takes every request that needs an Android API -- the soft
// keyboard, the clipboard, finishing the Activity -- because the alternative is
// a method id per verb and the verbs are all "a string in, maybe a string out".
// Upstream spreads these across a plugin each; at this size the seam is not
// worth the surface.
//
// Every call below happens on the platform thread, which is the Android main
// thread, which is where these APIs must be called from anyway.

/// Must match RustflutterActivity.HOST_*.
enum class HostRequest : int32_t {
  kShowKeyboard = 0,
  kHideKeyboard = 1,
  kFinish = 2,
  kClipboardGet = 3,
  kClipboardSet = 4,
  kClipboardHasStrings = 5,
  kSetTaskLabel = 6,
  kRestartInput = 7,
};

class JavaBridge {
 public:
  /// Caches the class and the two methods. Called once, from JNI_OnLoad.
  static void Initialise(JNIEnv* env, jclass activity_class) {
    class_ = static_cast<jclass>(env->NewGlobalRef(activity_class));
    request_ = env->GetStaticMethodID(class_, "onHostRequest",
                                      "(ILjava/lang/String;)Ljava/lang/String;");
    editing_ = env->GetStaticMethodID(class_, "onEditingState",
                                      "(Ljava/lang/String;IIII)V");
    FML_CHECK(request_ != nullptr && editing_ != nullptr)
        << "RustflutterActivity is missing a method the host calls.";
  }

  /// Sends one request to Java. Returns the answer, or nothing for a null one.
  static std::optional<std::string> Request(HostRequest what,
                                            const std::string& argument) {
    if (class_ == nullptr) {
      return std::nullopt;
    }
    JNIEnv* env = fml::jni::AttachCurrentThread();
    fml::jni::ScopedJavaLocalRef<jstring> java_argument =
        fml::jni::StringToJavaString(env, argument);
    auto answer = static_cast<jstring>(env->CallStaticObjectMethod(
        class_, request_, static_cast<jint>(what), java_argument.obj()));
    // `CheckException` is true when the call was *clean*; it logs and clears
    // anything that was thrown.
    if (!fml::jni::CheckException(env) || answer == nullptr) {
      return std::nullopt;
    }
    std::string result = fml::jni::JavaStringToString(env, answer);
    env->DeleteLocalRef(answer);
    return result;
  }

  /// Mirrors the framework's editing state into the Android input connection.
  ///
  /// The IME keeps its own idea of what the field holds, and it is only ever
  /// right by accident unless somebody tells it. Upstream's `TextInputPlugin`
  /// mirrors into an `Editable` for the same reason.
  static void SetEditingState(const std::string& text,
                              int selection_base,
                              int selection_extent,
                              int composing_base,
                              int composing_extent) {
    if (class_ == nullptr) {
      return;
    }
    JNIEnv* env = fml::jni::AttachCurrentThread();
    fml::jni::ScopedJavaLocalRef<jstring> java_text =
        fml::jni::StringToJavaString(env, text);
    env->CallStaticVoidMethod(class_, editing_, java_text.obj(), selection_base,
                              selection_extent, composing_base,
                              composing_extent);
    // Logs and clears anything thrown. There is nothing to do about it here:
    // the framework's model is still right, and the IME will be told again on
    // the next edit.
    fml::jni::CheckException(env);
  }

 private:
  static jclass class_;
  static jmethodID request_;
  static jmethodID editing_;
};

jclass JavaBridge::class_ = nullptr;
jmethodID JavaBridge::request_ = nullptr;
jmethodID JavaBridge::editing_ = nullptr;

//------------------------------------------------------------------------------
/// Frames, from the display's own clock.
///
/// Upstream's `VsyncWaiterAndroid` asks Java's `Choreographer`; the NDK exposes
/// the same object directly, and this host is already on a thread with a Looper
/// -- which is `AChoreographer_getInstance`'s one requirement -- so it asks the
/// NDK and skips the trip through Java.
///
/// `AwaitVSync` is called on the UI thread. The callback has to be posted from
/// a Looper thread and arrives on it, so both hops go through the platform task
/// runner, which is the Android main thread.
class ChoreographerVsyncWaiter final : public VsyncWaiter {
 public:
  explicit ChoreographerVsyncWaiter(const TaskRunners& task_runners)
      : VsyncWaiter(task_runners) {}

  ~ChoreographerVsyncWaiter() override = default;

  /// The display's refresh interval, from Java. Set before the shell starts.
  static void SetRefreshRate(double hertz) {
    if (hertz > 1.0) {
      interval_micros_.store(static_cast<int64_t>(1000000.0 / hertz),
                             std::memory_order_relaxed);
    }
  }

 private:
  // |VsyncWaiter|
  void AwaitVSync() override {
    // A weak reference, on the heap, which is what upstream's
    // `VsyncHelper#asyncWaitForVsync` passes through Java for the same reason:
    // a posted frame callback cannot be cancelled. There is no
    // `AChoreographer_removeFrameCallback`, so when the Activity finishes
    // between one vsync and the next, the callback still arrives -- with a
    // pointer to a waiter the shell has already destroyed. A weak_ptr turns
    // that from a use-after-free into a lock that fails.
    //
    // The baton is deleted by whoever consumes it, on either path.
    auto* weak = new std::weak_ptr<VsyncWaiter>(shared_from_this());
    task_runners_.GetPlatformTaskRunner()->PostTask([weak]() {
      AChoreographer* choreographer = AChoreographer_getInstance();
      if (choreographer == nullptr) {
        // No Looper on this thread, which should not happen -- but a frame
        // that never arrives is a black screen, so fall back to firing now.
        Deliver(weak, fml::TimePoint::Now().ToEpochDelta().ToNanoseconds());
        return;
      }
      AChoreographer_postFrameCallback(
          choreographer,
          [](long frame_time_nanos, void* data) {
            Deliver(static_cast<std::weak_ptr<VsyncWaiter>*>(data),
                    static_cast<int64_t>(frame_time_nanos));
          },
          weak);
    });
  }

  /// Fires the waiter if it is still there, and frees the baton either way.
  static void Deliver(std::weak_ptr<VsyncWaiter>* weak, int64_t nanos) {
    if (auto waiter = weak->lock()) {
      static_cast<ChoreographerVsyncWaiter*>(waiter.get())->Fire(nanos);
    }
    delete weak;
  }

  /// Hands the engine the frame it was waiting for.
  ///
  /// The start is when the display last flipped, not now: a callback that
  /// arrives late still belongs to the frame it was scheduled for, and telling
  /// the engine otherwise makes every animation drift by the scheduling delay.
  void Fire(int64_t frame_time_nanos) {
    const auto start = fml::TimePoint::FromEpochDelta(
        fml::TimeDelta::FromNanoseconds(frame_time_nanos));
    const auto target =
        start + fml::TimeDelta::FromMicroseconds(
                    interval_micros_.load(std::memory_order_relaxed));
    FireCallback(start, target, /*pause_secondary_tasks=*/true);
  }

  /// 60 Hz until Java says otherwise, which is the rate a device that cannot
  /// be asked almost certainly has.
  static std::atomic<int64_t> interval_micros_;
};

std::atomic<int64_t> ChoreographerVsyncWaiter::interval_micros_{16667};

//------------------------------------------------------------------------------
// Text input.
//
// The editing model is the engine's own `flutter::TextInputModel`, exactly as
// the Windows host uses it. What differs is who edits it: on Windows the IME
// reports composition through window messages, and on Android the IME edits
// through an `InputConnection`, whose calls arrive here as the four `On*`
// methods below.
//
// The model stays the authority, and Java is told what it now holds after every
// change -- see JavaBridge::SetEditingState. Letting Java's `Editable` be the
// authority instead is upstream's arrangement, and it needs the whole
// `InputConnectionAdaptor`; with one field and no delta model this direction is
// both smaller and easier to keep true.
class TextInputHandler {
 public:
  using Sender = std::function<void(const std::string& method,
                                    const std::string& arguments_json)>;

  void SetSender(Sender sender) { sender_ = std::move(sender); }

  bool attached() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return model_ != nullptr;
  }

  /// Handles one call on `flutter/textinput`. Platform thread.
  std::optional<std::string> HandleMethodCall(const std::string& method,
                                              const rapidjson::Value* args);

  // -- What the input connection reports ---------------------------------------

  /// Text the IME committed.
  void OnText(const std::u16string& text) {
    if (Edit([&text](TextInputModel& model) {
          if (model.composing()) {
            // A commit ends the composition it was refining. Without this the
            // composing range would stay open over text that is now final.
            model.UpdateComposingText(text);
            model.CommitComposing();
            model.EndComposing();
          } else {
            model.AddText(text);
          }
          return true;
        })) {
      SendStateUpdate();
    }
  }

  /// Text the IME is still deciding about.
  void OnComposing(const std::u16string& text, int cursor) {
    if (Edit([&](TextInputModel& model) {
          if (!model.composing()) {
            model.BeginComposing();
          }
          model.UpdateComposingText(
              text, TextRange(static_cast<size_t>(cursor < 0 ? 0 : cursor)));
          return true;
        })) {
      SendStateUpdate();
    }
  }

  /// The IME gave up on the composition without committing it.
  void OnComposingEnd() {
    if (Edit([](TextInputModel& model) {
          if (!model.composing()) {
            return false;
          }
          model.CommitComposing();
          model.EndComposing();
          return true;
        })) {
      SendStateUpdate();
    }
  }

  /// Backspace, delete, and the arrows, home and end. True if the field used
  /// it -- the caller passes an unused key on to the framework as a key event.
  bool OnEditingKey(int32_t key_code, bool shift);

  /// Enter, which submits rather than edits.
  void OnAction();

  /// Drops the client without telling the framework. Used when the Activity
  /// goes away: there is nothing left to report an editing state to.
  void Detach() {
    std::lock_guard<std::mutex> lock(mutex_);
    model_.reset();
  }

 private:
  bool Edit(const std::function<bool(TextInputModel&)>& edit) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (model_ == nullptr) {
      return false;
    }
    return edit(*model_);
  }

  void SendStateUpdate();

  mutable std::mutex mutex_;
  std::unique_ptr<TextInputModel> model_;
  int client_id_ = 0;
  std::string input_action_;
  std::string input_type_;
  Sender sender_;
};

/// Must match RustflutterActivity's KEY_* constants, which are Android's own
/// `KeyEvent.KEYCODE_*` values for the keys an input connection reports.
constexpr int32_t kKeyCodeDel = 67;
constexpr int32_t kKeyCodeForwardDel = 112;
constexpr int32_t kKeyCodeDpadLeft = 21;
constexpr int32_t kKeyCodeDpadRight = 22;
constexpr int32_t kKeyCodeMoveHome = 122;
constexpr int32_t kKeyCodeMoveEnd = 123;

bool TextInputHandler::OnEditingKey(int32_t key_code, bool shift) {
  bool changed = false;
  const bool handled = Edit([&](TextInputModel& model) {
    switch (key_code) {
      case kKeyCodeDel:
        changed = model.Backspace();
        return true;
      case kKeyCodeForwardDel:
        changed = model.Delete();
        return true;
      case kKeyCodeDpadLeft:
        // The model has no one-character extend, only SelectToBeginning and
        // SelectToEnd, so shift-arrow moves like the unshifted arrow rather
        // than pretending to select. The Windows host is in the same position.
        changed = model.MoveCursorBack();
        return true;
      case kKeyCodeDpadRight:
        changed = model.MoveCursorForward();
        return true;
      case kKeyCodeMoveHome:
        changed = shift ? model.SelectToBeginning() : model.MoveCursorToBeginning();
        return true;
      case kKeyCodeMoveEnd:
        changed = shift ? model.SelectToEnd() : model.MoveCursorToEnd();
        return true;
      default:
        return false;
    }
  });
  if (changed) {
    SendStateUpdate();
  }
  return handled;
}

std::optional<std::string> TextInputHandler::HandleMethodCall(
    const std::string& method,
    const rapidjson::Value* args) {
  if (method == "TextInput.show") {
    JavaBridge::Request(HostRequest::kShowKeyboard, "");
    return NullEnvelope();
  }
  if (method == "TextInput.hide") {
    JavaBridge::Request(HostRequest::kHideKeyboard, "");
    return NullEnvelope();
  }

  if (method == "TextInput.setClient") {
    if (args == nullptr || !args->IsArray() || args->Size() < 2) {
      return ErrorEnvelope("TextInput.badArgument",
                           "setClient needs a client id and a configuration");
    }
    const rapidjson::Value& client = (*args)[0];
    const rapidjson::Value& config = (*args)[1];
    if (!client.IsInt()) {
      return ErrorEnvelope("TextInput.badArgument",
                           "the client id is not a number");
    }
    {
      std::lock_guard<std::mutex> lock(mutex_);
      client_id_ = client.GetInt();
      input_action_.clear();
      input_type_.clear();
      if (config.IsObject()) {
        auto action = config.FindMember("inputAction");
        if (action != config.MemberEnd() && action->value.IsString()) {
          input_action_ = action->value.GetString();
        }
        auto type = config.FindMember("inputType");
        if (type != config.MemberEnd() && type->value.IsObject()) {
          auto name = type->value.FindMember("name");
          if (name != type->value.MemberEnd() && name->value.IsString()) {
            input_type_ = name->value.GetString();
          }
        }
      }
      model_ = std::make_unique<TextInputModel>();
    }
    // A new field means a new editable as far as the IME is concerned: its
    // cached contents belong to the field that just lost focus. "1" says a
    // field is focused, which is what decides whether the view is a text
    // editor at all -- see RustflutterActivity.sHasClient.
    JavaBridge::SetEditingState("", 0, 0, -1, -1);
    JavaBridge::Request(HostRequest::kRestartInput, "1");
    return NullEnvelope();
  }

  if (method == "TextInput.clearClient") {
    bool was_composing = false;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (model_ != nullptr && model_->composing()) {
        model_->CommitComposing();
        model_->EndComposing();
        was_composing = true;
      }
    }
    if (was_composing) {
      SendStateUpdate();
    }
    {
      std::lock_guard<std::mutex> lock(mutex_);
      model_.reset();
    }
    JavaBridge::Request(HostRequest::kRestartInput, "0");
    return NullEnvelope();
  }

  if (method == "TextInput.setEditingState") {
    if (args == nullptr || !args->IsObject()) {
      return ErrorEnvelope("TextInput.badArgument",
                           "setEditingState needs a state");
    }
    auto text = args->FindMember("text");
    if (text == args->MemberEnd() || !text->value.IsString()) {
      return ErrorEnvelope("TextInput.badArgument", "the state has no text");
    }
    auto number = [args](const char* key, int fallback) {
      auto found = args->FindMember(key);
      return found != args->MemberEnd() && found->value.IsInt()
                 ? found->value.GetInt()
                 : fallback;
    };
    const int base = number("selectionBase", -1);
    const int extent = number("selectionExtent", -1);
    const int composing_base = number("composingBase", -1);
    const int composing_extent = number("composingExtent", -1);

    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (model_ == nullptr) {
        return ErrorEnvelope(
            "TextInput.noClient",
            "the editing state was set with no client attached");
      }
      model_->SetText(text->value.GetString(),
                      TextRange(static_cast<size_t>(base < 0 ? 0 : base),
                                static_cast<size_t>(extent < 0 ? 0 : extent)),
                      composing_base < 0 || composing_extent < 0
                          ? TextRange(0, 0)
                          : TextRange(static_cast<size_t>(composing_base),
                                      static_cast<size_t>(composing_extent)));
    }
    // The framework is the authority; this is it telling the platform what the
    // field now holds, and the IME is part of the platform.
    JavaBridge::SetEditingState(text->value.GetString(), base < 0 ? 0 : base,
                                extent < 0 ? 0 : extent, composing_base,
                                composing_extent);
    return NullEnvelope();
  }

  if (method == "TextInput.setMarkedTextRect" ||
      method == "TextInput.setEditableSizeAndTransform" ||
      method == "TextInput.setStyle") {
    // Where the editable sits on screen. Android places its candidate window
    // from the input connection rather than from coordinates the application
    // supplies, so there is nothing to do with these -- but they are answered
    // rather than refused, because an unanswered call is an error the
    // application would see for something that is working correctly.
    return NullEnvelope();
  }

  return std::nullopt;
}

void TextInputHandler::OnAction() {
  int client_id = 0;
  std::string action;
  bool newline = false;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    if (model_ == nullptr) {
      return;
    }
    client_id = client_id_;
    action = input_action_.empty() ? "TextInputAction.done" : input_action_;
    newline = input_type_ == "TextInputType.multiline" &&
              action == "TextInputAction.newline";
    if (newline) {
      model_->AddText(std::u16string(u"\n"));
    }
  }
  if (newline) {
    SendStateUpdate();
  }

  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartArray();
  writer.Int(client_id);
  writer.String(action.c_str());
  writer.EndArray();
  if (sender_) {
    sender_("TextInputClient.performAction",
            std::string(buffer.GetString(), buffer.GetSize()));
  }
}

void TextInputHandler::SendStateUpdate() {
  int client_id = 0;
  std::string text;
  int selection_base = 0;
  int selection_extent = 0;
  int composing_base = -1;
  int composing_extent = -1;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    if (model_ == nullptr) {
      return;
    }
    client_id = client_id_;
    text = model_->GetText();
    selection_base = static_cast<int>(model_->selection().base());
    selection_extent = static_cast<int>(model_->selection().extent());
    if (model_->composing()) {
      composing_base = static_cast<int>(model_->composing_range().base());
      composing_extent = static_cast<int>(model_->composing_range().extent());
    }
  }

  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartArray();
  writer.Int(client_id);
  writer.StartObject();
  writer.Key("selectionAffinity");
  writer.String("TextAffinity.downstream");
  writer.Key("selectionBase");
  writer.Int(selection_base);
  writer.Key("selectionExtent");
  writer.Int(selection_extent);
  writer.Key("selectionIsDirectional");
  writer.Bool(false);
  writer.Key("composingBase");
  writer.Int(composing_base);
  writer.Key("composingExtent");
  writer.Int(composing_extent);
  writer.Key("text");
  writer.String(text.c_str(), static_cast<rapidjson::SizeType>(text.size()));
  writer.EndObject();
  writer.EndArray();

  if (sender_) {
    sender_("TextInputClient.updateEditingState",
            std::string(buffer.GetString(), buffer.GetSize()));
  }
}

//------------------------------------------------------------------------------
// Application exit.
//
// The same exchange the Windows host implements, and for the same reason: an
// application with unsaved work needs a say between the reader asking to leave
// and the Activity finishing. On Android the ask arrives as the back gesture
// rather than a close button, and "close" means `Activity.finish()`.

class ExitRequester {
 public:
  virtual ~ExitRequester() = default;
  virtual void RequestAppExit(bool cancelable, int32_t exit_code) = 0;
  virtual void QuitApplication(int32_t exit_code) = 0;
};

std::optional<std::string> HandlePlatformCall(ExitRequester* requester,
                                              const std::string& method,
                                              const rapidjson::Value* args) {
  if (method == "System.exitApplication") {
    if (args == nullptr || !args->IsObject()) {
      return ErrorEnvelope(kExitRequestError, kInvalidExitRequestMessage);
    }
    auto type = args->FindMember("type");
    if (type == args->MemberEnd() || !type->value.IsString()) {
      return ErrorEnvelope(kExitRequestError, kInvalidExitRequestMessage);
    }
    auto code = args->FindMember("exitCode");
    if (code == args->MemberEnd() || !code->value.IsInt()) {
      return ErrorEnvelope(kExitRequestError, kInvalidExitRequestMessage);
    }
    const auto exit_code = code->value.GetInt();
    const bool cancelable =
        std::string(type->value.GetString()) == kExitTypeCancelable;

    if (!cancelable) {
      requester->QuitApplication(exit_code);
      return SuccessEnvelope([](auto& writer) {
        writer.StartObject();
        writer.Key("response");
        writer.String("exit");
        writer.EndObject();
      });
    }
    requester->RequestAppExit(/*cancelable=*/true, exit_code);
    return SuccessEnvelope([](auto& writer) {
      writer.StartObject();
      writer.Key("response");
      writer.String("cancel");
      writer.EndObject();
    });
  }

  if (method == "SystemNavigator.pop") {
    // Upstream's Android `SystemNavigator.pop` finishes the Activity, and the
    // reader sees the application leave the foreground rather than the process
    // die. Zero, because a pop is an ordinary way to leave.
    requester->QuitApplication(0);
    return NullEnvelope();
  }

  if (method == "Clipboard.getData") {
    if (args == nullptr || !args->IsString() ||
        std::string(args->GetString()) != kTextPlainFormat) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    auto text = JavaBridge::Request(HostRequest::kClipboardGet, "");
    if (!text.has_value()) {
      return NullEnvelope();
    }
    return SuccessEnvelope([&text](auto& writer) {
      writer.StartObject();
      writer.Key("text");
      writer.String(text->c_str(),
                    static_cast<rapidjson::SizeType>(text->size()));
      writer.EndObject();
    });
  }

  if (method == "Clipboard.setData") {
    if (args == nullptr || !args->IsObject()) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    auto text = args->FindMember("text");
    if (text == args->MemberEnd() || !text->value.IsString()) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    JavaBridge::Request(HostRequest::kClipboardSet, text->value.GetString());
    return NullEnvelope();
  }

  if (method == "Clipboard.hasStrings") {
    if (args == nullptr || !args->IsString() ||
        std::string(args->GetString()) != kTextPlainFormat) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    auto answer = JavaBridge::Request(HostRequest::kClipboardHasStrings, "");
    const bool has_text = answer.has_value() && *answer == "1";
    return SuccessEnvelope([has_text](auto& writer) {
      writer.StartObject();
      writer.Key("value");
      writer.Bool(has_text);
      writer.EndObject();
    });
  }

  if (method == "SystemChrome.setApplicationSwitcherDescription") {
    // The label the recents screen shows, which is what this method is for on
    // Android -- `ActivityManager.TaskDescription`. The colour has nowhere to
    // go here, as it has nowhere to go on Windows.
    if (args != nullptr && args->IsObject()) {
      auto label = args->FindMember("label");
      if (label != args->MemberEnd() && label->value.IsString()) {
        JavaBridge::Request(HostRequest::kSetTaskLabel,
                            label->value.GetString());
      }
    }
    return NullEnvelope();
  }

  if (method == "SystemSound.play" || method == "HapticFeedback.vibrate" ||
      method == "SystemChrome.setSystemUIOverlayStyle" ||
      method == "SystemChrome.setEnabledSystemUIMode" ||
      method == "SystemChrome.setPreferredOrientations" ||
      method == "System.initializationComplete") {
    // Answered rather than refused. None of these has anything to do here --
    // the sounds and the vibration are not wired up, and the chrome calls are
    // about a system bar this host does not manage -- but an application that
    // calls them is not doing anything wrong, and an error is what it would see
    // if they went unanswered.
    return NullEnvelope();
  }

  return std::nullopt;
}

//------------------------------------------------------------------------------
/// A reply to a message this host sent the framework.
///
/// Identical in shape to the Windows host's: completed on the UI thread, so the
/// callback is posted to the platform thread rather than run where it lands.
class HostPlatformMessageResponse : public PlatformMessageResponse {
 public:
  using Callback = std::function<void(const uint8_t*, size_t)>;

  void Complete(std::unique_ptr<fml::Mapping> data) override {
    if (data == nullptr) {
      CompleteEmpty();
      return;
    }
    Post(std::vector<uint8_t>(data->GetMapping(),
                              data->GetMapping() + data->GetSize()));
  }

  void CompleteEmpty() override { Post({}); }

 private:
  HostPlatformMessageResponse(fml::RefPtr<fml::TaskRunner> task_runner,
                              Callback callback)
      : task_runner_(std::move(task_runner)), callback_(std::move(callback)) {}

  void Post(std::vector<uint8_t> reply) {
    if (is_complete_) {
      FML_LOG(ERROR) << "Platform message response completed more than once.";
      return;
    }
    is_complete_ = true;
    if (!task_runner_) {
      return;
    }
    task_runner_->PostTask(fml::MakeCopyable(
        [callback = callback_, reply = std::move(reply)]() mutable {
          callback(reply.empty() ? nullptr : reply.data(), reply.size());
        }));
  }

  fml::RefPtr<fml::TaskRunner> task_runner_;
  Callback callback_;

  FML_FRIEND_MAKE_REF_COUNTED(HostPlatformMessageResponse);
  FML_FRIEND_REF_COUNTED_THREAD_SAFE(HostPlatformMessageResponse);
  FML_DISALLOW_COPY_AND_ASSIGN(HostPlatformMessageResponse);
};

//------------------------------------------------------------------------------
/// The shell's view of the Surface.
class HostPlatformView final : public PlatformView,
                               public GPUSurfaceSoftwareDelegate,
                               public ExitRequester {
 public:
  HostPlatformView(PlatformView::Delegate& delegate,
                   const TaskRunners& task_runners,
                   ANativeWindow* window,
                   TextInputHandler* text_input,
                   std::atomic<bool>* exit_processing,
                   bool prefer_impeller)
      : PlatformView(delegate, task_runners),
        window_(window),
        text_input_(text_input),
        exit_processing_(exit_processing),
        prefer_impeller_(prefer_impeller) {}

  ~HostPlatformView() override = default;

  // |PlatformView|
  void SetupImpellerContext() override {
    if (prefer_impeller_ && !gl_context_) {
      gl_context_ = ImpellerGlContext::Create();
      if (!gl_context_) {
        FML_LOG(IMPORTANT)
            << "Falling back to software rendering; see the error above.";
      }
    }
    rf_set_impeller_backend(gl_context_ != nullptr ? 1 : 0);
  }

  // |PlatformView|
  std::unique_ptr<Surface> CreateRenderingSurface() override {
    if (gl_context_) {
      if (auto surface = CreateImpellerSurface()) {
        return surface;
      }
      FML_LOG(IMPORTANT)
          << "Falling back to software rendering; see the error above.";
    }
    return std::make_unique<GPUSurfaceSoftware>(this,
                                                /*render_to_surface=*/true);
  }

  // |PlatformView|
  std::shared_ptr<impeller::Context> GetImpellerContext() const override {
    return gl_context_ ? gl_context_->GetImpellerContext() : nullptr;
  }

  // |PlatformView|
  //
  // Deliberately does not offer the IO thread as an upload target, which is the
  // one place this host does less than the Windows one.
  //
  // Uploading a texture on one thread and drawing it on another needs the two
  // GL contexts to be synchronised, not merely to share a group: the writer has
  // to flush before the reader may use what it wrote. Desktop drivers are
  // forgiving about this and ANGLE hides it entirely; the drivers here are not,
  // and the result was an album whose thumbnails came out as whatever had been
  // in that texture memory before -- fragments of the launcher, upside down.
  //
  // So uploads happen on the raster thread, where the drawing is. The cost is a
  // hitch the first time an image is drawn rather than a smooth first frame,
  // and it is paid only by applications that decode images themselves.
  sk_sp<GrDirectContext> CreateResourceContext() const override {
    return nullptr;
  }

  // |PlatformView|
  std::unique_ptr<VsyncWaiter> CreateVSyncWaiter() override {
    return std::make_unique<ChoreographerVsyncWaiter>(task_runners_);
  }

  // |GPUSurfaceSoftwareDelegate|
  sk_sp<SkSurface> AcquireBackingStore(const DlISize& size) override {
    if (size.width <= 0 || size.height <= 0) {
      return nullptr;
    }
    if (backing_store_ != nullptr && backing_store_->width() == size.width &&
        backing_store_->height() == size.height) {
      return backing_store_;
    }
    SkImageInfo info = SkImageInfo::MakeN32Premul(size.width, size.height);
    backing_store_ = SkSurfaces::Raster(info);
    return backing_store_;
  }

  // |GPUSurfaceSoftwareDelegate|
  //
  // The software path copies the frame into the Surface's own buffer. It exists
  // as a fallback for a device whose GLES will not come up, and is slower by
  // the cost of that copy.
  bool PresentBackingStore(sk_sp<SkSurface> backing_store) override {
    if (backing_store == nullptr || window_ == nullptr) {
      return false;
    }
    SkPixmap pixmap;
    if (!backing_store->peekPixels(&pixmap)) {
      return false;
    }

    // Asked for once, and it has to be asked for: a SurfaceView's default
    // format is whatever the window manager chose, which on this emulator is
    // RGB_565 -- two bytes a pixel where Skia has written four. Copying rows
    // sized for one into a buffer sized for the other walks off the end of the
    // mapping, which is what it did.
    if (!geometry_set_) {
      geometry_set_ = true;
      ANativeWindow_setBuffersGeometry(window_, 0, 0, WINDOW_FORMAT_RGBX_8888);
    }

    ANativeWindow_Buffer buffer;
    if (ANativeWindow_lock(window_, &buffer, nullptr) != 0) {
      return false;
    }
    // Both are RGBA in memory -- Skia's N32 is RGBA on Android, and so is
    // RGBX_8888 -- so the copy is per row rather than per pixel. If the format
    // is anything else, presenting nothing is better than presenting rubbish.
    bool presented = false;
    if (buffer.format == WINDOW_FORMAT_RGBX_8888 ||
        buffer.format == WINDOW_FORMAT_RGBA_8888) {
      // The window and the frame can disagree for a frame after a rotation or
      // a resize, so both dimensions are the smaller of the two.
      const int rows = std::min(buffer.height, pixmap.height());
      const size_t row_bytes =
          static_cast<size_t>(std::min(buffer.width, pixmap.width())) * 4u;
      auto* destination = static_cast<uint8_t*>(buffer.bits);
      const auto* source = static_cast<const uint8_t*>(pixmap.addr());
      for (int row = 0; row < rows; ++row) {
        memcpy(destination + static_cast<size_t>(row) * buffer.stride * 4u,
               source + static_cast<size_t>(row) * pixmap.rowBytes(),
               row_bytes);
      }
      presented = true;
    }
    ANativeWindow_unlockAndPost(window_);
    return presented;
  }

  /// Sends one pointer event to the engine.
  void SendPointer(const PointerData& data) {
    auto packet = std::make_unique<PointerDataPacket>(1);
    packet->SetPointerData(0, data);
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), packet = std::move(packet)]() mutable {
          if (weak) {
            static_cast<HostPlatformView*>(weak.get())
                ->DispatchPointerDataPacket(std::move(packet));
          }
        }));
  }

  // |PlatformView|
  void HandlePlatformMessage(std::unique_ptr<PlatformMessage> message) override {
    const auto& data = message->data();
    std::optional<std::vector<uint8_t>> reply;

    const bool platform = message->channel() == kPlatformChannel;
    const bool editing = message->channel() == kTextInputChannel;
    if (!platform && !editing) {
      PlatformView::HandlePlatformMessage(std::move(message));
      return;
    }

    rapidjson::Document document;
    document.Parse(reinterpret_cast<const char*>(data.GetMapping()),
                   data.GetSize());
    if (!document.HasParseError() && document.IsObject()) {
      auto method = document.FindMember("method");
      if (method != document.MemberEnd() && method->value.IsString()) {
        auto found = document.FindMember("args");
        const rapidjson::Value* args =
            found == document.MemberEnd() ? nullptr : &found->value;
        std::optional<std::string> json =
            platform
                ? HandlePlatformCall(this, method->value.GetString(), args)
                : text_input_->HandleMethodCall(method->value.GetString(), args);
        if (json.has_value()) {
          reply.emplace(json->begin(), json->end());
        }
      }
    }

    auto response = message->response();
    if (!response) {
      return;
    }
    if (!reply.has_value()) {
      response->CompleteEmpty();
      return;
    }
    response->Complete(std::make_unique<fml::DataMapping>(std::move(*reply)));
  }

  // |PlatformView|
  void SendChannelUpdate(const std::string& name, bool listening) override {
    if (name == kPlatformChannel) {
      exit_processing_->store(listening, std::memory_order_relaxed);
    }
  }

  // |ExitRequester|
  void RequestAppExit(bool cancelable, int32_t exit_code) override {
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartObject();
    writer.Key("type");
    writer.String(cancelable ? kExitTypeCancelable : kExitTypeRequired);
    writer.EndObject();

    SendMethodCallWithReply(
        kPlatformChannel, "System.requestAppExit",
        std::string(buffer.GetString(), buffer.GetSize()),
        [this, exit_code](const uint8_t* reply, size_t length) {
          HandleExitResponse(reply, length, exit_code);
        });
  }

  // |ExitRequester|
  void QuitApplication(int32_t exit_code) override {
    JavaBridge::Request(HostRequest::kFinish, std::to_string(exit_code));
  }

  /// The back gesture: asks the framework to pop a route.
  ///
  /// Upstream's Android embedder sends `popRoute` and leaves it there, because
  /// upstream's `WidgetsBinding` always answers -- it pops if it can and calls
  /// `SystemNavigator.pop` if it cannot, so the Activity finishing is the
  /// framework's decision either way.
  ///
  /// Here an application need not have a navigator at all, and a back gesture
  /// that silently did nothing would strand the reader in the application. So
  /// the reply is read: an empty one means nobody was listening on the channel,
  /// and then, and only then, the Activity finishes. Anything else means the
  /// framework took it.
  void SendPopRoute() {
    SendMethodCallWithReply(kNavigationChannel, "popRoute", "null",
                            [this](const uint8_t* reply, size_t length) {
                              if (reply == nullptr || length == 0) {
                                QuitApplication(0);
                              }
                            });
  }

  /// Tells the framework the reader's settings and languages.
  ///
  /// Both come from Java as finished JSON: see RustflutterActivity.settings().
  /// Empty means Java had nothing to say, which is not the same as the defaults
  /// and must not overwrite what the framework already has.
  void SendPlatformSettings(const std::string& settings,
                            const std::string& locales) {
    if (!settings.empty()) {
      SendOnChannel(kSettingsChannel, settings);
    }
    if (!locales.empty()) {
      SendOnChannel(kLocalizationChannel, locales);
    }
  }

  void SendLifecycleState(const std::string& state) {
    SendOnChannel(kLifecycleChannel, state);
  }

  void SendMethodCall(const char* channel,
                      const std::string& method,
                      const std::string& arguments_json) {
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartObject();
    writer.Key("method");
    writer.String(method.c_str());
    writer.Key("args");
    writer.RawValue(arguments_json.c_str(), arguments_json.size(),
                    rapidjson::kArrayType);
    writer.EndObject();
    SendOnChannel(channel, std::string(buffer.GetString(), buffer.GetSize()));
  }

 private:
  void SendMethodCallWithReply(const char* channel,
                               const std::string& method,
                               const std::string& arguments_json,
                               HostPlatformMessageResponse::Callback callback) {
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartObject();
    writer.Key("method");
    writer.String(method.c_str());
    writer.Key("args");
    writer.RawValue(arguments_json.c_str(), arguments_json.size(),
                    rapidjson::kObjectType);
    writer.EndObject();
    std::string payload(buffer.GetString(), buffer.GetSize());

    auto response = fml::MakeRefCounted<HostPlatformMessageResponse>(
        task_runners_.GetPlatformTaskRunner(), std::move(callback));
    auto message = std::make_unique<PlatformMessage>(
        channel, fml::MallocMapping::Copy(payload.data(), payload.size()),
        std::move(response));
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
  }

  /// What the framework said when asked whether it may close. Only `"exit"`
  /// finishes the Activity; anything else leaves it up.
  void HandleExitResponse(const uint8_t* reply, size_t length, int32_t code) {
    if (reply == nullptr || length == 0) {
      QuitApplication(code);
      return;
    }
    rapidjson::Document envelope;
    envelope.Parse(reinterpret_cast<const char*>(reply), length);
    if (envelope.HasParseError() || !envelope.IsArray() ||
        envelope.Size() != 1 || !envelope[0].IsObject()) {
      FML_LOG(ERROR) << "Application exit request response did not contain a "
                        "valid response value.";
      return;
    }
    auto response = envelope[0].FindMember("response");
    if (response == envelope[0].MemberEnd() || !response->value.IsString()) {
      FML_LOG(ERROR) << "Application exit request response did not contain a "
                        "valid response value.";
      return;
    }
    if (std::string(response->value.GetString()) == "exit") {
      QuitApplication(code);
    }
  }

  void SendOnChannel(const std::string& channel, const std::string& payload) {
    auto message = std::make_unique<PlatformMessage>(
        channel, fml::MallocMapping::Copy(payload.data(), payload.size()),
        /*response=*/nullptr);
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
  }

  std::unique_ptr<Surface> CreateImpellerSurface() {
    gl_delegate_ = std::make_unique<ImpellerGlDelegate>(
        gl_context_.get(), static_cast<EGLNativeWindowType>(window_));
    if (!gl_delegate_->IsValid()) {
      gl_delegate_.reset();
      return nullptr;
    }
    auto made_current = gl_delegate_->GLContextMakeCurrent();
    if (!made_current || !made_current->GetResult()) {
      FML_LOG(ERROR) << "Could not make the GL context current on the raster "
                        "thread.";
      gl_delegate_.reset();
      return nullptr;
    }
    auto surface = std::make_unique<GPUSurfaceGLImpeller>(
        gl_delegate_.get(), gl_context_->GetImpellerContext(),
        /*render_to_surface=*/true);
    if (!surface->IsValid()) {
      FML_LOG(ERROR) << "The Impeller GL surface came up invalid.";
      gl_delegate_.reset();
      return nullptr;
    }
    return surface;
  }

  ANativeWindow* window_ = nullptr;
  /// Whether the window has been told what pixels it will be handed. Only the
  /// software path asks, and only once.
  bool geometry_set_ = false;
  TextInputHandler* text_input_ = nullptr;
  std::atomic<bool>* exit_processing_ = nullptr;
  bool prefer_impeller_ = false;
  std::unique_ptr<ImpellerGlContext> gl_context_;
  std::unique_ptr<ImpellerGlDelegate> gl_delegate_;
  sk_sp<SkSurface> backing_store_;

  FML_DISALLOW_COPY_AND_ASSIGN(HostPlatformView);
};

//------------------------------------------------------------------------------
/// Everything the JNI entry points reach.
///
/// A singleton because JNI entry points are free functions and an Activity is
/// the whole process here: one Surface, one shell, one framework. Upstream's
/// `AndroidShellHolder` is per-engine because an application may hold several;
/// nothing in this fork can.
struct HostState {
  static HostState& Get() {
    static HostState instance;
    return instance;
  }

  std::unique_ptr<ThreadHost> threads;
  std::unique_ptr<TaskRunners> task_runners;
  std::unique_ptr<Shell> shell;
  HostPlatformView* platform_view = nullptr;
  TextInputHandler text_input;

  /// Set by SendChannelUpdate once the framework is listening on
  /// `flutter/platform`, which is what makes a back press a question rather
  /// than an order.
  std::atomic<bool> exit_processing{false};

  ANativeWindow* window = nullptr;

  /// Where each pointer was last seen, so a move can say how far it moved.
  ///
  /// The framework measures a drag by accumulating `physical_delta`, not by
  /// subtracting positions -- see `GestureRouter::on_move` -- so a host that
  /// leaves the delta at zero has every swipe arbitrated as a tap. The Windows
  /// host keeps one last position because Windows has one mouse; a touch screen
  /// has as many pointers as fingers.
  std::map<int32_t, std::pair<double, double>> last_positions;

  int32_t width = 0;
  int32_t height = 0;
  double device_pixel_ratio = 1.0;
  std::string lifecycle_state;

  /// What Java last told us, kept so that the first frame has it. The
  /// application's first `build` chooses between the light and the dark theme,
  /// and it has to choose correctly rather than showing one frame of the wrong
  /// one.
  std::string settings_json;
  std::string locales_json;
  std::string icu_data_path;
};

void SendViewportMetrics() {
  HostState& state = HostState::Get();
  if (state.shell == nullptr || state.width <= 0 || state.height <= 0) {
    return;
  }
  ViewportMetrics metrics;
  metrics.device_pixel_ratio = state.device_pixel_ratio;
  metrics.physical_width = state.width;
  metrics.physical_height = state.height;
  metrics.physical_max_width_constraint = state.width;
  metrics.physical_max_height_constraint = state.height;

  state.task_runners->GetPlatformTaskRunner()->PostTask(
      [view = state.shell->GetPlatformView(), metrics]() {
        if (view) {
          view->SetViewportMetrics(kFlutterImplicitViewId, metrics);
        }
      });
}

void SendLifecycle(const char* next) {
  HostState& state = HostState::Get();
  if (state.platform_view == nullptr || state.lifecycle_state == next) {
    return;
  }
  state.lifecycle_state = next;
  state.platform_view->SendLifecycleState(next);
}

/// Tears the shell down. Called from the Activity's destruction and from
/// rf_host_run if a second start ever arrived.
void Shutdown() {
  HostState& state = HostState::Get();
  if (state.shell == nullptr) {
    return;
  }
  state.text_input.Detach();
  state.platform_view = nullptr;

  // The shell must be destroyed on the platform thread, which is this one: its
  // destructor drains the UI, raster and IO threads in order and would deadlock
  // if it were not the one holding the platform thread. That this is already
  // the platform thread is the one real simplification Android buys.
  if (auto view = state.shell->GetPlatformView()) {
    view->NotifyDestroyed();
  }
  state.shell.reset();
  state.task_runners.reset();
  state.threads.reset();
  state.lifecycle_state.clear();

  if (state.window != nullptr) {
    ANativeWindow_release(state.window);
    state.window = nullptr;
  }
}

}  // namespace
}  // namespace flutter

//------------------------------------------------------------------------------
// The entry point the Rust side calls.
//
// On Windows this opens a window and pumps messages until it closes. Here the
// Activity already exists, already has a Surface, and already owns the loop, so
// this only builds the shell and returns -- which is why the Rust `run()`
// returning is not the application ending on Android.
int32_t rf_host_run(const RfHostOptions* options) {
  using namespace flutter;  // NOLINT(build/namespaces)

  HostState& state = HostState::Get();
  if (state.window == nullptr) {
    FML_LOG(ERROR) << "rf_host_run was called before the Surface existed.";
    return -1;
  }
  if (state.shell != nullptr) {
    // A second start with the first still up. Nothing in the Activity does
    // this, but a restart that skipped the teardown would leak two shells onto
    // one Surface, and one of them would be drawing invisibly.
    Shutdown();
  }

  Settings settings;
  settings.enable_impeller = options == nullptr || options->enable_impeller != 0;
  settings.enable_software_rendering = !settings.enable_impeller;
  settings.icu_initialization_required = true;
  settings.icu_data_path = state.icu_data_path;
  settings.warn_on_impeller_opt_out = false;

  // The platform thread is this thread -- the Android main thread. Everything
  // else is an fml thread, exactly as on Windows.
  fml::MessageLoop::EnsureInitializedForCurrentThread();
  state.threads = std::make_unique<ThreadHost>(
      "rf", ThreadHost::Type::kUi | ThreadHost::Type::kRaster |
                ThreadHost::Type::kIo);
  // `GetCurrent` is marked FML_EMBEDDER_ONLY, which is exactly what this is:
  // taking a task runner for a loop somebody else owns is how an embedder
  // borrows its host's thread, and Android's main thread is the case the
  // annotation was written for.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
  state.task_runners = std::make_unique<TaskRunners>(
      "rustflutter", fml::MessageLoop::GetCurrent().GetTaskRunner(),
      state.threads->raster_thread->GetTaskRunner(),
      state.threads->ui_thread->GetTaskRunner(),
      state.threads->io_thread->GetTaskRunner());
#pragma clang diagnostic pop

  PlatformData platform_data;
  state.shell = Shell::Create(
      platform_data, *state.task_runners, settings,
      [&state, impeller = settings.enable_impeller](Shell& shell) {
        auto view = std::make_unique<HostPlatformView>(
            shell, shell.GetTaskRunners(), state.window, &state.text_input,
            &state.exit_processing, impeller);
        state.platform_view = view.get();
        state.text_input.SetSender(
            [sender = view.get()](const std::string& method,
                                  const std::string& arguments) {
              sender->SendMethodCall(kTextInputChannel, method, arguments);
            });
        return view;
      },
      [](Shell& shell) { return std::make_unique<Rasterizer>(shell); });

  if (state.shell == nullptr || !state.shell->IsSetup()) {
    FML_LOG(ERROR) << "The shell would not start.";
    state.shell.reset();
    return -4;
  }

  // Already on the platform thread, so this is a call rather than a post --
  // but the ordering is the same as the Windows host's, and for the same
  // reasons: a surface before the first frame is rasterised, a size before the
  // framework lays anything out, and the settings before the first build.
  state.shell->RunEngine(RunConfiguration{});
  if (auto view = state.shell->GetPlatformView()) {
    view->NotifyCreated();
  }
  std::vector<std::unique_ptr<Display>> displays;
  displays.push_back(std::make_unique<Display>(
      /*display_id=*/0, 1000000.0 / 16667.0, state.width, state.height,
      state.device_pixel_ratio));
  state.shell->OnDisplayUpdates(std::move(displays));
  SendViewportMetrics();
  state.platform_view->SendPlatformSettings(state.settings_json,
                                            state.locales_json);
  SendLifecycle("AppLifecycleState.resumed");
  return 0;
}

//------------------------------------------------------------------------------
// JNI.
//
// Every function below runs on the Android main thread, which is the platform
// thread. Names are mangled from `io.flutter.rustflutter.RustflutterActivity`,
// which is why every application in this fork shares that one class and varies
// only its application id.

extern "C" {

/// The application's own entry point, which is what a Rust example compiles to.
/// Declared rather than included because the host must not depend on any one
/// application.
int rustflutter_app_main(int argc, const char** argv);

JNIEXPORT jint JNI_OnLoad(JavaVM* vm, void* reserved) {
  fml::jni::InitJavaVM(vm);
  JNIEnv* env = fml::jni::AttachCurrentThread();
  jclass activity =
      env->FindClass("io/flutter/rustflutter/RustflutterActivity");
  FML_CHECK(activity != nullptr) << "RustflutterActivity was not found.";
  flutter::JavaBridge::Initialise(env, activity);
  env->DeleteLocalRef(activity);
  return JNI_VERSION_1_6;
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeSurfaceCreated(
    JNIEnv* env,
    jclass clazz,
    jobject surface,
    jint width,
    jint height,
    jfloat device_pixel_ratio,
    jfloat refresh_rate) {
  auto& state = flutter::HostState::Get();
  if (state.window != nullptr) {
    ANativeWindow_release(state.window);
  }
  state.window = ANativeWindow_fromSurface(env, surface);
  state.width = width;
  state.height = height;
  state.device_pixel_ratio = device_pixel_ratio > 0 ? device_pixel_ratio : 1.0;
  flutter::ChoreographerVsyncWaiter::SetRefreshRate(refresh_rate);
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeStart(
    JNIEnv* env,
    jclass clazz,
    jstring icu_data_path,
    jstring settings_json,
    jstring locales_json,
    jstring files_path,
    jstring external_files_path) {
  auto& state = flutter::HostState::Get();
  // Before the application runs, so that anything it prints on the way up is
  // readable.
  flutter::SendOutputToLogcat();
  state.icu_data_path = fml::jni::JavaStringToString(env, icu_data_path);
  state.settings_json = fml::jni::JavaStringToString(env, settings_json);
  state.locales_json = fml::jni::JavaStringToString(env, locales_json);

  // Into the environment, because that is where a program written against a
  // standard library looks for something the operating system decided. Both
  // paths depend on the package and the user, so nothing in the application
  // could work them out, and on every other platform the equivalent question --
  // where does this program keep its files -- has an answer std can give.
  setenv("RUSTFLUTTER_FILES_DIR",
         fml::jni::JavaStringToString(env, files_path).c_str(), 1);
  setenv("RUSTFLUTTER_EXTERNAL_FILES_DIR",
         fml::jni::JavaStringToString(env, external_files_path).c_str(), 1);
  // Everything after this is the application's: it registers itself and calls
  // `run`, which lands in rf_host_run above and comes straight back.
  rustflutter_app_main(0, nullptr);
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeSurfaceChanged(
    JNIEnv* env,
    jclass clazz,
    jint width,
    jint height,
    jfloat device_pixel_ratio) {
  auto& state = flutter::HostState::Get();
  state.width = width;
  state.height = height;
  state.device_pixel_ratio = device_pixel_ratio > 0 ? device_pixel_ratio : 1.0;
  flutter::SendViewportMetrics();
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeStop(JNIEnv* env,
                                                           jclass clazz) {
  flutter::Shutdown();
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeLifecycle(
    JNIEnv* env,
    jclass clazz,
    jstring state_name) {
  flutter::SendLifecycle(
      fml::jni::JavaStringToString(env, state_name).c_str());
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeSettingsChanged(
    JNIEnv* env,
    jclass clazz,
    jstring settings_json,
    jstring locales_json) {
  auto& state = flutter::HostState::Get();
  state.settings_json = fml::jni::JavaStringToString(env, settings_json);
  state.locales_json = fml::jni::JavaStringToString(env, locales_json);
  if (state.platform_view != nullptr) {
    state.platform_view->SendPlatformSettings(state.settings_json,
                                              state.locales_json);
  }
}

/// One touch point, already in physical pixels.
///
/// `phase` is PointerData::Change as an int, which is what the Java side maps
/// Android's MotionEvent actions onto -- the mapping belongs there because that
/// is where the pointer index bookkeeping is.
JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativePointer(
    JNIEnv* env,
    jclass clazz,
    jint pointer_id,
    jint phase,
    jfloat x,
    jfloat y,
    jlong timestamp_micros,
    jfloat pressure) {
  auto& state = flutter::HostState::Get();
  if (state.platform_view == nullptr) {
    return;
  }
  flutter::PointerData data;
  data.Clear();
  data.time_stamp = timestamp_micros;
  data.change = static_cast<flutter::PointerData::Change>(phase);
  data.kind = flutter::PointerData::DeviceKind::kTouch;
  data.signal_kind = flutter::PointerData::SignalKind::kNone;
  data.device = pointer_id;
  data.pointer_identifier = 0;
  data.physical_x = x;
  data.physical_y = y;

  // How far this pointer moved since it was last seen. A pointer that has just
  // arrived has moved nothing, and one that is leaving takes its entry with it.
  auto previous = state.last_positions.find(pointer_id);
  if (data.change == flutter::PointerData::Change::kMove &&
      previous != state.last_positions.end()) {
    data.physical_delta_x = x - previous->second.first;
    data.physical_delta_y = y - previous->second.second;
  }
  if (data.change == flutter::PointerData::Change::kRemove ||
      data.change == flutter::PointerData::Change::kCancel) {
    state.last_positions.erase(pointer_id);
  } else {
    state.last_positions[pointer_id] = {x, y};
  }

  const bool down =
      data.change == flutter::PointerData::Change::kDown ||
      data.change == flutter::PointerData::Change::kMove;
  data.buttons = down ? flutter::kPointerButtonTouchContact : 0;
  data.pressure = down ? (pressure > 0 ? pressure : 1.0) : 0.0;
  data.pressure_max = 1.0;
  data.view_id = flutter::kFlutterImplicitViewId;
  state.platform_view->SendPointer(data);
}

/// Text the IME committed.
JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeText(JNIEnv* env,
                                                           jclass clazz,
                                                           jstring text) {
  const std::string utf8 = fml::jni::JavaStringToString(env, text);
  // The model counts in UTF-16 code units, which is also what Java hands out,
  // so the conversion is only about the type.
  std::u16string utf16;
  for (size_t index = 0; index < utf8.size();) {
    unsigned char lead = utf8[index];
    char32_t code_point = 0;
    size_t length = 1;
    if (lead < 0x80) {
      code_point = lead;
    } else if ((lead & 0xE0) == 0xC0) {
      code_point = lead & 0x1F;
      length = 2;
    } else if ((lead & 0xF0) == 0xE0) {
      code_point = lead & 0x0F;
      length = 3;
    } else {
      code_point = lead & 0x07;
      length = 4;
    }
    if (index + length > utf8.size()) {
      break;
    }
    for (size_t extra = 1; extra < length; ++extra) {
      code_point = (code_point << 6) | (utf8[index + extra] & 0x3F);
    }
    index += length;
    if (code_point >= 0x10000) {
      code_point -= 0x10000;
      utf16.push_back(static_cast<char16_t>(0xD800 + (code_point >> 10)));
      utf16.push_back(static_cast<char16_t>(0xDC00 + (code_point & 0x3FF)));
    } else {
      utf16.push_back(static_cast<char16_t>(code_point));
    }
  }
  flutter::HostState::Get().text_input.OnText(utf16);
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeComposing(
    JNIEnv* env,
    jclass clazz,
    jstring text,
    jint cursor) {
  const std::string utf8 = fml::jni::JavaStringToString(env, text);
  std::u16string utf16(utf8.begin(), utf8.end());
  flutter::HostState::Get().text_input.OnComposing(utf16, cursor);
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeComposingEnd(
    JNIEnv* env,
    jclass clazz) {
  flutter::HostState::Get().text_input.OnComposingEnd();
}

JNIEXPORT jboolean JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeEditingKey(
    JNIEnv* env,
    jclass clazz,
    jint key_code,
    jboolean shift) {
  return flutter::HostState::Get().text_input.OnEditingKey(key_code, shift)
             ? JNI_TRUE
             : JNI_FALSE;
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeEditorAction(
    JNIEnv* env,
    jclass clazz) {
  flutter::HostState::Get().text_input.OnAction();
}

/// The back gesture.
///
/// Returns true if the framework was asked and false if the Activity should
/// just finish. The difference is whether anything over there is listening: a
/// back press that silently did nothing would be worse than one that leaves.
JNIEXPORT jboolean JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeBackPressed(JNIEnv* env,
                                                                  jclass clazz) {
  auto& state = flutter::HostState::Get();
  if (state.platform_view == nullptr) {
    return JNI_FALSE;
  }
  state.platform_view->SendPopRoute();
  return JNI_TRUE;
}

}  // extern "C"
