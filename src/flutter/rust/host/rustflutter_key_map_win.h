// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_WIN_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_WIN_H_

#include <stdint.h>

namespace flutter {

// Flutter identifies a key twice over, and both are needed: the physical key is
// where the finger went, the logical key is what it meant. A shortcut is
// logical -- Ctrl+Z is Ctrl+Z wherever Z sits on a French keyboard -- while
// "is this key still down" is physical, because the layout can change between
// the press and the release and the release must still cancel the press.

// The high bits of a logical key value say which namespace it is drawn from.
// A key with no entry in the tables keeps its Windows identity rather than
// colliding with a real Unicode codepoint.
constexpr uint64_t kValueMask = 0x000ffffffff;
constexpr uint64_t kWindowsPlane = 0x01600000000;

//------------------------------------------------------------------------------
/// The USB HID usage code for a key, from its Win32 scan code.
///
/// `extended` is bit 24 of a key message's lparam, which is what separates the
/// keys that were added to the right of the original PC layout -- the arrow
/// cluster from the numeric keypad, right Alt from left.
uint64_t PhysicalKeyForScanCode(uint16_t scan_code, bool extended);

//------------------------------------------------------------------------------
/// What the key means under the layout in force, from its virtual key code.
///
/// The scan code is needed as well, and not as a fallback: `VK_SHIFT` alone
/// cannot say which Shift, and the numeric keypad reports the same virtual keys
/// as the digit row. Position settles those.
uint64_t LogicalKeyForVirtualKey(uint32_t virtual_key,
                                 uint16_t scan_code,
                                 bool extended);

}  // namespace flutter

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_WIN_H_
