// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Scan codes and virtual keys to Flutter's physical and logical key values.
//
// GENERATED -- see rust/host/tools/gen_key_map.py. The data is upstream's
// shell/platform/windows/flutter_key_map.g.cc, which gen_keycodes.dart
// generates from the same source as the framework's key definitions. It is
// reshaped here into sorted arrays: the upstream file defines them as static
// std::map members of KeyboardKeyEmbedderHandler, a class this host does not
// link, and three maps built at static-initialisation time is a cost with
// nothing to show for it when a binary search over constant data does.

#include "flutter/rust/host/rustflutter_key_map_win.h"

#include <algorithm>

namespace flutter {
namespace {

struct KeyMapping {
  uint32_t from;
  uint64_t to;
};

constexpr KeyMapping kScanCodeToPhysical[] = {
    {0x00000001, 0x00000070029},  // escape
    {0x00000002, 0x0000007001e},  // digit1
    {0x00000003, 0x0000007001f},  // digit2
    {0x00000004, 0x00000070020},  // digit3
    {0x00000005, 0x00000070021},  // digit4
    {0x00000006, 0x00000070022},  // digit5
    {0x00000007, 0x00000070023},  // digit6
    {0x00000008, 0x00000070024},  // digit7
    {0x00000009, 0x00000070025},  // digit8
    {0x0000000a, 0x00000070026},  // digit9
    {0x0000000b, 0x00000070027},  // digit0
    {0x0000000c, 0x0000007002d},  // minus
    {0x0000000d, 0x0000007002e},  // equal
    {0x0000000e, 0x0000007002a},  // backspace
    {0x0000000f, 0x0000007002b},  // tab
    {0x00000010, 0x00000070014},  // keyQ
    {0x00000011, 0x0000007001a},  // keyW
    {0x00000012, 0x00000070008},  // keyE
    {0x00000013, 0x00000070015},  // keyR
    {0x00000014, 0x00000070017},  // keyT
    {0x00000015, 0x0000007001c},  // keyY
    {0x00000016, 0x00000070018},  // keyU
    {0x00000017, 0x0000007000c},  // keyI
    {0x00000018, 0x00000070012},  // keyO
    {0x00000019, 0x00000070013},  // keyP
    {0x0000001a, 0x0000007002f},  // bracketLeft
    {0x0000001b, 0x00000070030},  // bracketRight
    {0x0000001c, 0x00000070028},  // enter
    {0x0000001d, 0x000000700e0},  // controlLeft
    {0x0000001e, 0x00000070004},  // keyA
    {0x0000001f, 0x00000070016},  // keyS
    {0x00000020, 0x00000070007},  // keyD
    {0x00000021, 0x00000070009},  // keyF
    {0x00000022, 0x0000007000a},  // keyG
    {0x00000023, 0x0000007000b},  // keyH
    {0x00000024, 0x0000007000d},  // keyJ
    {0x00000025, 0x0000007000e},  // keyK
    {0x00000026, 0x0000007000f},  // keyL
    {0x00000027, 0x00000070033},  // semicolon
    {0x00000028, 0x00000070034},  // quote
    {0x00000029, 0x00000070035},  // backquote
    {0x0000002a, 0x000000700e1},  // shiftLeft
    {0x0000002b, 0x00000070031},  // backslash
    {0x0000002c, 0x0000007001d},  // keyZ
    {0x0000002d, 0x0000007001b},  // keyX
    {0x0000002e, 0x00000070006},  // keyC
    {0x0000002f, 0x00000070019},  // keyV
    {0x00000030, 0x00000070005},  // keyB
    {0x00000031, 0x00000070011},  // keyN
    {0x00000032, 0x00000070010},  // keyM
    {0x00000033, 0x00000070036},  // comma
    {0x00000034, 0x00000070037},  // period
    {0x00000035, 0x00000070038},  // slash
    {0x00000036, 0x000000700e5},  // shiftRight
    {0x00000037, 0x00000070055},  // numpadMultiply
    {0x00000038, 0x000000700e2},  // altLeft
    {0x00000039, 0x0000007002c},  // space
    {0x0000003a, 0x00000070039},  // capsLock
    {0x0000003b, 0x0000007003a},  // f1
    {0x0000003c, 0x0000007003b},  // f2
    {0x0000003d, 0x0000007003c},  // f3
    {0x0000003e, 0x0000007003d},  // f4
    {0x0000003f, 0x0000007003e},  // f5
    {0x00000040, 0x0000007003f},  // f6
    {0x00000041, 0x00000070040},  // f7
    {0x00000042, 0x00000070041},  // f8
    {0x00000043, 0x00000070042},  // f9
    {0x00000044, 0x00000070043},  // f10
    {0x00000045, 0x00000070048},  // pause
    {0x00000046, 0x00000070047},  // scrollLock
    {0x00000047, 0x0000007005f},  // numpad7
    {0x00000048, 0x00000070060},  // numpad8
    {0x00000049, 0x00000070061},  // numpad9
    {0x0000004a, 0x00000070056},  // numpadSubtract
    {0x0000004b, 0x0000007005c},  // numpad4
    {0x0000004c, 0x0000007005d},  // numpad5
    {0x0000004d, 0x0000007005e},  // numpad6
    {0x0000004e, 0x00000070057},  // numpadAdd
    {0x0000004f, 0x00000070059},  // numpad1
    {0x00000050, 0x0000007005a},  // numpad2
    {0x00000051, 0x0000007005b},  // numpad3
    {0x00000052, 0x00000070062},  // numpad0
    {0x00000053, 0x00000070063},  // numpadDecimal
    {0x00000056, 0x00000070064},  // intlBackslash
    {0x00000057, 0x00000070044},  // f11
    {0x00000058, 0x00000070045},  // f12
    {0x00000059, 0x00000070067},  // numpadEqual
    {0x00000064, 0x00000070068},  // f13
    {0x00000065, 0x00000070069},  // f14
    {0x00000066, 0x0000007006a},  // f15
    {0x00000067, 0x0000007006b},  // f16
    {0x00000068, 0x0000007006c},  // f17
    {0x00000069, 0x0000007006d},  // f18
    {0x0000006a, 0x0000007006e},  // f19
    {0x0000006b, 0x0000007006f},  // f20
    {0x0000006c, 0x00000070070},  // f21
    {0x0000006d, 0x00000070071},  // f22
    {0x0000006e, 0x00000070072},  // f23
    {0x00000070, 0x00000070088},  // kanaMode
    {0x00000071, 0x00000070091},  // lang2
    {0x00000072, 0x00000070090},  // lang1
    {0x00000073, 0x00000070087},  // intlRo
    {0x00000076, 0x00000070073},  // f24
    {0x00000077, 0x00000070093},  // lang4
    {0x00000078, 0x00000070092},  // lang3
    {0x00000079, 0x0000007008a},  // convert
    {0x0000007b, 0x0000007008b},  // nonConvert
    {0x0000007d, 0x00000070089},  // intlYen
    {0x0000007e, 0x00000070085},  // numpadComma
    {0x000000fc, 0x00000070002},  // usbPostFail
    {0x000000ff, 0x00000070001},  // usbErrorRollOver
    {0x0000e008, 0x0000007007a},  // undo
    {0x0000e00a, 0x0000007007d},  // paste
    {0x0000e010, 0x000000c00b6},  // mediaTrackPrevious
    {0x0000e017, 0x0000007007b},  // cut
    {0x0000e018, 0x0000007007c},  // copy
    {0x0000e019, 0x000000c00b5},  // mediaTrackNext
    {0x0000e01c, 0x00000070058},  // numpadEnter
    {0x0000e01d, 0x000000700e4},  // controlRight
    {0x0000e020, 0x0000007007f},  // audioVolumeMute
    {0x0000e021, 0x000000c0192},  // launchApp2
    {0x0000e022, 0x000000c00cd},  // mediaPlayPause
    {0x0000e024, 0x000000c00b7},  // mediaStop
    {0x0000e02c, 0x000000c00b8},  // eject
    {0x0000e02e, 0x00000070081},  // audioVolumeDown
    {0x0000e030, 0x00000070080},  // audioVolumeUp
    {0x0000e032, 0x000000c0223},  // browserHome
    {0x0000e035, 0x00000070054},  // numpadDivide
    {0x0000e037, 0x00000070046},  // printScreen
    {0x0000e038, 0x000000700e6},  // altRight
    {0x0000e03b, 0x00000070075},  // help
    {0x0000e045, 0x00000070053},  // numLock
    {0x0000e047, 0x0000007004a},  // home
    {0x0000e048, 0x00000070052},  // arrowUp
    {0x0000e049, 0x0000007004b},  // pageUp
    {0x0000e04b, 0x00000070050},  // arrowLeft
    {0x0000e04d, 0x0000007004f},  // arrowRight
    {0x0000e04f, 0x0000007004d},  // end
    {0x0000e050, 0x00000070051},  // arrowDown
    {0x0000e051, 0x0000007004e},  // pageDown
    {0x0000e052, 0x00000070049},  // insert
    {0x0000e053, 0x0000007004c},  // delete
    {0x0000e05b, 0x000000700e3},  // metaLeft
    {0x0000e05c, 0x000000700e7},  // metaRight
    {0x0000e05d, 0x00000070065},  // contextMenu
    {0x0000e05e, 0x00000070066},  // power
    {0x0000e05f, 0x00000010082},  // sleep
    {0x0000e063, 0x00000010083},  // wakeUp
    {0x0000e065, 0x000000c0221},  // browserSearch
    {0x0000e066, 0x000000c022a},  // browserFavorites
    {0x0000e067, 0x000000c0227},  // browserRefresh
    {0x0000e068, 0x000000c0226},  // browserStop
    {0x0000e069, 0x000000c0225},  // browserForward
    {0x0000e06a, 0x000000c0224},  // browserBack
    {0x0000e06b, 0x000000c0194},  // launchApp1
    {0x0000e06c, 0x000000c018a},  // launchMail
    {0x0000e06d, 0x000000c0183},  // mediaSelect
};

constexpr KeyMapping kVirtualKeyToLogical[] = {
    {0x00000003, 0x00100000504},  // CANCEL -> cancel
    {0x00000008, 0x00100000008},  // BACK -> backspace
    {0x00000009, 0x00100000009},  // TAB -> tab
    {0x0000000c, 0x00100000401},  // CLEAR -> clear
    {0x0000000d, 0x0010000000d},  // RETURN -> enter
    {0x00000010, 0x00200000102},  // SHIFT -> shiftLeft
    {0x00000011, 0x00200000100},  // CONTROL -> controlLeft
    {0x00000013, 0x00100000509},  // PAUSE -> pause
    {0x00000014, 0x00100000104},  // CAPITAL -> capsLock
    {0x00000015, 0x00200000010},  // KANA, HANGEUL, HANGUL -> lang1
    {0x00000017, 0x00100000713},  // JUNJA -> junjaMode
    {0x00000018, 0x00100000706},  // FINAL -> finalMode
    {0x00000019, 0x00100000719},  // HANJA, KANJI -> kanjiMode
    {0x0000001b, 0x0010000001b},  // ESCAPE -> escape
    {0x0000001c, 0x00100000705},  // CONVERT -> convert
    {0x0000001e, 0x00100000501},  // ACCEPT -> accept
    {0x0000001f, 0x0010000070b},  // MODECHANGE -> modeChange
    {0x00000020, 0x00000000020},  // SPACE -> space
    {0x00000021, 0x00100000308},  // PRIOR -> pageUp
    {0x00000022, 0x00100000307},  // NEXT -> pageDown
    {0x00000023, 0x00100000305},  // END -> end
    {0x00000024, 0x00100000306},  // HOME -> home
    {0x00000025, 0x00100000302},  // LEFT -> arrowLeft
    {0x00000026, 0x00100000304},  // UP -> arrowUp
    {0x00000027, 0x00100000303},  // RIGHT -> arrowRight
    {0x00000028, 0x00100000301},  // DOWN -> arrowDown
    {0x00000029, 0x0010000050c},  // SELECT -> select
    {0x0000002a, 0x00100000a0c},  // PRINT -> print
    {0x0000002b, 0x00100000506},  // EXECUTE -> execute
    {0x0000002c, 0x00100000608},  // SNAPSHOT -> printScreen
    {0x0000002d, 0x00100000407},  // INSERT -> insert
    {0x0000002e, 0x0010000007f},  // DELETE -> delete
    {0x0000002f, 0x00100000508},  // HELP -> help
    {0x0000005b, 0x00200000106},  // LWIN -> metaLeft
    {0x0000005c, 0x00200000107},  // RWIN -> metaRight
    {0x0000005d, 0x00100000505},  // APPS -> contextMenu
    {0x0000005f, 0x00200000002},  // SLEEP -> sleep
    {0x00000060, 0x00200000230},  // NUMPAD0 -> numpad0
    {0x00000061, 0x00200000231},  // NUMPAD1 -> numpad1
    {0x00000062, 0x00200000232},  // NUMPAD2 -> numpad2
    {0x00000063, 0x00200000233},  // NUMPAD3 -> numpad3
    {0x00000064, 0x00200000234},  // NUMPAD4 -> numpad4
    {0x00000065, 0x00200000235},  // NUMPAD5 -> numpad5
    {0x00000066, 0x00200000236},  // NUMPAD6 -> numpad6
    {0x00000067, 0x00200000237},  // NUMPAD7 -> numpad7
    {0x00000068, 0x00200000238},  // NUMPAD8 -> numpad8
    {0x00000069, 0x00200000239},  // NUMPAD9 -> numpad9
    {0x0000006a, 0x0020000022a},  // MULTIPLY -> numpadMultiply
    {0x0000006b, 0x0020000022b},  // ADD -> numpadAdd
    {0x0000006c, 0x0020000022c},  // SEPARATOR -> numpadComma
    {0x0000006d, 0x0020000022d},  // SUBTRACT -> numpadSubtract
    {0x0000006e, 0x0020000022e},  // DECIMAL -> numpadDecimal
    {0x0000006f, 0x0020000022f},  // DIVIDE -> numpadDivide
    {0x00000070, 0x00100000801},  // F1 -> f1
    {0x00000071, 0x00100000802},  // F2 -> f2
    {0x00000072, 0x00100000803},  // F3 -> f3
    {0x00000073, 0x00100000804},  // F4 -> f4
    {0x00000074, 0x00100000805},  // F5 -> f5
    {0x00000075, 0x00100000806},  // F6 -> f6
    {0x00000076, 0x00100000807},  // F7 -> f7
    {0x00000077, 0x00100000808},  // F8 -> f8
    {0x00000078, 0x00100000809},  // F9 -> f9
    {0x00000079, 0x0010000080a},  // F10 -> f10
    {0x0000007a, 0x0010000080b},  // F11 -> f11
    {0x0000007b, 0x0010000080c},  // F12 -> f12
    {0x0000007c, 0x0010000080d},  // F13 -> f13
    {0x0000007d, 0x0010000080e},  // F14 -> f14
    {0x0000007e, 0x0010000080f},  // F15 -> f15
    {0x0000007f, 0x00100000810},  // F16 -> f16
    {0x00000080, 0x00100000811},  // F17 -> f17
    {0x00000081, 0x00100000812},  // F18 -> f18
    {0x00000082, 0x00100000813},  // F19 -> f19
    {0x00000083, 0x00100000814},  // F20 -> f20
    {0x00000084, 0x00100000815},  // F21 -> f21
    {0x00000085, 0x00100000816},  // F22 -> f22
    {0x00000086, 0x00100000817},  // F23 -> f23
    {0x00000087, 0x00100000818},  // F24 -> f24
    {0x00000090, 0x0010000010a},  // NUMLOCK -> numLock
    {0x00000091, 0x0010000010c},  // SCROLL -> scrollLock
    {0x00000092, 0x0020000023d},  // OEM_NEC_EQUAL -> numpadEqual
    {0x000000a0, 0x00200000102},  // LSHIFT -> shiftLeft
    {0x000000a1, 0x00200000103},  // RSHIFT -> shiftRight
    {0x000000a2, 0x00200000100},  // LCONTROL -> controlLeft
    {0x000000a3, 0x00200000101},  // RCONTROL -> controlRight
    {0x000000a4, 0x00200000104},  // LMENU -> altLeft
    {0x000000a5, 0x00200000105},  // RMENU -> altRight
    {0x000000a6, 0x00100000c01},  // BROWSER_BACK -> browserBack
    {0x000000a7, 0x00100000c03},  // BROWSER_FORWARD -> browserForward
    {0x000000a8, 0x00100000c05},  // BROWSER_REFRESH -> browserRefresh
    {0x000000a9, 0x00100000c07},  // BROWSER_STOP -> browserStop
    {0x000000aa, 0x00100000c06},  // BROWSER_SEARCH -> browserSearch
    {0x000000ab, 0x00100000c02},  // BROWSER_FAVORITES -> browserFavorites
    {0x000000ac, 0x00100000c04},  // BROWSER_HOME -> browserHome
    {0x000000ad, 0x00100000a11},  // VOLUME_MUTE -> audioVolumeMute
    {0x000000ae, 0x00100000a0f},  // VOLUME_DOWN -> audioVolumeDown
    {0x000000af, 0x00100000a10},  // VOLUME_UP -> audioVolumeUp
    {0x000000b2, 0x00100000a07},  // MEDIA_STOP -> mediaStop
    {0x000000b3, 0x00100000a05},  // MEDIA_PLAY_PAUSE -> mediaPlayPause
    {0x000000b4, 0x00100000b03},  // LAUNCH_MAIL -> launchMail
    {0x000000ba, 0x0000000003b},  // OEM_1 -> semicolon
    {0x000000bb, 0x0000000003d},  // OEM_PLUS -> equal
    {0x000000bc, 0x0000000002c},  // OEM_COMMA -> comma
    {0x000000bd, 0x0000000002d},  // OEM_MINUS -> minus
    {0x000000be, 0x0000000002e},  // OEM_PERIOD -> period
    {0x000000bf, 0x0000000002f},  // OEM_2 -> slash
    {0x000000c0, 0x00000000060},  // OEM_3 -> backquote
    {0x000000c3, 0x00200000308},  // GAMEPAD_A -> gameButton8
    {0x000000c4, 0x00200000309},  // GAMEPAD_B -> gameButton9
    {0x000000c5, 0x0020000030a},  // GAMEPAD_X -> gameButton10
    {0x000000c6, 0x0020000030b},  // GAMEPAD_Y -> gameButton11
    {0x000000c7, 0x0020000030c},  // GAMEPAD_RIGHT_SHOULDER -> gameButton12
    {0x000000c8, 0x0020000030d},  // GAMEPAD_LEFT_SHOULDER -> gameButton13
    {0x000000c9, 0x0020000030e},  // GAMEPAD_LEFT_TRIGGER -> gameButton14
    {0x000000ca, 0x0020000030f},  // GAMEPAD_RIGHT_TRIGGER -> gameButton15
    {0x000000cb, 0x00200000310},  // GAMEPAD_DPAD_UP -> gameButton16
    {0x000000db, 0x0000000005b},  // OEM_4 -> bracketLeft
    {0x000000dc, 0x0000000005c},  // OEM_5 -> backslash
    {0x000000dd, 0x0000000005d},  // OEM_6 -> bracketRight
    {0x000000de, 0x00000000022},  // OEM_7 -> quote
    {0x000000f6, 0x00100000503},  // ATTN -> attn
    {0x000000fa, 0x0010000050a},  // PLAY -> play
};

// Keys whose virtual key code is ambiguous or zero, and so have to be told
// apart by where they are on the keyboard rather than by what they report.
constexpr KeyMapping kScanCodeToLogical[] = {
    {0x00000037, 0x0020000022a},  // numpadMultiply -> numpadMultiply
    {0x00000047, 0x00200000237},  // numpad7 -> numpad7
    {0x00000048, 0x00200000238},  // numpad8 -> numpad8
    {0x00000049, 0x00200000239},  // numpad9 -> numpad9
    {0x0000004a, 0x0020000022d},  // numpadSubtract -> numpadSubtract
    {0x0000004b, 0x00200000234},  // numpad4 -> numpad4
    {0x0000004c, 0x00200000235},  // numpad5 -> numpad5
    {0x0000004d, 0x00200000236},  // numpad6 -> numpad6
    {0x0000004e, 0x0020000022b},  // numpadAdd -> numpadAdd
    {0x0000004f, 0x00200000231},  // numpad1 -> numpad1
    {0x00000050, 0x00200000232},  // numpad2 -> numpad2
    {0x00000051, 0x00200000233},  // numpad3 -> numpad3
    {0x00000052, 0x00200000230},  // numpad0 -> numpad0
    {0x00000053, 0x0020000022e},  // numpadDecimal -> numpadDecimal
    {0x00000059, 0x0020000023d},  // numpadEqual -> numpadEqual
    {0x0000e01d, 0x00200000101},  // controlRight -> controlRight
    {0x0000e035, 0x0020000022f},  // numpadDivide -> numpadDivide
    {0x0000e038, 0x00200000105},  // altRight -> altRight
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

/// Chromium's scan code convention, which the tables above are keyed by: the
/// extended-key flag becomes the 0xe000 bit, so PageUp is 0xe049 rather than
/// 0x49 plus a bool.
uint32_t NormalizeScanCode(uint16_t scan_code, bool extended) {
  return (scan_code & 0xff) | (extended ? 0xe000 : 0);
}

}  // namespace

uint64_t PhysicalKeyForScanCode(uint16_t scan_code, bool extended) {
  const uint32_t normalized = NormalizeScanCode(scan_code, extended);
  // A key with no entry keeps its scan code, in the Windows plane. That is a
  // stable identity even though nothing can name it, which is what physical
  // keys are for.
  return Lookup(kScanCodeToPhysical, normalized,
                (scan_code & kValueMask) | kWindowsPlane);
}

uint64_t LogicalKeyForVirtualKey(uint32_t virtual_key,
                                 uint16_t scan_code,
                                 bool extended) {
  const uint32_t normalized = NormalizeScanCode(scan_code, extended);

  // Position first: the numeric keypad and the right-hand modifiers report the
  // same virtual key as their counterparts elsewhere on the keyboard.
  const uint64_t by_position = Lookup(kScanCodeToLogical, normalized, 0);
  if (by_position != 0) {
    return by_position;
  }

  const uint64_t by_key = Lookup(kVirtualKeyToLogical, virtual_key, 0);
  if (by_key != 0) {
    return by_key;
  }

  // Letters and digits are not in the table: their logical value is the
  // character itself, lower-cased, in the Unicode plane. `A` and `a` are the
  // same key and must be the same logical value.
  if (virtual_key >= 0x20 && virtual_key <= 0xff) {
    uint32_t lowered = virtual_key;
    if (lowered >= 'A' && lowered <= 'Z') {
      lowered += 'a' - 'A';
    }
    return lowered;
  }

  return (virtual_key & kValueMask) | kWindowsPlane;
}

}  // namespace flutter
