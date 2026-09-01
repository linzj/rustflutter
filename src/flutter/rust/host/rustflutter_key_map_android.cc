// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Android key codes and scan codes to Flutter's physical and logical key
// values.
//
// GENERATED -- see rust/host/tools/gen_key_map.py. The data is upstream's
// shell/platform/android/io/flutter/embedding/android/KeyboardMap.java, which
// gen_keycodes.dart generates from the same source as the framework's key
// definitions and as the Windows table next door.
//
// Upstream reads these maps in Java, from KeyEmbedderResponder. This host maps
// in C++ instead, for the same reason the JNI layer is thin everywhere else:
// the Java side of this port is an Activity and nothing more, and a table that
// lives beside the Windows one is a table that gets regenerated with it.
//
// This file is compiled on every platform, not only Android. It names no
// Android header and reaches for nothing -- it is arithmetic over constant
// data -- and building it everywhere is what lets the host test binary check
// it on a desktop, which is the only place these tests are ever run.

#include "flutter/rust/host/rustflutter_key_map_android.h"

#include <algorithm>

namespace flutter {
namespace {

struct KeyMapping {
  uint32_t from;
  uint64_t to;
};

// The low bits a key value can occupy. See the header for why this one is not
// declared there beside kAndroidPlane.
constexpr uint64_t kValueMask = 0x000ffffffff;

// Linux evdev scan codes, which is what `KeyEvent.getScanCode()` returns.
constexpr KeyMapping kAndroidScanCodeToPhysical[] = {
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
    {0x00000045, 0x00000070053},  // numLock
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
    {0x00000056, 0x00000070031},  // backslash
    {0x00000057, 0x00000070044},  // f11
    {0x00000058, 0x00000070045},  // f12
    {0x00000059, 0x00000070087},  // intlRo
    {0x0000005a, 0x00000070092},  // lang3
    {0x0000005b, 0x00000070093},  // lang4
    {0x0000005c, 0x0000007008a},  // convert
    {0x0000005e, 0x0000007008b},  // nonConvert
    {0x0000005f, 0x00000070085},  // numpadComma
    {0x00000060, 0x00000070058},  // numpadEnter
    {0x00000061, 0x000000700e4},  // controlRight
    {0x00000062, 0x00000070054},  // numpadDivide
    {0x00000063, 0x00000070046},  // printScreen
    {0x00000064, 0x000000700e6},  // altRight
    {0x00000066, 0x0000007004a},  // home
    {0x00000067, 0x00000070052},  // arrowUp
    {0x00000068, 0x0000007004b},  // pageUp
    {0x00000069, 0x00000070050},  // arrowLeft
    {0x0000006a, 0x0000007004f},  // arrowRight
    {0x0000006b, 0x0000007004d},  // end
    {0x0000006c, 0x00000070051},  // arrowDown
    {0x0000006d, 0x0000007004e},  // pageDown
    {0x0000006e, 0x00000070049},  // insert
    {0x0000006f, 0x0000007004c},  // delete
    {0x00000071, 0x0000007007f},  // audioVolumeMute
    {0x00000072, 0x00000070081},  // audioVolumeDown
    {0x00000073, 0x00000070080},  // audioVolumeUp
    {0x00000074, 0x00000070066},  // power
    {0x00000075, 0x00000070067},  // numpadEqual
    {0x00000077, 0x00000070048},  // pause
    {0x00000079, 0x00000070085},  // numpadComma
    {0x0000007c, 0x00000070089},  // intlYen
    {0x0000007d, 0x000000700e3},  // metaLeft
    {0x0000007e, 0x000000700e7},  // metaRight
    {0x0000007f, 0x00000070065},  // contextMenu
    {0x00000080, 0x000000c00b7},  // mediaStop
    {0x00000081, 0x00000070079},  // again
    {0x00000082, 0x000000700a3},  // props
    {0x00000083, 0x0000007007a},  // undo
    {0x00000085, 0x0000007007c},  // copy
    {0x00000086, 0x00000070074},  // open
    {0x00000087, 0x0000007007d},  // paste
    {0x00000088, 0x0000007007e},  // find
    {0x00000089, 0x0000007007b},  // cut
    {0x0000008a, 0x00000070075},  // help
    {0x0000008b, 0x00000070065},  // contextMenu
    {0x0000008e, 0x00000010082},  // sleep
    {0x0000008f, 0x00000010083},  // wakeUp
    {0x00000098, 0x00000070066},  // power
    {0x0000009b, 0x000000c018a},  // launchMail
    {0x0000009c, 0x000000c022a},  // browserFavorites
    {0x0000009f, 0x000000c0225},  // browserForward
    {0x000000a0, 0x000000c0203},  // close
    {0x000000a1, 0x000000c00b8},  // eject
    {0x000000a2, 0x000000c00b8},  // eject
    {0x000000a3, 0x000000c00b5},  // mediaTrackNext
    {0x000000a4, 0x000000c00cd},  // mediaPlayPause
    {0x000000a5, 0x000000c00b6},  // mediaTrackPrevious
    {0x000000a6, 0x000000c00b7},  // mediaStop
    {0x000000a7, 0x000000c00b2},  // mediaRecord
    {0x000000a8, 0x000000c00b4},  // mediaRewind
    {0x000000ae, 0x000000c0094},  // exit
    {0x000000b1, 0x0000007004b},  // pageUp
    {0x000000b2, 0x0000007004e},  // pageDown
    {0x000000b3, 0x000000700b6},  // numpadParenLeft
    {0x000000b4, 0x000000700b7},  // numpadParenRight
    {0x000000b6, 0x000000c0279},  // redo
    {0x000000b7, 0x00000070068},  // f13
    {0x000000b8, 0x00000070069},  // f14
    {0x000000b9, 0x0000007006a},  // f15
    {0x000000ba, 0x0000007006b},  // f16
    {0x000000bb, 0x0000007006c},  // f17
    {0x000000bc, 0x0000007006d},  // f18
    {0x000000bd, 0x0000007006e},  // f19
    {0x000000be, 0x0000007006f},  // f20
    {0x000000bf, 0x00000070070},  // f21
    {0x000000c0, 0x00000070071},  // f22
    {0x000000c1, 0x00000070072},  // f23
    {0x000000c2, 0x00000070073},  // f24
    {0x000000c8, 0x000000c00b0},  // mediaPlay
    {0x000000c9, 0x000000c00b1},  // mediaPause
    {0x000000cd, 0x00000000014},  // suspend
    {0x000000ce, 0x000000c0203},  // close
    {0x000000cf, 0x000000c00b0},  // mediaPlay
    {0x000000d0, 0x000000c00b3},  // mediaFastForward
    {0x000000d1, 0x000000c00e5},  // bassBoost
    {0x000000d2, 0x000000c0208},  // print
    {0x000000d7, 0x000000c018a},  // launchMail
    {0x000000d9, 0x000000c0221},  // browserSearch
    {0x000000e0, 0x000000c0070},  // brightnessDown
    {0x000000e1, 0x000000c006f},  // brightnessUp
    {0x00000100, 0x0000005ff01},  // gameButton1
    {0x00000101, 0x0000005ff02},  // gameButton2
    {0x00000102, 0x0000005ff03},  // gameButton3
    {0x00000103, 0x0000005ff04},  // gameButton4
    {0x00000104, 0x0000005ff05},  // gameButton5
    {0x00000105, 0x0000005ff06},  // gameButton6
    {0x00000106, 0x0000005ff07},  // gameButton7
    {0x00000107, 0x0000005ff08},  // gameButton8
    {0x00000108, 0x0000005ff09},  // gameButton9
    {0x00000109, 0x0000005ff0a},  // gameButton10
    {0x0000010a, 0x0000005ff0b},  // gameButton11
    {0x0000010b, 0x0000005ff0c},  // gameButton12
    {0x0000010c, 0x0000005ff0d},  // gameButton13
    {0x0000010d, 0x0000005ff0e},  // gameButton14
    {0x0000010e, 0x0000005ff0f},  // gameButton15
    {0x0000010f, 0x0000005ff10},  // gameButton16
    {0x00000120, 0x0000005ff01},  // gameButton1
    {0x00000121, 0x0000005ff02},  // gameButton2
    {0x00000122, 0x0000005ff03},  // gameButton3
    {0x00000123, 0x0000005ff04},  // gameButton4
    {0x00000124, 0x0000005ff05},  // gameButton5
    {0x00000125, 0x0000005ff06},  // gameButton6
    {0x00000126, 0x0000005ff07},  // gameButton7
    {0x00000127, 0x0000005ff08},  // gameButton8
    {0x00000128, 0x0000005ff09},  // gameButton9
    {0x00000129, 0x0000005ff0a},  // gameButton10
    {0x0000012a, 0x0000005ff0b},  // gameButton11
    {0x0000012b, 0x0000005ff0c},  // gameButton12
    {0x0000012c, 0x0000005ff0d},  // gameButton13
    {0x0000012d, 0x0000005ff0e},  // gameButton14
    {0x0000012e, 0x0000005ff0f},  // gameButton15
    {0x0000012f, 0x0000005ff10},  // gameButton16
    {0x00000130, 0x0000005ff11},  // gameButtonA
    {0x00000131, 0x0000005ff12},  // gameButtonB
    {0x00000132, 0x0000005ff13},  // gameButtonC
    {0x00000133, 0x0000005ff1d},  // gameButtonX
    {0x00000134, 0x0000005ff1e},  // gameButtonY
    {0x00000135, 0x0000005ff1f},  // gameButtonZ
    {0x00000136, 0x0000005ff14},  // gameButtonLeft1
    {0x00000137, 0x0000005ff17},  // gameButtonRight1
    {0x00000138, 0x0000005ff15},  // gameButtonLeft2
    {0x00000139, 0x0000005ff18},  // gameButtonRight2
    {0x0000013a, 0x0000005ff19},  // gameButtonSelect
    {0x0000013b, 0x0000005ff1a},  // gameButtonStart
    {0x0000013c, 0x0000005ff16},  // gameButtonMode
    {0x0000013d, 0x0000005ff1b},  // gameButtonThumbLeft
    {0x0000013e, 0x0000005ff1c},  // gameButtonThumbRight
    {0x00000161, 0x00000070077},  // select
    {0x00000166, 0x000000c0060},  // info
    {0x00000172, 0x000000c0061},  // closedCaptionToggle
    {0x0000018d, 0x000000c018e},  // launchCalendar
    {0x00000192, 0x000000c009c},  // channelUp
    {0x00000193, 0x000000c009d},  // channelDown
    {0x00000195, 0x000000c0083},  // mediaLast
    {0x0000019b, 0x00000070048},  // pause
    {0x000001ad, 0x000000c018d},  // launchContacts
    {0x000001d0, 0x00000000012},  // fn
    {0x00000247, 0x000000c01cb},  // launchAssistant
};

// Android's own key codes, which is what `KeyEvent.getKeyCode()` returns.
constexpr KeyMapping kAndroidKeyCodeToLogical[] = {
    {0x00000003, 0x00100001006},  // goHome
    {0x00000004, 0x00100001005},  // goBack
    {0x00000005, 0x00100001002},  // call
    {0x00000006, 0x00100001004},  // endCall
    {0x00000007, 0x00000000030},  // digit0
    {0x00000008, 0x00000000031},  // digit1
    {0x00000009, 0x00000000032},  // digit2
    {0x0000000a, 0x00000000033},  // digit3
    {0x0000000b, 0x00000000034},  // digit4
    {0x0000000c, 0x00000000035},  // digit5
    {0x0000000d, 0x00000000036},  // digit6
    {0x0000000e, 0x00000000037},  // digit7
    {0x0000000f, 0x00000000038},  // digit8
    {0x00000010, 0x00000000039},  // digit9
    {0x00000011, 0x0000000002a},  // asterisk
    {0x00000012, 0x00000000023},  // numberSign
    {0x00000013, 0x00100000304},  // arrowUp
    {0x00000014, 0x00100000301},  // arrowDown
    {0x00000015, 0x00100000302},  // arrowLeft
    {0x00000016, 0x00100000303},  // arrowRight
    {0x00000017, 0x0010000050c},  // select
    {0x00000018, 0x00100000a10},  // audioVolumeUp
    {0x00000019, 0x00100000a0f},  // audioVolumeDown
    {0x0000001a, 0x00100000606},  // power
    {0x0000001b, 0x00100000603},  // camera
    {0x0000001c, 0x00100000401},  // clear
    {0x0000001d, 0x00000000061},  // keyA
    {0x0000001e, 0x00000000062},  // keyB
    {0x0000001f, 0x00000000063},  // keyC
    {0x00000020, 0x00000000064},  // keyD
    {0x00000021, 0x00000000065},  // keyE
    {0x00000022, 0x00000000066},  // keyF
    {0x00000023, 0x00000000067},  // keyG
    {0x00000024, 0x00000000068},  // keyH
    {0x00000025, 0x00000000069},  // keyI
    {0x00000026, 0x0000000006a},  // keyJ
    {0x00000027, 0x0000000006b},  // keyK
    {0x00000028, 0x0000000006c},  // keyL
    {0x00000029, 0x0000000006d},  // keyM
    {0x0000002a, 0x0000000006e},  // keyN
    {0x0000002b, 0x0000000006f},  // keyO
    {0x0000002c, 0x00000000070},  // keyP
    {0x0000002d, 0x00000000071},  // keyQ
    {0x0000002e, 0x00000000072},  // keyR
    {0x0000002f, 0x00000000073},  // keyS
    {0x00000030, 0x00000000074},  // keyT
    {0x00000031, 0x00000000075},  // keyU
    {0x00000032, 0x00000000076},  // keyV
    {0x00000033, 0x00000000077},  // keyW
    {0x00000034, 0x00000000078},  // keyX
    {0x00000035, 0x00000000079},  // keyY
    {0x00000036, 0x0000000007a},  // keyZ
    {0x00000037, 0x0000000002c},  // comma
    {0x00000038, 0x0000000002e},  // period
    {0x00000039, 0x00200000104},  // altLeft
    {0x0000003a, 0x00200000105},  // altRight
    {0x0000003b, 0x00200000102},  // shiftLeft
    {0x0000003c, 0x00200000103},  // shiftRight
    {0x0000003d, 0x00100000009},  // tab
    {0x0000003e, 0x00000000020},  // space
    {0x0000003f, 0x0010000010f},  // symbol
    {0x00000040, 0x00100000b09},  // launchWebBrowser
    {0x00000041, 0x00100000b03},  // launchMail
    {0x00000042, 0x0010000000d},  // enter
    {0x00000043, 0x00100000008},  // backspace
    {0x00000044, 0x00000000060},  // backquote
    {0x00000045, 0x0000000002d},  // minus
    {0x00000046, 0x0000000003d},  // equal
    {0x00000047, 0x0000000005b},  // bracketLeft
    {0x00000048, 0x0000000005d},  // bracketRight
    {0x00000049, 0x0000000005c},  // backslash
    {0x0000004a, 0x0000000003b},  // semicolon
    {0x0000004b, 0x00000000022},  // quote
    {0x0000004c, 0x0000000002f},  // slash
    {0x0000004d, 0x00000000040},  // at
    {0x0000004f, 0x00100001007},  // headsetHook
    {0x00000050, 0x00100001003},  // cameraFocus
    {0x00000051, 0x0000000002b},  // add
    {0x00000052, 0x00100000505},  // contextMenu
    {0x00000053, 0x00100001009},  // notification
    {0x00000054, 0x00100000c06},  // browserSearch
    {0x00000055, 0x00100000a05},  // mediaPlayPause
    {0x00000056, 0x00100000a07},  // mediaStop
    {0x00000057, 0x00100000a08},  // mediaTrackNext
    {0x00000058, 0x00100000a09},  // mediaTrackPrevious
    {0x00000059, 0x00100000d31},  // mediaRewind
    {0x0000005a, 0x00100000d2c},  // mediaFastForward
    {0x0000005b, 0x00100000e09},  // microphoneVolumeMute
    {0x0000005c, 0x00100000308},  // pageUp
    {0x0000005d, 0x00100000307},  // pageDown
    {0x0000005f, 0x0010000070b},  // modeChange
    {0x00000060, 0x00200000311},  // gameButtonA
    {0x00000061, 0x00200000312},  // gameButtonB
    {0x00000062, 0x00200000313},  // gameButtonC
    {0x00000063, 0x0020000031d},  // gameButtonX
    {0x00000064, 0x0020000031e},  // gameButtonY
    {0x00000065, 0x0020000031f},  // gameButtonZ
    {0x00000066, 0x00200000314},  // gameButtonLeft1
    {0x00000067, 0x00200000317},  // gameButtonRight1
    {0x00000068, 0x00200000315},  // gameButtonLeft2
    {0x00000069, 0x00200000318},  // gameButtonRight2
    {0x0000006a, 0x0020000031b},  // gameButtonThumbLeft
    {0x0000006b, 0x0020000031c},  // gameButtonThumbRight
    {0x0000006c, 0x0020000031a},  // gameButtonStart
    {0x0000006d, 0x00200000319},  // gameButtonSelect
    {0x0000006e, 0x00200000316},  // gameButtonMode
    {0x0000006f, 0x0010000001b},  // escape
    {0x00000070, 0x0010000007f},  // delete
    {0x00000071, 0x00200000100},  // controlLeft
    {0x00000072, 0x00200000101},  // controlRight
    {0x00000073, 0x00100000104},  // capsLock
    {0x00000074, 0x0010000010c},  // scrollLock
    {0x00000075, 0x00200000106},  // metaLeft
    {0x00000076, 0x00200000107},  // metaRight
    {0x00000077, 0x00100000106},  // fn
    {0x00000078, 0x00100000608},  // printScreen
    {0x00000079, 0x00100000509},  // pause
    {0x0000007a, 0x00100000306},  // home
    {0x0000007b, 0x00100000305},  // end
    {0x0000007c, 0x00100000407},  // insert
    {0x0000007d, 0x00100000c03},  // browserForward
    {0x0000007e, 0x00100000d2f},  // mediaPlay
    {0x0000007f, 0x00100000d2e},  // mediaPause
    {0x00000080, 0x00100000a01},  // close
    {0x00000081, 0x00100000604},  // eject
    {0x00000082, 0x00100000d30},  // mediaRecord
    {0x00000083, 0x00100000801},  // f1
    {0x00000084, 0x00100000802},  // f2
    {0x00000085, 0x00100000803},  // f3
    {0x00000086, 0x00100000804},  // f4
    {0x00000087, 0x00100000805},  // f5
    {0x00000088, 0x00100000806},  // f6
    {0x00000089, 0x00100000807},  // f7
    {0x0000008a, 0x00100000808},  // f8
    {0x0000008b, 0x00100000809},  // f9
    {0x0000008c, 0x0010000080a},  // f10
    {0x0000008d, 0x0010000080b},  // f11
    {0x0000008e, 0x0010000080c},  // f12
    {0x0000008f, 0x0010000010a},  // numLock
    {0x00000090, 0x00200000230},  // numpad0
    {0x00000091, 0x00200000231},  // numpad1
    {0x00000092, 0x00200000232},  // numpad2
    {0x00000093, 0x00200000233},  // numpad3
    {0x00000094, 0x00200000234},  // numpad4
    {0x00000095, 0x00200000235},  // numpad5
    {0x00000096, 0x00200000236},  // numpad6
    {0x00000097, 0x00200000237},  // numpad7
    {0x00000098, 0x00200000238},  // numpad8
    {0x00000099, 0x00200000239},  // numpad9
    {0x0000009a, 0x0020000022f},  // numpadDivide
    {0x0000009b, 0x0020000022a},  // numpadMultiply
    {0x0000009c, 0x0020000022d},  // numpadSubtract
    {0x0000009d, 0x0020000022b},  // numpadAdd
    {0x0000009e, 0x0020000022e},  // numpadDecimal
    {0x0000009f, 0x0020000022c},  // numpadComma
    {0x000000a0, 0x0020000020d},  // numpadEnter
    {0x000000a1, 0x0020000023d},  // numpadEqual
    {0x000000a2, 0x00200000228},  // numpadParenLeft
    {0x000000a3, 0x00200000229},  // numpadParenRight
    {0x000000a4, 0x00100000a11},  // audioVolumeMute
    {0x000000a5, 0x00100000d25},  // info
    {0x000000a6, 0x00100000d0b},  // channelUp
    {0x000000a7, 0x00100000d0a},  // channelDown
    {0x000000a8, 0x0010000050d},  // zoomIn
    {0x000000a9, 0x0010000050e},  // zoomOut
    {0x000000aa, 0x00100000d49},  // tv
    {0x000000ac, 0x00100000d22},  // guide
    {0x000000ad, 0x00100000d4f},  // dvr
    {0x000000ae, 0x00100000c02},  // browserFavorites
    {0x000000af, 0x00100000d12},  // closedCaptionToggle
    {0x000000b0, 0x00100000d43},  // settings
    {0x000000b1, 0x00100000d4b},  // tvPower
    {0x000000b2, 0x00100000d4a},  // tvInput
    {0x000000b3, 0x00100000d46},  // stbPower
    {0x000000b4, 0x00100000d45},  // stbInput
    {0x000000b5, 0x00100000d09},  // avrPower
    {0x000000b6, 0x00100000d08},  // avrInput
    {0x000000b7, 0x00100000d0c},  // colorF0Red
    {0x000000b8, 0x00100000d0d},  // colorF1Green
    {0x000000b9, 0x00100000d0e},  // colorF2Yellow
    {0x000000ba, 0x00100000d0f},  // colorF3Blue
    {0x000000bb, 0x00100001001},  // appSwitch
    {0x000000bc, 0x00200000301},  // gameButton1
    {0x000000bd, 0x00200000302},  // gameButton2
    {0x000000be, 0x00200000303},  // gameButton3
    {0x000000bf, 0x00200000304},  // gameButton4
    {0x000000c0, 0x00200000305},  // gameButton5
    {0x000000c1, 0x00200000306},  // gameButton6
    {0x000000c2, 0x00200000307},  // gameButton7
    {0x000000c3, 0x00200000308},  // gameButton8
    {0x000000c4, 0x00200000309},  // gameButton9
    {0x000000c5, 0x0020000030a},  // gameButton10
    {0x000000c6, 0x0020000030b},  // gameButton11
    {0x000000c7, 0x0020000030c},  // gameButton12
    {0x000000c8, 0x0020000030d},  // gameButton13
    {0x000000c9, 0x0020000030e},  // gameButton14
    {0x000000ca, 0x0020000030f},  // gameButton15
    {0x000000cb, 0x00200000310},  // gameButton16
    {0x000000cc, 0x00100000709},  // groupNext
    {0x000000cd, 0x0010000100a},  // mannerMode
    {0x000000ce, 0x00100001101},  // tv3DMode
    {0x000000cf, 0x00100000b0c},  // launchContacts
    {0x000000d0, 0x00100000b02},  // launchCalendar
    {0x000000d1, 0x00100000b05},  // launchMusicPlayer
    {0x000000d3, 0x0010000071d},  // zenkakuHankaku
    {0x000000d4, 0x00100000714},  // eisu
    {0x000000d5, 0x0010000070d},  // nonConvert
    {0x000000d6, 0x00100000705},  // convert
    {0x000000d7, 0x00100000717},  // hiraganaKatakana
    {0x000000d8, 0x00200000022},  // intlYen
    {0x000000d9, 0x00200000021},  // intlRo
    {0x000000da, 0x00100000719},  // kanjiMode
    {0x000000db, 0x00100000b0e},  // launchAssistant
    {0x000000dc, 0x00100000601},  // brightnessDown
    {0x000000dd, 0x00100000602},  // brightnessUp
    {0x000000de, 0x00100000d50},  // mediaAudioTrack
    {0x000000df, 0x00200000002},  // sleep
    {0x000000e0, 0x0010000060b},  // wakeUp
    {0x000000e1, 0x00100000d5a},  // pairing
    {0x000000e2, 0x00100000d55},  // mediaTopMenu
    {0x000000e5, 0x00100000d2d},  // mediaLast
    {0x000000e6, 0x00100001107},  // tvDataService
    {0x000000e8, 0x00100001114},  // tvRadioService
    {0x000000e9, 0x00100000d48},  // teletext
    {0x000000ea, 0x00100001113},  // tvNumberEntry
    {0x000000eb, 0x00100001119},  // tvTerrestrialAnalog
    {0x000000ec, 0x0010000111a},  // tvTerrestrialDigital
    {0x000000ed, 0x00100001115},  // tvSatellite
    {0x000000ee, 0x00100001116},  // tvSatelliteBS
    {0x000000ef, 0x00100001117},  // tvSatelliteCS
    {0x000000f0, 0x00100001118},  // tvSatelliteToggle
    {0x000000f1, 0x00100001112},  // tvNetwork
    {0x000000f2, 0x00100001102},  // tvAntennaCable
    {0x000000f3, 0x0010000110c},  // tvInputHDMI1
    {0x000000f4, 0x0010000110d},  // tvInputHDMI2
    {0x000000f5, 0x0010000110e},  // tvInputHDMI3
    {0x000000f6, 0x0010000110f},  // tvInputHDMI4
    {0x000000f7, 0x0010000110a},  // tvInputComposite1
    {0x000000f8, 0x0010000110b},  // tvInputComposite2
    {0x000000f9, 0x00100001108},  // tvInputComponent1
    {0x000000fa, 0x00100001109},  // tvInputComponent2
    {0x000000fb, 0x00100001110},  // tvInputVGA1
    {0x000000fc, 0x00100001103},  // tvAudioDescription
    {0x000000fd, 0x00100001105},  // tvAudioDescriptionMixUp
    {0x000000fe, 0x00100001104},  // tvAudioDescriptionMixDown
    {0x000000ff, 0x00100000d4e},  // zoomToggle
    {0x00000100, 0x00100001106},  // tvContentsMenu
    {0x00000102, 0x0010000111b},  // tvTimer
    {0x00000103, 0x00100000508},  // help
    {0x00000104, 0x00100000d59},  // navigatePrevious
    {0x00000105, 0x00100000d57},  // navigateNext
    {0x00000106, 0x00100000d56},  // navigateIn
    {0x00000107, 0x00100000d58},  // navigateOut
    {0x00000110, 0x00100000d52},  // mediaSkipForward
    {0x00000111, 0x00100000d51},  // mediaSkipBackward
    {0x00000112, 0x00100000d54},  // mediaStepForward
    {0x00000113, 0x00100000d53},  // mediaStepBackward
    {0x00000115, 0x00100000404},  // cut
    {0x00000116, 0x00100000402},  // copy
    {0x00000117, 0x00100000408},  // paste
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

/// A key nothing can name keeps the number the platform gave it, moved into
/// Android's plane so it cannot collide with a real Unicode codepoint or with
/// another platform's unnamed key.
uint64_t KeyOfAndroidPlane(uint32_t key) {
  return kAndroidPlane | (key & kValueMask);
}

}  // namespace

uint64_t PhysicalKeyForAndroidKey(uint32_t scan_code, uint32_t key_code) {
  // A scan code of zero is not a key at the top of the keyboard: it is a key
  // that never came from a keyboard. `adb shell input keyevent` produces
  // exactly this, and so does an emulator, and the two keys under test would
  // otherwise share one physical value and cancel each other's press. The key
  // code cannot also be zero -- `KEYCODE_UNKNOWN` events are dropped before
  // this -- so it is what tells them apart.
  if (scan_code == 0) {
    return KeyOfAndroidPlane(key_code);
  }
  return Lookup(kAndroidScanCodeToPhysical, scan_code,
                KeyOfAndroidPlane(scan_code));
}

uint64_t LogicalKeyForAndroidKeyCode(uint32_t key_code) {
  return Lookup(kAndroidKeyCodeToLogical, key_code,
                KeyOfAndroidPlane(key_code));
}

// The modifiers whose held state Android reports as a bit in `getMetaState()`,
// and the keys each bit stands for. Ctrl, Shift and Alt; upstream leaves Meta
// out of this list, and so does this.
//
// Upstream uses only the unsided bits (META_SHIFT_ON, never META_SHIFT_LEFT_ON)
// because ChromeOS reports a right-hand modifier as UNSIDED | LEFT_SIDE, which
// makes the sided bits worse than useless.
constexpr ModifierKeyPair kCtrlKeys[] = {
    {0x000000700e0, 0x00200000100},  // ControlLeft
    {0x000000700e4, 0x00200000101},  // ControlRight
};

constexpr ModifierKeyPair kShiftKeys[] = {
    {0x000000700e1, 0x00200000102},  // ShiftLeft
    {0x000000700e5, 0x00200000103},  // ShiftRight
};

constexpr ModifierKeyPair kAltKeys[] = {
    {0x000000700e2, 0x00200000104},  // AltLeft
    {0x000000700e6, 0x00200000105},  // AltRight
};

constexpr PressingGoal kAndroidPressingGoals[] = {
    {0x00001000, kCtrlKeys, 2},   // KeyEvent.META_CTRL_ON
    {0x00000001, kShiftKeys, 2},  // KeyEvent.META_SHIFT_ON
    {0x00000002, kAltKeys, 2},    // KeyEvent.META_ALT_ON
};

const PressingGoal* AndroidPressingGoals(size_t* count) {
  *count = sizeof(kAndroidPressingGoals) / sizeof(kAndroidPressingGoals[0]);
  return kAndroidPressingGoals;
}

// The locks. A lock is on while nobody is touching its key, which is the whole
// difference between it and a modifier, and why its state cannot be read off
// the held set.
//
// CapsLock alone. Upstream leaves NumLock and ScrollLock out because on
// ChromeOS their presses set no meta bit at all, and a goal watching a bit
// that never changes would either do nothing or fight forever.
constexpr TogglingGoal kAndroidTogglingGoals[] = {
    {0x00100000, 0x00000070039, 0x00100000104},  // KeyEvent.META_CAPS_LOCK_ON
};

const TogglingGoal* AndroidTogglingGoals(size_t* count) {
  *count = sizeof(kAndroidTogglingGoals) / sizeof(kAndroidTogglingGoals[0]);
  return kAndroidTogglingGoals;
}

}  // namespace flutter
