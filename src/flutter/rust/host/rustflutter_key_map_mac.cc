// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// macOS virtual key codes to Flutter's physical and logical key values.
//
// GENERATED -- see rust/host/tools/gen_key_map_mac.py. The data is upstream's
// shell/platform/darwin/macos/framework/Source/KeyCodeMap.g.mm, which
// gen_keycodes.dart generates from the same source as the framework's key
// definitions. It is reshaped here into sorted arrays: the upstream file
// defines them as NSDictionary literals built at static-initialisation time and
// read by boxing an integer, and a binary search over constant data does the
// same job without Foundation.

#include "flutter/rust/host/rustflutter_key_map_mac.h"

#include <algorithm>

namespace flutter {
namespace {

struct KeyMapping {
  uint32_t from;
  uint64_t to;
};

constexpr KeyMapping kKeyCodeToPhysical[] = {
    {0x00000000, 0x00000070004},  // keyA
    {0x00000001, 0x00000070016},  // keyS
    {0x00000002, 0x00000070007},  // keyD
    {0x00000003, 0x00000070009},  // keyF
    {0x00000004, 0x0000007000b},  // keyH
    {0x00000005, 0x0000007000a},  // keyG
    {0x00000006, 0x0000007001d},  // keyZ
    {0x00000007, 0x0000007001b},  // keyX
    {0x00000008, 0x00000070006},  // keyC
    {0x00000009, 0x00000070019},  // keyV
    {0x0000000a, 0x00000070064},  // intlBackslash
    {0x0000000b, 0x00000070005},  // keyB
    {0x0000000c, 0x00000070014},  // keyQ
    {0x0000000d, 0x0000007001a},  // keyW
    {0x0000000e, 0x00000070008},  // keyE
    {0x0000000f, 0x00000070015},  // keyR
    {0x00000010, 0x0000007001c},  // keyY
    {0x00000011, 0x00000070017},  // keyT
    {0x00000012, 0x0000007001e},  // digit1
    {0x00000013, 0x0000007001f},  // digit2
    {0x00000014, 0x00000070020},  // digit3
    {0x00000015, 0x00000070021},  // digit4
    {0x00000016, 0x00000070023},  // digit6
    {0x00000017, 0x00000070022},  // digit5
    {0x00000018, 0x0000007002e},  // equal
    {0x00000019, 0x00000070026},  // digit9
    {0x0000001a, 0x00000070024},  // digit7
    {0x0000001b, 0x0000007002d},  // minus
    {0x0000001c, 0x00000070025},  // digit8
    {0x0000001d, 0x00000070027},  // digit0
    {0x0000001e, 0x00000070030},  // bracketRight
    {0x0000001f, 0x00000070012},  // keyO
    {0x00000020, 0x00000070018},  // keyU
    {0x00000021, 0x0000007002f},  // bracketLeft
    {0x00000022, 0x0000007000c},  // keyI
    {0x00000023, 0x00000070013},  // keyP
    {0x00000024, 0x00000070028},  // enter
    {0x00000025, 0x0000007000f},  // keyL
    {0x00000026, 0x0000007000d},  // keyJ
    {0x00000027, 0x00000070034},  // quote
    {0x00000028, 0x0000007000e},  // keyK
    {0x00000029, 0x00000070033},  // semicolon
    {0x0000002a, 0x00000070031},  // backslash
    {0x0000002b, 0x00000070036},  // comma
    {0x0000002c, 0x00000070038},  // slash
    {0x0000002d, 0x00000070011},  // keyN
    {0x0000002e, 0x00000070010},  // keyM
    {0x0000002f, 0x00000070037},  // period
    {0x00000030, 0x0000007002b},  // tab
    {0x00000031, 0x0000007002c},  // space
    {0x00000032, 0x00000070035},  // backquote
    {0x00000033, 0x0000007002a},  // backspace
    {0x00000035, 0x00000070029},  // escape
    {0x00000036, 0x000000700e7},  // metaRight
    {0x00000037, 0x000000700e3},  // metaLeft
    {0x00000038, 0x000000700e1},  // shiftLeft
    {0x00000039, 0x00000070039},  // capsLock
    {0x0000003a, 0x000000700e2},  // altLeft
    {0x0000003b, 0x000000700e0},  // controlLeft
    {0x0000003c, 0x000000700e5},  // shiftRight
    {0x0000003d, 0x000000700e6},  // altRight
    {0x0000003e, 0x000000700e4},  // controlRight
    {0x0000003f, 0x00000000012},  // fn
    {0x00000040, 0x0000007006c},  // f17
    {0x00000041, 0x00000070063},  // numpadDecimal
    {0x00000043, 0x00000070055},  // numpadMultiply
    {0x00000045, 0x00000070057},  // numpadAdd
    {0x00000047, 0x00000070053},  // numLock
    {0x00000048, 0x00000070080},  // audioVolumeUp
    {0x00000049, 0x00000070081},  // audioVolumeDown
    {0x0000004a, 0x0000007007f},  // audioVolumeMute
    {0x0000004b, 0x00000070054},  // numpadDivide
    {0x0000004c, 0x00000070058},  // numpadEnter
    {0x0000004e, 0x00000070056},  // numpadSubtract
    {0x0000004f, 0x0000007006d},  // f18
    {0x00000050, 0x0000007006e},  // f19
    {0x00000051, 0x00000070067},  // numpadEqual
    {0x00000052, 0x00000070062},  // numpad0
    {0x00000053, 0x00000070059},  // numpad1
    {0x00000054, 0x0000007005a},  // numpad2
    {0x00000055, 0x0000007005b},  // numpad3
    {0x00000056, 0x0000007005c},  // numpad4
    {0x00000057, 0x0000007005d},  // numpad5
    {0x00000058, 0x0000007005e},  // numpad6
    {0x00000059, 0x0000007005f},  // numpad7
    {0x0000005a, 0x0000007006f},  // f20
    {0x0000005b, 0x00000070060},  // numpad8
    {0x0000005c, 0x00000070061},  // numpad9
    {0x0000005d, 0x00000070089},  // intlYen
    {0x0000005e, 0x00000070087},  // intlRo
    {0x0000005f, 0x00000070085},  // numpadComma
    {0x00000060, 0x0000007003e},  // f5
    {0x00000061, 0x0000007003f},  // f6
    {0x00000062, 0x00000070040},  // f7
    {0x00000063, 0x0000007003c},  // f3
    {0x00000064, 0x00000070041},  // f8
    {0x00000065, 0x00000070042},  // f9
    {0x00000066, 0x00000070091},  // lang2
    {0x00000067, 0x00000070044},  // f11
    {0x00000068, 0x00000070090},  // lang1
    {0x00000069, 0x00000070068},  // f13
    {0x0000006a, 0x0000007006b},  // f16
    {0x0000006b, 0x00000070069},  // f14
    {0x0000006d, 0x00000070043},  // f10
    {0x0000006e, 0x00000070065},  // contextMenu
    {0x0000006f, 0x00000070045},  // f12
    {0x00000071, 0x0000007006a},  // f15
    {0x00000072, 0x00000070049},  // insert
    {0x00000073, 0x0000007004a},  // home
    {0x00000074, 0x0000007004b},  // pageUp
    {0x00000075, 0x0000007004c},  // delete
    {0x00000076, 0x0000007003d},  // f4
    {0x00000077, 0x0000007004d},  // end
    {0x00000078, 0x0000007003b},  // f2
    {0x00000079, 0x0000007004e},  // pageDown
    {0x0000007a, 0x0000007003a},  // f1
    {0x0000007b, 0x00000070050},  // arrowLeft
    {0x0000007c, 0x0000007004f},  // arrowRight
    {0x0000007d, 0x00000070051},  // arrowDown
    {0x0000007e, 0x00000070052},  // arrowUp
};

constexpr KeyMapping kKeyCodeToLogical[] = {
    {0x00000024, 0x0010000000d},  // Enter -> enter
    {0x00000030, 0x00100000009},  // Tab -> tab
    {0x00000033, 0x00100000008},  // Backspace -> backspace
    {0x00000035, 0x0010000001b},  // Escape -> escape
    {0x00000036, 0x00200000107},  // MetaRight -> metaRight
    {0x00000037, 0x00200000106},  // MetaLeft -> metaLeft
    {0x00000038, 0x00200000102},  // ShiftLeft -> shiftLeft
    {0x00000039, 0x00100000104},  // CapsLock -> capsLock
    {0x0000003a, 0x00200000104},  // AltLeft -> altLeft
    {0x0000003b, 0x00200000100},  // ControlLeft -> controlLeft
    {0x0000003c, 0x00200000103},  // ShiftRight -> shiftRight
    {0x0000003d, 0x00200000105},  // AltRight -> altRight
    {0x0000003e, 0x00200000101},  // ControlRight -> controlRight
    {0x0000003f, 0x00100000106},  // Fn -> fn
    {0x00000040, 0x00100000811},  // F17 -> f17
    {0x00000041, 0x0020000022e},  // NumpadDecimal -> numpadDecimal
    {0x00000043, 0x0020000022a},  // NumpadMultiply -> numpadMultiply
    {0x00000045, 0x0020000022b},  // NumpadAdd -> numpadAdd
    {0x00000047, 0x0010000010a},  // NumLock -> numLock
    {0x00000048, 0x00100000a10},  // AudioVolumeUp -> audioVolumeUp
    {0x00000049, 0x00100000a0f},  // AudioVolumeDown -> audioVolumeDown
    {0x0000004a, 0x00100000a11},  // AudioVolumeMute -> audioVolumeMute
    {0x0000004b, 0x0020000022f},  // NumpadDivide -> numpadDivide
    {0x0000004c, 0x0020000020d},  // NumpadEnter -> numpadEnter
    {0x0000004e, 0x0020000022d},  // NumpadSubtract -> numpadSubtract
    {0x0000004f, 0x00100000812},  // F18 -> f18
    {0x00000050, 0x00100000813},  // F19 -> f19
    {0x00000051, 0x0020000023d},  // NumpadEqual -> numpadEqual
    {0x00000052, 0x00200000230},  // Numpad0 -> numpad0
    {0x00000053, 0x00200000231},  // Numpad1 -> numpad1
    {0x00000054, 0x00200000232},  // Numpad2 -> numpad2
    {0x00000055, 0x00200000233},  // Numpad3 -> numpad3
    {0x00000056, 0x00200000234},  // Numpad4 -> numpad4
    {0x00000057, 0x00200000235},  // Numpad5 -> numpad5
    {0x00000058, 0x00200000236},  // Numpad6 -> numpad6
    {0x00000059, 0x00200000237},  // Numpad7 -> numpad7
    {0x0000005a, 0x00100000814},  // F20 -> f20
    {0x0000005b, 0x00200000238},  // Numpad8 -> numpad8
    {0x0000005c, 0x00200000239},  // Numpad9 -> numpad9
    {0x0000005d, 0x00200000022},  // IntlYen -> intlYen
    {0x0000005e, 0x00200000021},  // IntlRo -> intlRo
    {0x0000005f, 0x0020000022c},  // NumpadComma -> numpadComma
    {0x00000060, 0x00100000805},  // F5 -> f5
    {0x00000061, 0x00100000806},  // F6 -> f6
    {0x00000062, 0x00100000807},  // F7 -> f7
    {0x00000063, 0x00100000803},  // F3 -> f3
    {0x00000064, 0x00100000808},  // F8 -> f8
    {0x00000065, 0x00100000809},  // F9 -> f9
    {0x00000066, 0x00200000011},  // Lang2 -> lang2
    {0x00000067, 0x0010000080b},  // F11 -> f11
    {0x00000068, 0x00200000010},  // Lang1 -> lang1
    {0x00000069, 0x0010000080d},  // F13 -> f13
    {0x0000006a, 0x00100000810},  // F16 -> f16
    {0x0000006b, 0x0010000080e},  // F14 -> f14
    {0x0000006d, 0x0010000080a},  // F10 -> f10
    {0x0000006e, 0x00100000505},  // ContextMenu -> contextMenu
    {0x0000006f, 0x0010000080c},  // F12 -> f12
    {0x00000071, 0x0010000080f},  // F15 -> f15
    {0x00000072, 0x00100000407},  // Insert -> insert
    {0x00000073, 0x00100000306},  // Home -> home
    {0x00000074, 0x00100000308},  // PageUp -> pageUp
    {0x00000075, 0x0010000007f},  // Delete -> delete
    {0x00000076, 0x00100000804},  // F4 -> f4
    {0x00000077, 0x00100000305},  // End -> end
    {0x00000078, 0x00100000802},  // F2 -> f2
    {0x00000079, 0x00100000307},  // PageDown -> pageDown
    {0x0000007a, 0x00100000801},  // F1 -> f1
    {0x0000007b, 0x00100000302},  // ArrowLeft -> arrowLeft
    {0x0000007c, 0x00100000303},  // ArrowRight -> arrowRight
    {0x0000007d, 0x00100000301},  // ArrowDown -> arrowDown
    {0x0000007e, 0x00100000304},  // ArrowUp -> arrowUp
};

constexpr KeyMapping kKeyCodeToModifierFlag[] = {
    {0x00000036, 0x0010},  // MetaRight
    {0x00000037, 0x0008},  // MetaLeft
    {0x00000038, 0x0002},  // ShiftLeft
    {0x0000003a, 0x0020},  // AltLeft
    {0x0000003b, 0x0001},  // ControlLeft
    {0x0000003c, 0x0004},  // ShiftRight
    {0x0000003d, 0x0040},  // AltRight
    {0x0000003e, 0x2000},  // ControlRight
};

/// Binary search over a table sorted by `from`. Returns `fallback` when absent.
template <size_t N>
uint64_t Lookup(const KeyMapping (&table)[N],
                uint32_t from,
                uint64_t fallback) {
  const KeyMapping* end = table + N;
  const KeyMapping* found = std::lower_bound(
      table, end, from, [](const KeyMapping& entry, uint32_t value) {
        return entry.from < value;
      });
  return (found != end && found->from == from) ? found->to : fallback;
}

}  // namespace

uint64_t PhysicalKeyForKeyCode(uint16_t key_code) {
  // A key with no entry keeps its macOS identity rather than colliding with a
  // real HID usage. That is a stable identity even though nothing can name it,
  // which is what physical keys are for.
  return Lookup(kKeyCodeToPhysical, key_code,
                (key_code & kValueMask) | kMacosPlane);
}

uint64_t LogicalKeyForKeyCode(uint16_t key_code, uint32_t character) {
  // The named keys first: enter, the arrows, the function row and both of every
  // modifier. Their meaning does not depend on the layout.
  const uint64_t named = Lookup(kKeyCodeToLogical, key_code, 0);
  if (named != 0) {
    return named;
  }

  // Everything else is a character key, and its logical value is the character
  // it produces with no modifiers applied, lower-cased, in the Unicode plane.
  // `A` and `a` are the same key and must be the same logical value -- which is
  // why the caller passes `charactersIgnoringModifiers` rather than
  // `characters`.
  if (character != 0) {
    uint32_t lowered = character;
    if (lowered >= 'A' && lowered <= 'Z') {
      lowered += 'a' - 'A';
    }
    return static_cast<uint64_t>(lowered) | kUnicodePlane;
  }

  return (key_code & kValueMask) | kMacosPlane;
}

uint64_t ModifierFlagForKeyCode(uint16_t key_code) {
  return Lookup(kKeyCodeToModifierFlag, key_code, 0);
}

}  // namespace flutter
