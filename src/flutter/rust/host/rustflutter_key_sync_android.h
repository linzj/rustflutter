// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_SYNC_ANDROID_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_SYNC_ANDROID_H_

#include <cstdint>
#include <functional>
#include <map>
#include <string>

#include "flutter/lib/ui/window/key_data.h"

namespace flutter {

//------------------------------------------------------------------------------
/// One Android key event, as much of it as the framework needs.
///
/// Upstream this is a `KeyEvent` object; here the JNI layer takes it apart so
/// that everything below is ordinary C++ and can be tested off a device.
struct AndroidKeyEvent {
  uint32_t key_code = 0;
  uint32_t scan_code = 0;
  /// `KeyEvent.getMetaState()`. Which modifiers Android believes are held --
  /// which is not always what this host has been told.
  int32_t meta_state = 0;
  bool down = false;
  /// `KeyEvent.getRepeatCount() > 0`.
  bool repeat = false;
  /// `getDeviceId() == KeyCharacterMap.VIRTUAL_KEYBOARD`. An on-screen
  /// keyboard is a special case; see the .cc.
  bool virtual_keyboard = false;
  uint64_t timestamp_micros = 0;
};

//------------------------------------------------------------------------------
/// Turns Android key events into the stream of key events the framework
/// expects, inventing the ones Android never sent.
///
/// # Why anything has to be invented
///
/// The framework keeps a set of held keys and answers "is Shift down?" from
/// it. That set is built from the events it receives, so it is only ever as
/// right as the stream is complete -- and Android's stream is not complete.
/// An app started while Shift was held, a key released over another window, a
/// modifier consumed by the system: all of them leave a press with no release
/// or a release with no press, and the framework then believes a key is held
/// that nobody is holding, or misses one that is.
///
/// Android does say what it believes, in `getMetaState()`, and every event
/// carries it. So each event is an opportunity to compare, and to send whatever
/// synthesized presses and releases make the two agree before the real event
/// goes out. This is upstream's `KeyEmbedderResponder`, and the record below is
/// its `pressingRecords`.
///
/// # Why the record is kept here and not asked for
///
/// The framework's own set is on the far side of an asynchronous hop -- keys
/// go out as a platform message and are handled on the platform thread -- so
/// it cannot be consulted while deciding what to send. This class keeps its
/// own, which is what upstream does for the same reason. The two are the same
/// set only because everything that changes one goes through here.
class AndroidKeyboard {
 public:
  /// Where a finished event goes. The character is what the key typed, empty
  /// for a key that typed nothing.
  using Emit = std::function<void(const KeyData&, const std::string&)>;

  //----------------------------------------------------------------------------
  /// Handles one event, emitting between zero and several.
  ///
  /// Zero happens: a release for a key that was never recorded as pressed is
  /// dropped rather than passed on, because a release the framework cannot
  /// match would take a key out of its held set that something else put there.
  ///
  /// Returns whether the real event was emitted at all.
  bool Handle(const AndroidKeyEvent& event,
              const std::string& character,
              const Emit& emit);

  /// Whether this physical key is recorded as held. For tests.
  bool IsPressed(uint64_t physical) const;

 private:
  void Synchronize(bool true_pressed,
                   const struct PressingGoal& goal,
                   uint64_t event_logical,
                   uint64_t event_physical,
                   const AndroidKeyEvent& event,
                   const Emit& emit,
                   bool* release_after);

  void Synthesize(bool down,
                  uint64_t logical,
                  uint64_t physical,
                  uint64_t timestamp_micros,
                  const Emit& emit);

  /// Physical key to the logical key it was pressed with. The logical key is
  /// kept because a release has to carry the same one the press did, even if
  /// the layout changed in between -- otherwise the framework cannot match
  /// them.
  std::map<uint64_t, uint64_t> pressing_records_;
};

}  // namespace flutter

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_SYNC_ANDROID_H_
