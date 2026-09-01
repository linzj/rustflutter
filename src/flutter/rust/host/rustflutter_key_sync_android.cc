// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/rust/host/rustflutter_key_sync_android.h"

#include <vector>

#include "flutter/rust/host/rustflutter_key_map_android.h"

namespace flutter {

void AndroidKeyboard::Synthesize(bool down,
                                 uint64_t logical,
                                 uint64_t physical,
                                 uint64_t timestamp_micros,
                                 const Emit& emit) {
  if (down) {
    pressing_records_[physical] = logical;
  } else {
    pressing_records_.erase(physical);
  }

  KeyData data;
  data.Clear();
  data.timestamp = timestamp_micros;
  data.type = down ? KeyEventType::kDown : KeyEventType::kUp;
  data.physical = physical;
  data.logical = logical;
  // The one place this flag is set. It tells the framework that no finger did
  // this, which matters for anything that counts keystrokes rather than
  // tracking state -- a synthesized press must not type a character, and it
  // carries none.
  data.synthesized = 1;
  emit(data, std::string());
}

void AndroidKeyboard::Synchronize(bool true_pressed,
                                  const PressingGoal& goal,
                                  uint64_t event_logical,
                                  uint64_t event_physical,
                                  const AndroidKeyEvent& event,
                                  const Emit& emit,
                                  bool* release_after) {
  // The shape of the problem, in upstream's words: there is a state now, a
  // state the event assumes it starts from, and the true state afterwards.
  //
  //   now ---(synthesized before)--> pre-event ---(event)--> true
  //
  // The job is to pick a pre-event state that both reaches the true state and
  // needs as few invented events as possible.
  std::vector<bool> now(goal.count);
  std::vector<int> pre(goal.count, -1);  // -1 for "not decided yet"
  bool any_pressed_after = false;

  for (size_t index = 0; index < goal.count; ++index) {
    const ModifierKeyPair& key = goal.keys[index];
    now[index] = pressing_records_.count(key.physical) > 0;

    if (key.logical != event_logical) {
      any_pressed_after = any_pressed_after || now[index];
      continue;
    }

    // This goal's own key is the one being pressed or released right now, so
    // its pre-event state is not a guess: the event itself says what it was.
    if (!event.down) {
      // A release. The key should have been down, but do not invent a press to
      // make that true -- an unmatched release is dropped later instead, and
      // inventing a press here would turn a stray release into a keystroke.
      pre[index] = now[index] ? 1 : 0;
      continue;
    }
    if (event.repeat) {
      // A repeat means it was already down, so nothing needs inventing before.
      // A press must not be invented here either: a down *and* a repeat would
      // both carry the printable character, and the key would type twice.
      pre[index] = now[index] ? 1 : 0;
      any_pressed_after = true;
      if (!true_pressed) {
        *release_after = true;
      }
      continue;
    }
    pre[index] = 0;
    any_pressed_after = true;
    if (!true_pressed) {
      // Android says the modifier is *not* held even though this event is its
      // press. Its release has to follow the event rather than precede it,
      // otherwise the press would arrive for a key already recorded as down.
      *release_after = true;
    }
  }

  if (true_pressed) {
    // At least one of the pair has to end up held.
    for (size_t index = 0; index < goal.count; ++index) {
      if (pre[index] != -1) {
        continue;
      }
      // An on-screen keyboard is trusted rather than corrected. Gboard leaves
      // the Shift bit set without ever sending a Shift key event, so inventing
      // the press it implies would leave a modifier held that nothing will
      // ever release.
      if (any_pressed_after || event.virtual_keyboard) {
        pre[index] = now[index] ? 1 : 0;
      } else {
        pre[index] = 1;
        any_pressed_after = true;
      }
    }
    if (!any_pressed_after && !event.virtual_keyboard) {
      // Nothing was chosen and the bit says something must be held. The left
      // one is as good a guess as exists -- Android's unsided bit does not say
      // which, and a wrong side still gives the right answer to "is Shift
      // down".
      pre[0] = 1;
    }
  } else {
    for (size_t index = 0; index < goal.count; ++index) {
      if (pre[index] == -1) {
        pre[index] = 0;
      }
    }
  }

  for (size_t index = 0; index < goal.count; ++index) {
    const bool wanted = pre[index] == 1;
    if (now[index] != wanted) {
      const ModifierKeyPair& key = goal.keys[index];
      Synthesize(wanted, key.logical, key.physical, event.timestamp_micros,
                 emit);
    }
  }
}

void AndroidKeyboard::SynchronizeToggling(bool true_enabled,
                                          const TogglingGoal& goal,
                                          uint64_t event_logical,
                                          const AndroidKeyEvent& event,
                                          const Emit& emit) {
  // Not for the lock key's own events. Upstream's reason is ChromeOS, where
  // CapsLock's own events set the bit as if it were a *held* modifier -- 1 on
  // the way down, 0 on the way up -- while every other event sets it as the
  // lock state it is meant to be. Reconciling against a bit that means
  // something different on this one event would toggle the lock twice.
  if (goal.logical == event_logical) {
    return;
  }
  if (enabled_locks_.count(goal.logical) == (true_enabled ? 1u : 0u)) {
    return;
  }

  // Two events, not one, and this is the crux of a lock. The framework flips
  // the mode on each *key down* of the lock key -- that is what a lock is made
  // of -- so a single synthesized event would either flip nothing (an up) or
  // flip it and leave the key recorded as held forever (a down).
  //
  // Which comes first depends on where the key is now: if it is not held, the
  // down is what flips the mode and the up puts it back; if it somehow is
  // held, the up comes first and the down that follows does the flipping.
  const bool first_is_down = pressing_records_.count(goal.physical) == 0;
  if (true_enabled) {
    enabled_locks_.insert(goal.logical);
  } else {
    enabled_locks_.erase(goal.logical);
  }
  Synthesize(first_is_down, goal.logical, goal.physical, event.timestamp_micros,
             emit);
  Synthesize(!first_is_down, goal.logical, goal.physical,
             event.timestamp_micros, emit);
}

bool AndroidKeyboard::IsLockEnabled(uint64_t logical) const {
  return enabled_locks_.count(logical) > 0;
}

bool AndroidKeyboard::Handle(const AndroidKeyEvent& event,
                             const std::string& character,
                             const Emit& emit) {
  // An event with neither number is not a key. It cannot be mapped, and the
  // physical key synthesized from a zero key code would be the same value for
  // every one of them.
  if (event.key_code == 0 && event.scan_code == 0) {
    return false;
  }

  const uint64_t physical =
      PhysicalKeyForAndroidKey(event.scan_code, event.key_code);
  const uint64_t logical = LogicalKeyForAndroidKeyCode(event.key_code);

  // Which of this event's own goal keys need releasing *after* it rather than
  // before. Upstream collects closures; there are at most three, and each is
  // the same key as the event, so one flag per goal is the whole of it.
  std::vector<const PressingGoal*> release_after;
  size_t goal_count = 0;
  const PressingGoal* goals = AndroidPressingGoals(&goal_count);
  for (size_t index = 0; index < goal_count; ++index) {
    bool after = false;
    Synchronize((event.meta_state & goals[index].mask) != 0, goals[index],
                logical, physical, event, emit, &after);
    if (after) {
      release_after.push_back(&goals[index]);
    }
  }

  size_t toggle_count = 0;
  const TogglingGoal* toggles = AndroidTogglingGoals(&toggle_count);
  for (size_t index = 0; index < toggle_count; ++index) {
    SynchronizeToggling((event.meta_state & toggles[index].mask) != 0,
                        toggles[index], logical, event, emit);
  }

  KeyEventType type;
  std::string text = character;
  auto recorded = pressing_records_.find(physical);
  if (event.down) {
    if (recorded == pressing_records_.end()) {
      type = KeyEventType::kDown;
    } else if (event.repeat) {
      type = KeyEventType::kRepeat;
    } else {
      // A press for a key already held, and not a repeat: the release went
      // missing. Invent it, so the press that follows is not a second press of
      // a key the framework already has down.
      Synthesize(false, recorded->second, physical, event.timestamp_micros,
                 emit);
      type = KeyEventType::kDown;
    }
  } else {
    if (recorded == pressing_records_.end()) {
      // An abrupt release: nothing here ever saw it pressed. Passing it on
      // would ask the framework to remove a key it does not have.
      return false;
    }
    type = KeyEventType::kUp;
    // Only a press types. A release carries no character even when Android
    // would happily map one.
    text.clear();
  }

  if (type != KeyEventType::kRepeat) {
    if (event.down) {
      pressing_records_[physical] = logical;
    } else {
      pressing_records_.erase(physical);
    }
  }

  // The lock key's own press flips the lock here rather than through the
  // synchronisation above, which skipped this event on purpose. Down only: a
  // release does not flip a lock, and neither does a repeat, or holding
  // CapsLock would make it flicker.
  if (type == KeyEventType::kDown) {
    size_t toggle_count = 0;
    const TogglingGoal* toggles = AndroidTogglingGoals(&toggle_count);
    for (size_t index = 0; index < toggle_count; ++index) {
      if (toggles[index].logical != logical) {
        continue;
      }
      if (enabled_locks_.count(logical) > 0) {
        enabled_locks_.erase(logical);
      } else {
        enabled_locks_.insert(logical);
      }
    }
  }

  KeyData data;
  data.Clear();
  data.timestamp = event.timestamp_micros;
  data.type = type;
  data.physical = physical;
  data.logical = logical;
  data.synthesized = 0;
  emit(data, text);

  for (const PressingGoal* goal : release_after) {
    for (size_t index = 0; index < goal->count; ++index) {
      if (goal->keys[index].logical == logical) {
        Synthesize(false, logical, physical, event.timestamp_micros, emit);
      }
    }
  }
  return true;
}

bool AndroidKeyboard::IsPressed(uint64_t physical) const {
  return pressing_records_.count(physical) > 0;
}

}  // namespace flutter
