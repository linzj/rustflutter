// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_TEXT_INPUT_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_TEXT_INPUT_H_

// The platform half of `flutter/textinput`, shared by the macOS and iOS
// hosts. The framework opens an editing session (`TextInput.setClient`) and
// from then on the *platform* owns the editing: every key or IME event is
// applied to a model here and reported back as
// `TextInputClient.updateEditingState`.
//
// The editing model is the engine's own `flutter::TextInputModel`, the same
// class the Windows host edits (`rustflutter_host_win.cc`, whose handler this
// grew out of). What differs per platform is who calls in: on macOS an
// `NSTextInputClient` view, on iOS a `UITextInput` one; both speak committed
// text, marked text, and editing keys, which is exactly this class's surface.
//
// Channel calls arrive on the platform thread and input events on the main
// thread, so the model sits behind a mutex, as it does on Windows.

#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>

#include "flutter/shell/platform/common/text_input_model.h"
#include "rapidjson/document.h"
#include "rapidjson/stringbuffer.h"
#include "rapidjson/writer.h"

namespace flutter {

/// The framework's text field, as the platform sees it.
class TextInputHandler {
 public:
  /// How a state update leaves here. Set once, by the platform view.
  using Sender = std::function<void(const std::string& method,
                                    const std::string& arguments_json)>;

  void SetSender(Sender sender) { sender_ = std::move(sender); }

  /// Called after the *framework* changes the editing state -- a
  /// `setEditingState`, a fresh `setClient` -- so a platform input system
  /// that keeps its own notion of the text (iOS's keyboard) can be told to
  /// re-read it. Runs on the platform thread; the hook hops if it must.
  void SetOnFrameworkStateChanged(std::function<void()> hook) {
    framework_state_changed_ = std::move(hook);
  }

  /// True once the framework has attached a field. Everything typed while
  /// this is false goes nowhere, which is correct: there is nothing to type
  /// into.
  bool attached() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return model_ != nullptr;
  }

  // -- What the field was configured with, for a platform keyboard's traits --

