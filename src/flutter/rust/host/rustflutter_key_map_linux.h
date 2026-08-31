// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_LINUX_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_LINUX_H_

#include <stdint.h>

namespace flutter {

// Flutter identifies a key twice over, and both are needed: the physical key is
// where the finger went, the logical key is what it meant. A shortcut is
// logical -- Ctrl+Z is Ctrl+Z wherever Z sits on a French keyboard -- while
// "is this key still down" is physical, because the layout can change between
// the press and the release and the release must still cancel the press.

// The high bits of a logical key value say which namespace it is drawn from.
// A key with no entry in the tables keeps its GTK identity rather than
// colliding with a real Unicode codepoint.
constexpr uint64_t kValueMask = 0x000ffffffff;
constexpr uint64_t kGtkPlane = 0x01500000000;

//------------------------------------------------------------------------------
/// The USB HID usage code for a key, from its XKB keycode -- which is what a
/// GdkEventKey's `hardware_keycode` is: the evdev scancode plus eight.
uint64_t PhysicalKeyForKeycode(uint16_t keycode);

//------------------------------------------------------------------------------
/// What the key means under the layout in force, from its GDK keyval.
///
/// `code_point` is what the key types with no modifiers held, lower-cased --
/// `gdk_keyval_to_unicode(gdk_keyval_to_lower(keyval))` -- or zero for a key
/// that types nothing. Passed in rather than derived so this file needs no GDK.
uint64_t LogicalKeyForKeyval(uint32_t keyval, uint32_t code_point);

}  // namespace flutter

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_KEY_MAP_LINUX_H_
