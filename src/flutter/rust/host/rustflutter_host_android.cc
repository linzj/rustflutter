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
#include "flutter/lib/ui/window/key_data.h"
#include "flutter/lib/ui/window/key_data_packet.h"
#include "flutter/lib/ui/window/platform_message.h"
#include "flutter/lib/ui/window/pointer_data.h"
#include "flutter/lib/ui/window/pointer_data_packet.h"
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"
#include "flutter/rust/host/rustflutter_gl.h"
#include "flutter/rust/host/rustflutter_key_map_android.h"
#include "flutter/rust/host/rustflutter_key_sync_android.h"
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
constexpr char kKeyDataChannel[] = "flutter/keydata";

constexpr char kClipboardError[] = "Clipboard error";
constexpr char kUnknownClipboardFormatMessage[] = "Unknown clipboard format";
constexpr char kTextPlainFormat[] = "text/plain";

constexpr char kExitRequestError[] = "ExitApplication error";
constexpr char kInvalidExitRequestMessage[] =
    "Invalid application exit request";
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
              __android_log_write(ANDROID_LOG_INFO, "rustflutter",
                                  line.c_str());
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
    request_ = env->GetStaticMethodID(
        class_, "onHostRequest", "(ILjava/lang/String;)Ljava/lang/String;");
    editing_ = env->GetStaticMethodID(class_, "onEditingState",
                                      "(Ljava/lang/String;IIII)V");
    semantics_ = env->GetStaticMethodID(class_, "onSemanticsUpdate",
                                        "(Ljava/lang/String;)V");
    key_result_ = env->GetStaticMethodID(class_, "onKeyResult", "(IZ)V");
    FML_CHECK(request_ != nullptr && editing_ != nullptr &&
              semantics_ != nullptr && key_result_ != nullptr)
        << "RustflutterActivity is missing a method the host calls.";
  }

  /// What the framework decided about one key.
  ///
  /// `handled` false means nothing in the application wanted it, and Java's
  /// job is then to give it back to Android -- otherwise an application would
  /// swallow the volume keys.
  static void KeyResult(int32_t sequence_id, bool handled) {
    if (class_ == nullptr) {
      return;
    }
    JNIEnv* env = fml::jni::AttachCurrentThread();
    env->CallStaticVoidMethod(class_, key_result_,
                              static_cast<jint>(sequence_id),
                              handled ? JNI_TRUE : JNI_FALSE);
    fml::jni::CheckException(env);
  }

  /// Hands the semantics tree to Java, as JSON.
  ///
  /// JSON rather than an array of structs across JNI, because the tree is a
  /// tree: every node carries a child list of its own, and marshalling that
  /// by hand is more code than parsing it with the `org.json` that is already
  /// on every Android device. It runs only while a screen reader is on.
  static void SemanticsUpdate(const std::string& json) {
    if (class_ == nullptr) {
      return;
    }
    JNIEnv* env = fml::jni::AttachCurrentThread();
    fml::jni::ScopedJavaLocalRef<jstring> java_json =
        fml::jni::StringToJavaString(env, json);
    env->CallStaticVoidMethod(class_, semantics_, java_json.obj());
    fml::jni::CheckException(env);
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
  static jmethodID semantics_;
  static jmethodID key_result_;
};