  int client_id() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return client_id_;
  }

  std::string input_type() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return input_type_;
  }

  std::string input_action() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return input_action_;
  }

  bool obscure_text() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return obscure_text_;
  }

  bool autocorrect() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return autocorrect_;
  }

  /// Handles one call on `flutter/textinput`. Platform thread.
  std::optional<std::string> HandleMethodCall(const std::string& method,
                                              const rapidjson::Value* args) {
    if (method == "TextInput.show" || method == "TextInput.hide") {
      // No-ops here: raising a software keyboard is the host's business, and
      // the host watches these methods go past (see the iOS host's
      // HandlePlatformMessage).
      return NullEnvelope();
    }

    if (method == "TextInput.setClient") {
      // `[clientId, config]`. The config carries the action and the type.
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
        obscure_text_ = false;
        autocorrect_ = true;
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
          auto obscure = config.FindMember("obscureText");
          if (obscure != config.MemberEnd() && obscure->value.IsBool()) {
            obscure_text_ = obscure->value.GetBool();
          }
          auto correct = config.FindMember("autocorrect");
          if (correct != config.MemberEnd() && correct->value.IsBool()) {
            autocorrect_ = correct->value.GetBool();
          }
        }
        model_ = std::make_unique<TextInputModel>();
      }
      NotifyFrameworkStateChanged();
      return NullEnvelope();
    }

    if (method == "TextInput.clearClient") {
      {
        std::lock_guard<std::mutex> lock(mutex_);
        model_.reset();
      }
      NotifyFrameworkStateChanged();
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

      {
        std::lock_guard<std::mutex> lock(mutex_);
        if (model_ == nullptr) {
          return ErrorEnvelope(
              "TextInput.noClient",
              "the editing state was set with no client attached");
        }
        // The framework is the authority here: this is it telling the
        // platform what the field now holds, which is how a programmatic
        // edit -- a paste, a tap moving the caret -- reaches the model.
        model_->SetText(
            text->value.GetString(),
            TextRange(static_cast<size_t>(base < 0 ? 0 : base),
                      static_cast<size_t>(extent < 0 ? 0 : extent)));
      }
      NotifyFrameworkStateChanged();
      return NullEnvelope();
    }

    if (method == "TextInput.setMarkedTextRect") {
      // Where the caret is, in the editable's own coordinates. This plus the
      // transform is where an IME's candidate window goes.
      if (args == nullptr || !args->IsObject()) {
        return ErrorEnvelope("TextInput.badArgument",
                             "Method invoked without args");
      }
      auto number = [args](const char* key, bool* found_it) {
        auto found = args->FindMember(key);
        *found_it = found != args->MemberEnd() && found->value.IsNumber();
        return *found_it ? found->value.GetDouble() : 0.0;
      };
      bool ok[4] = {};
      const double x = number("x", &ok[0]);
      const double y = number("y", &ok[1]);
      const double width = number("width", &ok[2]);
      const double height = number("height", &ok[3]);
      if (!ok[0] || !ok[1] || !ok[2] || !ok[3]) {
        return ErrorEnvelope("TextInput.badArgument",
                             "Composing rect values invalid.");
      }
      std::lock_guard<std::mutex> lock(mutex_);
      marked_x_ = x;
      marked_y_ = y;
      marked_width_ = width;
      marked_height_ = height;
      caret_valid_ = true;
      return NullEnvelope();
    }

    if (method == "TextInput.setEditableSizeAndTransform") {
      // A 4x4 matrix, row-major; only its translation is used, which is
      // entries 12 and 13 -- a candidate window cannot be rotated.
      if (args == nullptr || !args->IsObject()) {
        return ErrorEnvelope("TextInput.badArgument",
                             "Method invoked without args");
      }
      auto transform = args->FindMember("transform");
      if (transform == args->MemberEnd() || !transform->value.IsArray() ||
          transform->value.Size() != 16) {
        return ErrorEnvelope("TextInput.badArgument",
                             "EditableText transform invalid.");
      }
      const rapidjson::Value& matrix = transform->value;
      if (!matrix[12].IsNumber() || !matrix[13].IsNumber()) {
        return ErrorEnvelope("TextInput.badArgument",
                             "EditableText transform contains null value.");
      }
      std::lock_guard<std::mutex> lock(mutex_);
      transform_x_ = matrix[12].GetDouble();
      transform_y_ = matrix[13].GetDouble();
      caret_valid_ = true;
      return NullEnvelope();
    }

    if (method == "TextInput.setCaretRect") {
      return NullEnvelope();
    }

    return std::nullopt;
  }

  // -- The input system's half ------------------------------------------------

  /// Committed text. During a composition this is the IME cashing in the
  /// marked text; outside one it is a plain keystroke.
  void OnInsertText(const std::u16string& text) {
    if (Edit([&text](TextInputModel& model) {
          if (model.composing()) {
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

  /// The composition as it stands. `cursor`/`length` are where the IME's own
  /// selection sits inside the marked text.
  void OnSetMarkedText(const std::u16string& text, long cursor, long length) {
    if (Edit([&](TextInputModel& model) {
          if (!model.composing()) {
            model.BeginComposing();
          }
          const size_t base = cursor < 0 ? 0 : static_cast<size_t>(cursor);
          const size_t extent =
              base + (length < 0 ? 0 : static_cast<size_t>(length));
          model.UpdateComposingText(text, TextRange(base, extent));
          return true;
        })) {
      SendStateUpdate();
    }
  }

  /// The composition is taken as it stands.
  void OnUnmarkText() {
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

  /// Backspace -- iOS's `deleteBackward`, which deletes the selection when
  /// there is one and the character before the caret when there is not,
  /// which is exactly what the model's `Backspace` does.
  void OnDeleteBackward() {
    if (Edit([](TextInputModel& model) { return model.Backspace(); })) {
      SendStateUpdate();
    }
  }

  /// The platform moved the selection -- iOS's `setSelectedTextRange:`,
  /// which the keyboard uses for its own cursor gestures.
  void OnSetSelection(long base, long extent) {
    if (Edit([&](TextInputModel& model) {
          return model.SetSelection(
              TextRange(base < 0 ? 0 : static_cast<size_t>(base),
                        extent < 0 ? 0 : static_cast<size_t>(extent)));
        })) {
      SendStateUpdate();
    }
  }

  /// The platform replaced a run of text -- iOS's `replaceRange:withText:`,
  /// autocorrect's and dictation's spelling of an edit.
  void OnReplaceRange(long location, long length, const std::u16string& text) {
    if (Edit([&](TextInputModel& model) {
          const size_t start = location < 0 ? 0 : static_cast<size_t>(location);
          model.SetSelection(TextRange(
              start, start + (length < 0 ? 0 : static_cast<size_t>(length))));
          model.AddText(text);
          return true;
        })) {
      SendStateUpdate();
    }
  }

  bool Composing() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return model_ != nullptr && model_->composing();
  }

  /// The whole text, UTF-8. NSString converts it back to the UTF-16 the
  /// ranges below index into.
  std::string GetText() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return model_ == nullptr ? std::string() : model_->GetText();
  }

  /// The marked range in UTF-16 units; `location` is -1 when nothing is
  /// being composed. `NSRange`'s own units, which is the point.
  void GetMarkedRange(long* location, long* length) const {
    std::lock_guard<std::mutex> lock(mutex_);
    if (model_ == nullptr || !model_->composing()) {
      *location = -1;
      *length = 0;
      return;
    }
    const TextRange range = model_->composing_range();
    *location = static_cast<long>(range.start());
    *length = static_cast<long>(range.length());
  }

  void GetSelectedRange(long* location, long* length) const {
    std::lock_guard<std::mutex> lock(mutex_);
    if (model_ == nullptr) {
      *location = -1;
      *length = 0;
      return;
    }
    const TextRange range = model_->selection();
    *location = static_cast<long>(range.start());
    *length = static_cast<long>(range.length());
  }

  /// Where the caret is in the view, logical pixels: the marked rectangle's
  /// origin put through the editable's transform, both reported by the
  /// framework at paint.
  bool GetCaretRect(double* x, double* y, double* width, double* height) const {
    std::lock_guard<std::mutex> lock(mutex_);
    if (!caret_valid_) {
      return false;
    }
    *x = marked_x_ + transform_x_;
    *y = marked_y_ + transform_y_;
    *width = marked_width_;
    *height = marked_height_;
    return true;
  }

  /// An editing key by macOS virtual key code: backspace, forward delete,
  /// the arrows, home and end. Returns true if the field used it.
  bool OnEditingKey(unsigned short key_code, bool shift) {
    bool changed = false;
    const bool handled = Edit([&](TextInputModel& model) {
      switch (key_code) {
        case 0x33:  // Delete (backspace).
          changed = model.Backspace();
          return true;
        case 0x75:  // Forward delete.
          changed = model.Delete();
          return true;
        case 0x7B:  // Left arrow.
          changed = model.MoveCursorBack();
          return true;
        case 0x7C:  // Right arrow.
          changed = model.MoveCursorForward();
          return true;
        case 0x73:  // Home.
          changed =
              shift ? model.SelectToBeginning() : model.MoveCursorToBeginning();
          return true;
        case 0x77:  // End.
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

  /// Return, which submits rather than edits -- except in a multiline field
  /// whose action is newline, which gets both, upstream's `EnterPressed`.
  void OnAction() {
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

 private:
  static std::string NullEnvelope() {
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartArray();
    writer.Null();
    writer.EndArray();
    return buffer.GetString();
  }

  static std::string ErrorEnvelope(const char* code,
                                   const std::string& message) {
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartArray();
    writer.String(code);
    writer.String(message.c_str(),
                  static_cast<rapidjson::SizeType>(message.size()));
    writer.Null();
    writer.EndArray();
    return buffer.GetString();
  }

  /// Runs `edit` against the model, if there is one. Returns what it
  /// returned, or false when no client is attached.
  bool Edit(const std::function<bool(TextInputModel&)>& edit) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (model_ == nullptr) {
      return false;
    }
    return edit(*model_);
  }

  void NotifyFrameworkStateChanged() {
    if (framework_state_changed_) {
      framework_state_changed_();
    }
  }

  void SendStateUpdate() {
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
    // The keys, and their order, are upstream's. A field the framework does
    // not find is a field it substitutes a default for, silently.
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

  mutable std::mutex mutex_;
  std::unique_ptr<TextInputModel> model_;
  int client_id_ = 0;
  std::string input_action_;
  std::string input_type_;
  bool obscure_text_ = false;
  bool autocorrect_ = true;
  double marked_x_ = 0;
  double marked_y_ = 0;
  double marked_width_ = 0;
  double marked_height_ = 0;
  double transform_x_ = 0;
  double transform_y_ = 0;
  bool caret_valid_ = false;
  Sender sender_;
  std::function<void()> framework_state_changed_;
};

}  // namespace flutter

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_TEXT_INPUT_H_
