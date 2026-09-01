// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_ANDROID_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_ANDROID_H_

#include <stdint.h>

namespace flutter {

// Android says a key twice, from two different numbers, and the two are not
// interchangeable. `KeyEvent.getScanCode()` is the Linux evdev code -- where
// the finger went -- and `KeyEvent.getKeyCode()` is Android's own idea of what
// the key means. Flutter wants exactly this pair: physical for "is it still
// down" (the layout may change between the press and the release, and the
// release must still cancel the press), logical for what a shortcut binds.

// The high bits of a key value say which namespace it was drawn from. A key
// neither table names keeps its Android number, in Android's plane, rather
// than colliding with a real Unicode codepoint or with another host's unnamed
// key.
//
// The companion `kValueMask` -- the low 34 bits a key value can occupy -- is
// deliberately *not* here, though the other three key-map headers each declare
// their own copy. This is the one that is compiled on every platform, so it is
// the one whose copy could be included alongside another's and collide. It
// lives in the .cc instead. Four homes for one number is still four homes;
// unifying them means touching the mac and linux maps, which cannot be built
// or run from here, so it is written down rather than half-done.
constexpr uint64_t kAndroidPlane = 0x01100000000;

//------------------------------------------------------------------------------
/// The USB HID usage code for a key, from an Android `KeyEvent`.
///
/// Takes the key code as well as the scan code, and not as a fallback: a scan
/// code of zero means the event never came from a keyboard -- `adb shell input
/// keyevent` and the emulator both produce it -- and then the key code is the
/// only thing that tells two keys apart.
uint64_t PhysicalKeyForAndroidKey(uint32_t scan_code, uint32_t key_code);

//------------------------------------------------------------------------------
/// What the key means, from Android's key code alone.
///
/// The key code already carries the layout: Android resolves it before the
/// event is delivered, which is why -- unlike Windows -- no scan code is
/// needed here to disambiguate.
uint64_t LogicalKeyForAndroidKeyCode(uint32_t key_code);

}  // namespace flutter

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_ANDROID_H_
