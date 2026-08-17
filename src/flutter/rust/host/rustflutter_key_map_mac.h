// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_MAC_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_MAC_H_

#include <stdint.h>

namespace flutter {

// Flutter identifies a key twice over, and both are needed: the physical key is
// where the finger went, the logical key is what it meant. A shortcut is
// logical -- Cmd+Z is Cmd+Z wherever Z sits on a French keyboard -- while "is
// this key still down" is physical, because the layout can change between the
// press and the release and the release must still cancel the press.

// The high bits of a logical key value say which namespace it is drawn from.
// A key with no entry in the tables keeps its macOS identity rather than
// colliding with a real Unicode codepoint.
constexpr uint64_t kValueMask = 0x000ffffffff;
constexpr uint64_t kUnicodePlane = 0x00000000000;
constexpr uint64_t kMacosPlane = 0x01400000000;

//------------------------------------------------------------------------------
/// The USB HID usage code for a key, from its macOS virtual key code.
///
/// macOS needs no equivalent of Windows' extended-key flag: its key codes are
/// already positions, and the arrow cluster and the numeric keypad have codes
/// of their own.
uint64_t PhysicalKeyForKeyCode(uint16_t key_code);

//------------------------------------------------------------------------------
/// What the key means under the layout in force.
///
/// `character` is the first UTF-32 code point of `charactersIgnoringModifiers`,
/// or zero when the event produced none. It is what settles a character key: a
/// key code says where the finger went, and only the layout says what that
/// position currently produces.
uint64_t LogicalKeyForKeyCode(uint16_t key_code, uint32_t character);

//------------------------------------------------------------------------------
/// The modifier bit a key code toggles, or zero if it is not a modifier.
///
/// macOS reports a modifier as a change of state rather than as a press and a
/// release, so telling the two apart means asking which bit this key owns and
/// looking at whether it is now set. Left and right are separate bits, which
/// `NSEvent.modifierFlags` carries and `NSEventModifierFlags` does not name.
uint64_t ModifierFlagForKeyCode(uint16_t key_code);

}  // namespace flutter

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_MAC_H_