jclass JavaBridge::class_ = nullptr;
jmethodID JavaBridge::request_ = nullptr;
jmethodID JavaBridge::editing_ = nullptr;
jmethodID JavaBridge::semantics_ = nullptr;
jmethodID JavaBridge::key_result_ = nullptr;

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

  // -- What the input connection reports
  // ---------------------------------------

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

  /// The IME re-opening a composition over text that is already committed.
  ///
  /// Upstream's `InputConnectionAdaptor.setComposingRegion`, and **this is how
  /// a suggestion replaces a word**: the keyboard puts the region back around
  /// "wor" and then sets its text to "work". Without it the region stays where
  /// the caret is, an empty range, and the suggestion is inserted beside the
  /// word instead of over it.
  void OnComposingRegion(int start, int end) {
    if (Edit([&](TextInputModel& model) {
          if (start < 0 || end < 0 || end < start) {
            return false;
          }
          // **A composition has to be open first.** `SetComposingRange`
          // refuses outright when `composing_` is false, and re-opening a
          // region over text that is already committed -- which is the whole
          // point of this call -- is exactly the case where it is.
          if (!model.composing()) {
            model.BeginComposing();
          }
          // `SetComposingRange` moves the caret to `range.start() +
          // cursor_offset`, and upstream's `setComposingRegion` moves it
          // nowhere: `BaseInputConnection` sets spans and leaves the selection
          // alone. There is no way to ask this model for that, so the offset
          // is chosen to put the caret back where it already is -- which is
          // the same thing whenever the caret is inside the region, and it is
          // in every case a keyboard uses this for.
          const size_t begin = static_cast<size_t>(start);
          const size_t finish = static_cast<size_t>(end);
          const size_t caret = model.selection().extent();
          const size_t offset = caret >= begin && caret <= finish
                                    ? caret - begin
                                    : finish - begin;
          return model.SetComposingRange(TextRange(begin, finish), offset);
        })) {
      SendStateUpdate();
    }
  }

  /// The IME moving the caret -- upstream's
  /// `InputConnectionAdaptor.setSelection`.
  void OnSelection(int start, int end) {
    if (Edit([&](TextInputModel& model) {
          if (start < 0 || end < 0) {
            return false;
          }
          return model.SetSelection(
              TextRange(static_cast<size_t>(start), static_cast<size_t>(end)));
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

  /// The configuration, as the one line `RustflutterActivity` unpacks to build
  /// an `EditorInfo`.
  ///
  /// Ten fields separated by newlines, the first being "a field is focused".
  /// The *names* cross rather than Android's numbers: every constant
  /// (`TYPE_TEXT_FLAG_AUTO_CORRECT` and the rest) stays on the Java side where
  /// it can be spelled rather than copied, which is where upstream's
  /// `TextInputPlugin.inputTypeFromTextInputType` keeps them too.
  std::string Descriptor() const {
    std::lock_guard<std::mutex> lock(mutex_);
    std::string out = "1\n";
    out += input_type_ + "\n";
    out += input_action_ + "\n";
    out += (obscure_text_ ? "1\n" : "0\n");
    out += (autocorrect_ ? "1\n" : "0\n");
    out += (enable_suggestions_ ? "1\n" : "0\n");
    out += (personalized_learning_ ? "1\n" : "0\n");
    out += (number_signed_ ? "1\n" : "0\n");
    out += (number_decimal_ ? "1\n" : "0\n");
    out += capitalization_;
    return out;
  }

  mutable std::mutex mutex_;
  std::unique_ptr<TextInputModel> model_;
  int client_id_ = 0;
  std::string input_action_;
  std::string input_type_;
  // The rest of what upstream's `TextInputPlugin` turns into an `EditorInfo`.
  // Read from the configuration and handed to Java, which owns the mapping to
  // Android's own constants -- see
  // `RustflutterActivity.onCreateInputConnection`.
  bool obscure_text_ = false;
  bool autocorrect_ = true;
  bool enable_suggestions_ = true;
  bool personalized_learning_ = true;
  /// `TextInputType.number`'s two options, which upstream reads as
  /// `type.isSigned` and `type.isDecimal`. False for every other type, which
  /// is what the wire sends for them.
  bool number_signed_ = false;
  bool number_decimal_ = false;
  std::string capitalization_;
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
        changed =
            shift ? model.SelectToBeginning() : model.MoveCursorToBeginning();
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
      capitalization_.clear();
      // Upstream's defaults, from `TextInputConfiguration`: autocorrection and
      // suggestions are on unless a field turns them off, and only an obscured
      // field is obscured.
      obscure_text_ = false;
      autocorrect_ = true;
      enable_suggestions_ = true;
      personalized_learning_ = true;
      number_signed_ = false;
      number_decimal_ = false;
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
          auto option = [&type](const char* key) {
            auto member = type->value.FindMember(key);
            return member != type->value.MemberEnd() && member->value.IsBool()
                       ? member->value.GetBool()
                       : false;
          };
          number_signed_ = option("signed");
          number_decimal_ = option("decimal");
        }
        auto flag = [&config](const char* key, bool fallback) {
          auto member = config.FindMember(key);
          return member != config.MemberEnd() && member->value.IsBool()
                     ? member->value.GetBool()
                     : fallback;
        };
        obscure_text_ = flag("obscureText", false);
        autocorrect_ = flag("autocorrect", true);
        enable_suggestions_ = flag("enableSuggestions", true);
        personalized_learning_ = flag("enableIMEPersonalizedLearning", true);
        auto caps = config.FindMember("textCapitalization");
        if (caps != config.MemberEnd() && caps->value.IsString()) {
          capitalization_ = caps->value.GetString();
        }
      }
      model_ = std::make_unique<TextInputModel>();
    }
    // A new field means a new editable as far as the IME is concerned: its
    // cached contents belong to the field that just lost focus. "1" says a
    // field is focused, which is what decides whether the view is a text
    // editor at all -- see RustflutterActivity.sHasClient.
    JavaBridge::SetEditingState("", 0, 0, -1, -1);
    JavaBridge::Request(HostRequest::kRestartInput, Descriptor());
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

  // And Java, which is what this class's own header says happens after every
  // change and what did not: `SetEditingState` was called from `setClient`,
  // which clears it, and from the framework's `setEditingState`, and from
  // nowhere else. So the mirror the IME reads through stayed **empty for the
  // whole time the reader was typing**.
  //
  // Every question an IME asks about the field goes through that mirror --
  // `getTextBeforeCursor`, `getSelectedText`, `getExtractedText` -- and each
  // was answered "there is nothing here". A keyboard that cannot see the word
  // being typed has no word to correct or to finish, so it starts a new one at
  // every keystroke: "wor" arrives as w, then o, then r, and never becomes
  // "work".
  //
  // The offsets need no conversion. `TextInputModel` counts UTF-16 code units
  // and a Java `String` is indexed in them, so base and extent cross as they
  // are; only the text itself is re-encoded, by `StringToJavaString`.
  JavaBridge::SetEditingState(text, selection_base, selection_extent,
                              composing_base, composing_extent);
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

  //----------------------------------------------------------------------------
  /// Hands one frame's semantics tree to the accessibility bridge in Java.
  ///
  /// Upstream this is `PlatformViewAndroid::UpdateSemantics`, which packs the
  /// nodes into two buffers and hands them to `AccessibilityBridge.java`. The
  /// destination here is the same kind of object -- an
  /// `AccessibilityNodeProvider` over the host view -- and the difference is
  /// only in the wire format: JSON, because the tree is a tree and Android
  /// already has a parser.
  ///
  /// |PlatformView|
  void UpdateSemantics(int64_t view_id,
                       SemanticsNodeUpdates update,
                       CustomAccessibilityActionUpdates actions) override {
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartArray();
    for (const auto& [id, node] : update) {
      writer.StartObject();
      writer.Key("id");
      writer.Int(node.id);
      writer.Key("actions");
      writer.Int(node.actions);

      // The flags a screen reader reads out. Sent as they are here -- named
      // rather than packed -- because the bridge on the other side sets one
      // AccessibilityNodeInfo property per flag anyway, and a bit set would
      // have to be taken apart again at both ends.
      writer.Key("button");
      writer.Bool(node.flags.isButton);
      writer.Key("textField");
      writer.Bool(node.flags.isTextField);
      writer.Key("header");
      writer.Bool(node.flags.isHeader);
      writer.Key("image");
      writer.Bool(node.flags.isImage);
      writer.Key("link");
      writer.Bool(node.flags.isLink);
      writer.Key("obscured");
      writer.Bool(node.flags.isObscured);
      writer.Key("liveRegion");
      writer.Bool(node.flags.isLiveRegion);
      // Three states, not two: "checkable at all" is a separate fact from
      // "checked", and it is the one that makes *off* sayable.
      writer.Key("checkable");
      writer.Bool(node.flags.isChecked != SemanticsCheckState::kNone);
      writer.Key("checked");
      writer.Bool(node.flags.isChecked == SemanticsCheckState::kTrue);
      writer.Key("hasEnabled");
      writer.Bool(node.flags.isEnabled != SemanticsTristate::kNone);
      writer.Key("enabled");
      writer.Bool(node.flags.isEnabled != SemanticsTristate::kFalse);
      writer.Key("selected");
      writer.Bool(node.flags.isSelected == SemanticsTristate::kTrue);
      writer.Key("focused");
      writer.Bool(node.flags.isFocused == SemanticsTristate::kTrue);

      writer.Key("label");
      writer.String(node.label.c_str());
      writer.Key("value");
      writer.String(node.value.c_str());
      writer.Key("hint");
      writer.String(node.hint.c_str());

      // Logical pixels here; Java scales by the density, because the
      // rectangle Android wants is in the view's own pixels.
      writer.Key("left");
      writer.Double(node.rect.left());
      writer.Key("top");
      writer.Double(node.rect.top());
      writer.Key("right");
      writer.Double(node.rect.right());
      writer.Key("bottom");
      writer.Double(node.rect.bottom());

      writer.Key("children");
      writer.StartArray();
      for (int32_t child : node.childrenInTraversalOrder) {
        writer.Int(child);
      }
      writer.EndArray();
      writer.EndObject();
    }
    writer.EndArray();

    JavaBridge::SemanticsUpdate(
        std::string(buffer.GetString(), buffer.GetSize()));
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

  /// Sends one key event to the framework.
  ///
  /// Keys travel as a platform message rather than a call of their own, which
  /// is what every Flutter embedder does: the packet on `flutter/keydata` is
  /// the same bytes here as on Windows and Linux, and no key-shaped method
  /// exists on PlatformView to add one to.
  ///
  /// `sequence_id` is Java's name for this key, and it travels out with the
  /// message and comes back with the answer. Zero asks for no answer at all,
  /// which is right for a key nothing could do about: a synthesized modifier
  /// has no original event to hand back to the system.
  ///
  /// The answer is one byte: 1 if the framework consumed the key. An empty
  /// reply -- no listener, or the runtime shutting down -- reads as "not
  /// consumed", which is the safe way round: a key nobody wanted goes back to
  /// Android rather than disappearing.
  ///
  /// It cannot be waited for. `onKeyDown` has to answer before this call could
  /// possibly return, so Java says yes to every key and, when the answer comes
  /// back "no", dispatches the event into the activity a second time. That is
  /// upstream's arrangement too.
  void SendKey(const KeyData& data,
               const std::string& character,
               int32_t sequence_id) {
    KeyDataPacket packet(data, character.empty() ? nullptr : character.c_str());
    fml::RefPtr<PlatformMessageResponse> response;
    if (sequence_id != 0) {
      response = fml::MakeRefCounted<HostPlatformMessageResponse>(
          task_runners_.GetPlatformTaskRunner(),
          [sequence_id](const uint8_t* reply, size_t length) {
            const bool handled =
                length > 0 && reply != nullptr && reply[0] != 0;
            JavaBridge::KeyResult(sequence_id, handled);
          });
    }
    auto message = std::make_unique<PlatformMessage>(
        kKeyDataChannel,
        fml::MallocMapping::Copy(packet.data().data(), packet.data().size()),
        std::move(response));
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
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
  void HandlePlatformMessage(
      std::unique_ptr<PlatformMessage> message) override {
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
            platform ? HandlePlatformCall(this, method->value.GetString(), args)
                     : text_input_->HandleMethodCall(method->value.GetString(),
                                                     args);
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

  /// Remakes the EGL window surface after the Surface changed size.
  ///
  /// The same thing the Windows host does on WM_SIZE, and for the same reason:
  /// an EGL surface does not follow a window that changed size, and presenting
  /// to a stale one shows the old frame or nothing at all. Android resizes a
  /// Surface in place rather than replacing it -- a rotation, a fold, a
  /// multi-window drag, or an `adjustResize` window making room for the
  /// keyboard -- and that case had no answer here.
  ///
  /// Called on the raster thread, which is where the GL context is current.
  void OnWindowResized() {
    if (gl_delegate_) {
      gl_delegate_->Resize();
    }
    // The software path asks the window what pixels it will be handed, once.
    // A resize is the other moment worth asking at: the geometry it was told
    // belonged to the old size.
    geometry_set_ = false;
  }

  /// The Surface that was here has been taken away, and the engine stays up.
  ///
  /// `AndroidSurfaceGLImpeller::TeardownOnScreenContext`, which is two lines --
  /// clear the context, drop the onscreen surface -- and this is the same two,
  /// plus the software path's copy of the frame. Called on the platform thread,
  /// straight after `NotifyDestroyed`, which is the order
  /// `PlatformViewAndroid::NotifyDestroyed` uses: the rasterizer gives its
  /// surface back first, and only then does the EGL surface it was drawing into
  /// go.
  ///
  /// The GL *context* stays, exactly as upstream's stays -- it belongs to
  /// `AndroidContextGLImpeller` rather than to the surface, and nothing in
  /// upstream's teardown path touches it. That is the difference between this
  /// and a shutdown, and it is the whole reason the application survives being
  /// backgrounded: the context, the shell, the framework and everything the
  /// reader had done are all still here, waiting for a new Surface.
  void OnSurfaceReleased() {
    window_ = nullptr;
    geometry_set_ = false;
    backing_store_.reset();
    if (gl_delegate_ == nullptr) {
      return;
    }
    // On the raster thread, because that is where the surface was made current
    // and EGL only really destroys one that is not. Clearing first is what
    // `ImpellerGlDelegate::Resize` does before dropping its surface, and what
    // upstream's teardown does before dropping its own.
    fml::AutoResetWaitableEvent latch;
    fml::TaskRunner::RunNowOrPostTask(task_runners_.GetRasterTaskRunner(),
                                      [this, &latch]() {
                                        if (gl_context_) {
                                          gl_context_->ClearCurrent();
                                        }
                                        gl_delegate_.reset();
                                        latch.Signal();
                                      });
    latch.Wait();
  }

  /// A Surface again, after [`OnSurfaceReleased`].
  ///
  /// `AndroidSurfaceGLImpeller::SetNativeWindow`, and called where upstream
  /// calls it: on the platform thread, before `NotifyCreated`, because
  /// `NotifyCreated` is what builds the rendering surface out of this window
  /// and there has to be a window by then. Nothing is reading `window_` in
  /// between -- the rasterizer has no surface at all until that call returns.
  ///
  /// One thing this does that upstream does not: nulling `window_` in
  /// `OnSurfaceReleased` rather than leaving the old one there until a new one
  /// replaces it. Upstream can leave it because everything that reads it checks
  /// the onscreen surface first; the software path here reads the window
  /// directly.
  void OnSurfaceAcquired(ANativeWindow* window) {
    window_ = window;
    // A new window has been told nothing about what pixels it will be handed.
    geometry_set_ = false;
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

  /// What this host has told the framework is held, and the machinery that
  /// keeps it in step with what Android says. Touched only from the Android
  /// UI thread, which is where every key event arrives.
  AndroidKeyboard keyboard;

  /// Set by SendChannelUpdate once the framework is listening on
  /// `flutter/platform`, which is what makes a back press a question rather
  /// than an order.
  std::atomic<bool> exit_processing{false};

  ANativeWindow* window = nullptr;

  /// Whether the shell is currently rendering into that window.
  ///
  /// False between the Activity losing its Surface and being given a new one,
  /// which is most of the time an application spends in the background. The
  /// shell is up throughout; only the surface comes and goes, and this is which
  /// of the two states it is in -- `NotifyCreated` and `NotifyDestroyed` are
  /// not idempotent enough to be called on a guess.
  bool surface_attached = false;

  /// Whether an accessibility service is listening, as the Activity last said.
  ///
  /// Kept here because the Activity asks before there is a shell to ask: it
  /// looks at `AccessibilityManager` in `onCreate`, and the shell does not
  /// exist until the Surface does. Replayed once there is somewhere to send
  /// it, the same way `PlatformData` replays everything else the platform said
  /// too early.
  bool semantics_enabled = false;

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

  /// What the system is covering, in physical pixels, as the Activity last
  /// reported it. Two kinds, kept apart the way `ViewportMetrics` keeps them:
  /// `padding` is what is drawn over (status bar, cutout, gesture bar) and
  /// `view_inset` is what displaces content (the keyboard).
  double padding_top = 0.0;
  double padding_right = 0.0;
  double padding_bottom = 0.0;
  double padding_left = 0.0;
  double view_inset_top = 0.0;
  double view_inset_right = 0.0;
  double view_inset_bottom = 0.0;
  double view_inset_left = 0.0;

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
  // The padding slot carries *view* padding, which is the same thing upstream
  // puts there: FlutterRenderer sends viewPaddingTop as physicalPaddingTop and
  // the framework subtracts the insets itself.
  metrics.physical_padding_top = state.padding_top;
  metrics.physical_padding_right = state.padding_right;
  metrics.physical_padding_bottom = state.padding_bottom;
  metrics.physical_padding_left = state.padding_left;
  metrics.physical_view_inset_top = state.view_inset_top;
  metrics.physical_view_inset_right = state.view_inset_right;
  metrics.physical_view_inset_bottom = state.view_inset_bottom;
  metrics.physical_view_inset_left = state.view_inset_left;

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

/// Gives the Surface back, and keeps everything else.
///
/// Android reclaims an Activity's Surface every time it stops being visible --
/// the reader switched to another application, or the screen turned off -- and
/// hands a new one over on the way back in. Nothing about the application is
/// over when that happens, so nothing about it is torn down here: the shell,
/// the framework, and every route, scroll position and half-filled field the
/// reader had are all still in memory, waiting for somewhere to draw.
///
/// What does go is the surface, in the two layers that have one: the
/// rasterizer's, through `NotifyDestroyed`, and then this host's own EGL window
/// surface, through `OnSurfaceReleased`. In that order, because the second is
/// what the first was drawing into -- and it is the order
/// `PlatformViewAndroid::NotifyDestroyed` uses, where `PlatformView::
/// NotifyDestroyed` comes first and `TeardownOnScreenContext` after it.
///
/// Platform thread, which is where `NotifyDestroyed` must be called from.
void DetachSurface() {
  HostState& state = HostState::Get();
  if (state.shell != nullptr && state.surface_attached) {
    if (auto view = state.shell->GetPlatformView()) {
      view->NotifyDestroyed();
    }
    if (state.platform_view != nullptr) {
      state.platform_view->OnSurfaceReleased();
    }
    state.surface_attached = false;
  }
  if (state.window != nullptr) {
    ANativeWindow_release(state.window);
    state.window = nullptr;
  }
}

/// Tears the shell down. Called from the Activity's destruction and from
/// rf_host_run if a second start ever arrived.
void Shutdown() {
  HostState& state = HostState::Get();
  if (state.shell == nullptr) {
    return;
  }
  state.text_input.Detach();
  DetachSurface();
  state.platform_view = nullptr;

  // The shell must be destroyed on the platform thread, which is this one: its
  // destructor drains the UI, raster and IO threads in order and would deadlock
  // if it were not the one holding the platform thread. That this is already
  // the platform thread is the one real simplification Android buys.
  state.shell.reset();
  state.task_runners.reset();
  state.threads.reset();
  state.lifecycle_state.clear();
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
  settings.enable_impeller =
      options == nullptr || options->enable_impeller != 0;
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
  if (state.semantics_enabled && state.platform_view != nullptr) {
    state.platform_view->SetSemanticsEnabled(true);
  }
  if (auto view = state.shell->GetPlatformView()) {
    view->NotifyCreated();
  }
  state.surface_attached = true;
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
  //
  // Reached through the pointer the application left in rf_set_app_main rather
  // than by name, because by name is a call out of this library and up into
  // whoever loaded it once the engine is a shared library of its own. See
  // rustflutter_host.h.
  RfAppMain app_main = rf_app_main();
  FML_CHECK(app_main != nullptr)
      << "No application entry point is registered. The application registers "
         "rustflutter_app_main from a load-time initialiser; a library that "
         "left it out has none, and there is nothing here to start.";
  app_main(0, nullptr);
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeSurfaceChanged(
    JNIEnv* env,
    jclass clazz,
    jint width,
    jint height,
    jfloat device_pixel_ratio) {
  auto& state = flutter::HostState::Get();
  const bool resized = state.width != width || state.height != height;
  state.width = width;
  state.height = height;
  state.device_pixel_ratio = device_pixel_ratio > 0 ? device_pixel_ratio : 1.0;

  // Before the metrics, and on the raster thread where the GL context lives.
  // Without this the engine draws the new size into a surface that is still
  // the old one.
  if (resized && state.platform_view != nullptr && state.task_runners) {
    state.task_runners->GetRasterTaskRunner()->PostTask(
        [view = state.platform_view]() { view->OnWindowResized(); });
  }
  flutter::SendViewportMetrics();
}

/// The Activity has lost its Surface, and has not lost the application.
///
/// What used to happen here was a full shutdown, on the reasoning that this
/// fork has one Activity and no engine cache, so "the Surface is gone" and "the
/// application is gone" were the same event. They are not: Android takes the
/// Surface away every time the reader looks at something else. Coming back
/// built a second shell and ran the application's `main` a second time, and the
/// reader found themselves on the first screen with everything they had done
/// gone. The application ends at `onDestroy`, which is where `nativeStop` still
/// is; this is only the Surface.
///
/// Upstream's is `SurfaceDestroyed` in platform_view_android_jni_impl.cc, which
/// is one line -- `NotifyDestroyed` -- and reached from
/// `FlutterSurfaceView.surfaceDestroyed` by way of
/// `FlutterRenderer.stopRenderingToSurface`. Nothing on that path destroys an
/// engine; `FlutterEngine.destroy` is called from `onDetach` and nowhere else.
JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeSurfaceDestroyed(
    JNIEnv* env,
    jclass clazz) {
  flutter::DetachSurface();
}

/// A Surface again, for the shell that has been up all along.
///
/// Upstream's `SurfaceCreated`, which is the same two steps in the same order:
/// the window to the platform view first, then `NotifyCreated`, which asks the
/// shell for a rendering surface and -- through `Shell::OnPlatformViewCreated`
/// -- schedules a frame. So what the reader sees on the way back in is the
/// screen they left, drawn again rather than built again.
///
/// The counterpart of nativeSurfaceDestroyed, and deliberately not
/// nativeSurfaceCreated: that one is the first Surface, and starts the
/// application. This one starts nothing. Upstream has no such split because
/// starting the application is not a surface event over there at all -- the
/// engine is created and runs its entry point before a Surface exists.
JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeSurfaceRecreated(
    JNIEnv* env,
    jclass clazz,
    jobject surface,
    jint width,
    jint height,
    jfloat device_pixel_ratio) {
  auto& state = flutter::HostState::Get();
  if (state.window != nullptr) {
    ANativeWindow_release(state.window);
  }
  state.window = ANativeWindow_fromSurface(env, surface);
  state.width = width;
  state.height = height;
  state.device_pixel_ratio = device_pixel_ratio > 0 ? device_pixel_ratio : 1.0;
  if (state.shell == nullptr || state.platform_view == nullptr) {
    return;
  }
  // Before NotifyCreated, which is what makes the rendering surface out of it.
  state.platform_view->OnSurfaceAcquired(state.window);
  if (auto view = state.shell->GetPlatformView()) {
    view->NotifyCreated();
  }
  state.surface_attached = true;
  // The window may be a different size than the one that went away -- the
  // reader could have folded, rotated or resized while they were elsewhere.
  flutter::SendViewportMetrics();
}

/// What the system bars, a cutout and the keyboard are covering.
///
/// Arrives whenever Android applies window insets, which is at least once
/// before the first frame and again every time the keyboard opens or closes.
JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeInsets(
    JNIEnv* env,
    jclass clazz,
    jint padding_top,
    jint padding_right,
    jint padding_bottom,
    jint padding_left,
    jint inset_top,
    jint inset_right,
    jint inset_bottom,
    jint inset_left) {
  auto& state = flutter::HostState::Get();
  state.padding_top = padding_top;
  state.padding_right = padding_right;
  state.padding_bottom = padding_bottom;
  state.padding_left = padding_left;
  state.view_inset_top = inset_top;
  state.view_inset_right = inset_right;
  state.view_inset_bottom = inset_bottom;
  state.view_inset_left = inset_left;
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
  flutter::SendLifecycle(fml::jni::JavaStringToString(env, state_name).c_str());
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

  const bool down = data.change == flutter::PointerData::Change::kDown ||
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
Java_io_flutter_rustflutter_RustflutterActivity_nativeComposing(JNIEnv* env,
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

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeComposingRegion(
    JNIEnv* env,
    jclass clazz,
    jint start,
    jint end) {
  flutter::HostState::Get().text_input.OnComposingRegion(start, end);
}

JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeSetSelection(JNIEnv* env,
                                                                   jclass clazz,
                                                                   jint start,
                                                                   jint end) {
  flutter::HostState::Get().text_input.OnSelection(start, end);
}

/// One hardware key, on its way to the framework's shortcut tables and focus
/// traversal.
///
/// Android gives a key two numbers and this needs both: `key_code` is what the
/// key means -- the layout is already applied by the time the event is
/// delivered -- and `scan_code` is where it is, which is what makes a release
/// cancel the right press when a layout changes mid-key.
///
/// `meta_state` is which modifiers Android believes are held, which is not
/// always what this host has told the framework; `AndroidKeyboard` invents
/// whatever presses and releases make the two agree. `virtual_keyboard` says
/// the event came from an on-screen keyboard, which is believed rather than
/// corrected.
///
/// `character` is what the key typed, or null for a key that typed nothing.
/// Java computes it, because `KeyCharacterMap` is where the layout lives and
/// there is no way to ask it from here.
///
/// One event in can be several out, or none at all. See AndroidKeyboard.
JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeKey(
    JNIEnv* env,
    jclass clazz,
    jint key_code,
    jint scan_code,
    jint meta_state,
    jboolean down,
    jboolean repeat,
    jboolean virtual_keyboard,
    jstring character,
    jint sequence_id) {
  auto& state = flutter::HostState::Get();
  if (state.platform_view == nullptr) {
    return;
  }

  flutter::AndroidKeyEvent event;
  event.key_code = static_cast<uint32_t>(key_code);
  event.scan_code = static_cast<uint32_t>(scan_code);
  event.meta_state = static_cast<int32_t>(meta_state);
  event.down = down == JNI_TRUE;
  event.repeat = repeat == JNI_TRUE;
  event.virtual_keyboard = virtual_keyboard == JNI_TRUE;
  // Android's own event timestamp is in milliseconds since boot; the framework
  // counts microseconds, and only differences between key events are ever read
  // from it, so the epoch does not have to agree with anything.
  event.timestamp_micros = static_cast<uint64_t>(
      fml::TimePoint::Now().ToEpochDelta().ToMicroseconds());

  std::string text;
  if (character != nullptr) {
    text = fml::jni::JavaStringToString(env, character);
  }

  // Only the real event carries the sequence id, and so only it is answered.
  // The synthesized ones are this host's own invention -- there is no original
  // Android event behind them to give back, so an answer would have nowhere to
  // go. `synthesized` is what tells them apart, which is the same flag the
  // framework reads them by.
  flutter::HostPlatformView* view = state.platform_view;
  const int32_t sequence = static_cast<int32_t>(sequence_id);
  const bool sent = state.keyboard.Handle(
      event, text,
      [view, sequence](const flutter::KeyData& data,
                       const std::string& character) {
        view->SendKey(data, character, data.synthesized != 0 ? 0 : sequence);
      });
  if (!sent && sequence != 0) {
    // The event was dropped -- an abrupt release, or a key with no numbers at
    // all. Nothing will ever answer for it, and Java is holding it waiting.
    // The framework never saw it, so it did not handle it, and that is the
    // answer.
    flutter::JavaBridge::KeyResult(sequence, false);
  }
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

/// A screen reader arriving or leaving.
///
/// Upstream this is `FlutterJNI.setSemanticsEnabled`, called from
/// `AccessibilityBridge` when Android's `AccessibilityManager` says touch
/// exploration went on or off. Nothing is built while it is off, so this is
/// the switch that decides whether there is an accessibility tree at all.
JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeSemanticsEnabled(
    JNIEnv* env,
    jclass clazz,
    jboolean enabled) {
  auto& state = flutter::HostState::Get();
  state.semantics_enabled = enabled == JNI_TRUE;
  if (state.platform_view == nullptr) {
    // Asked before the shell exists, which is the ordinary order: the Activity
    // reads AccessibilityManager in onCreate and the Surface arrives after.
    // Replayed when the shell starts.
    return;
  }
  state.platform_view->SetSemanticsEnabled(state.semantics_enabled);
}

/// An action a screen reader asked for.
///
/// `action` is one flutter::SemanticsAction bit, chosen by the bridge from
/// whatever `AccessibilityNodeInfo` action Android delivered.
JNIEXPORT void JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeSemanticsAction(
    JNIEnv* env,
    jclass clazz,
    jint node_id,
    jint action) {
  auto& state = flutter::HostState::Get();
  if (state.platform_view == nullptr) {
    return;
  }
  state.platform_view->DispatchSemanticsAction(
      flutter::kFlutterImplicitViewId, node_id,
      static_cast<flutter::SemanticsAction>(action), fml::MallocMapping());
}

/// The back gesture.
///
/// Returns true if the framework was asked and false if the Activity should
/// just finish. The difference is whether anything over there is listening: a
/// back press that silently did nothing would be worse than one that leaves.
JNIEXPORT jboolean JNICALL
Java_io_flutter_rustflutter_RustflutterActivity_nativeBackPressed(
    JNIEnv* env,
    jclass clazz) {
  auto& state = flutter::HostState::Get();
  if (state.platform_view == nullptr) {
    return JNI_FALSE;
  }
  state.platform_view->SendPopRoute();
  return JNI_TRUE;
}

}  // extern "C"
