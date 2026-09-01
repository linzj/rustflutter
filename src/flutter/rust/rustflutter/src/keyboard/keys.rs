// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Names for the key values the host sends up.
//!
//! GENERATED -- see `rust/host/tools/gen_key_map.py`. Both sets come from the
//! same upstream table the C++ side is built from, so a name here and a value
//! there cannot drift apart. Upstream these are `PhysicalKeyboardKey` and
//! `LogicalKeyboardKey` in `packages/flutter/lib/src/services/keyboard_key.g.dart`.

use super::{LogicalKey, PhysicalKey};

/// Where the key is on the keyboard, regardless of layout. USB HID usage codes.
impl PhysicalKey {
    /// `escape`
    pub const ESCAPE: PhysicalKey = PhysicalKey(0x70029);
    /// `digit1`
    pub const DIGIT1: PhysicalKey = PhysicalKey(0x7001e);
    /// `digit2`
    pub const DIGIT2: PhysicalKey = PhysicalKey(0x7001f);
    /// `digit3`
    pub const DIGIT3: PhysicalKey = PhysicalKey(0x70020);
    /// `digit4`
    pub const DIGIT4: PhysicalKey = PhysicalKey(0x70021);
    /// `digit5`
    pub const DIGIT5: PhysicalKey = PhysicalKey(0x70022);
    /// `digit6`
    pub const DIGIT6: PhysicalKey = PhysicalKey(0x70023);
    /// `digit7`
    pub const DIGIT7: PhysicalKey = PhysicalKey(0x70024);
    /// `digit8`
    pub const DIGIT8: PhysicalKey = PhysicalKey(0x70025);
    /// `digit9`
    pub const DIGIT9: PhysicalKey = PhysicalKey(0x70026);
    /// `digit0`
    pub const DIGIT0: PhysicalKey = PhysicalKey(0x70027);
    /// `minus`
    pub const MINUS: PhysicalKey = PhysicalKey(0x7002d);
    /// `equal`
    pub const EQUAL: PhysicalKey = PhysicalKey(0x7002e);
    /// `backspace`
    pub const BACKSPACE: PhysicalKey = PhysicalKey(0x7002a);
    /// `tab`
    pub const TAB: PhysicalKey = PhysicalKey(0x7002b);
    /// `keyQ`
    pub const KEY_Q: PhysicalKey = PhysicalKey(0x70014);
    /// `keyW`
    pub const KEY_W: PhysicalKey = PhysicalKey(0x7001a);
    /// `keyE`
    pub const KEY_E: PhysicalKey = PhysicalKey(0x70008);
    /// `keyR`
    pub const KEY_R: PhysicalKey = PhysicalKey(0x70015);
    /// `keyT`
    pub const KEY_T: PhysicalKey = PhysicalKey(0x70017);
    /// `keyY`
    pub const KEY_Y: PhysicalKey = PhysicalKey(0x7001c);
    /// `keyU`
    pub const KEY_U: PhysicalKey = PhysicalKey(0x70018);
    /// `keyI`
    pub const KEY_I: PhysicalKey = PhysicalKey(0x7000c);
    /// `keyO`
    pub const KEY_O: PhysicalKey = PhysicalKey(0x70012);
    /// `keyP`
    pub const KEY_P: PhysicalKey = PhysicalKey(0x70013);
    /// `bracketLeft`
    pub const BRACKET_LEFT: PhysicalKey = PhysicalKey(0x7002f);
    /// `bracketRight`
    pub const BRACKET_RIGHT: PhysicalKey = PhysicalKey(0x70030);
    /// `enter`
    pub const ENTER: PhysicalKey = PhysicalKey(0x70028);
    /// `controlLeft`
    pub const CONTROL_LEFT: PhysicalKey = PhysicalKey(0x700e0);
    /// `keyA`
    pub const KEY_A: PhysicalKey = PhysicalKey(0x70004);
    /// `keyS`
    pub const KEY_S: PhysicalKey = PhysicalKey(0x70016);
    /// `keyD`
    pub const KEY_D: PhysicalKey = PhysicalKey(0x70007);
    /// `keyF`
    pub const KEY_F: PhysicalKey = PhysicalKey(0x70009);
    /// `keyG`
    pub const KEY_G: PhysicalKey = PhysicalKey(0x7000a);
    /// `keyH`
    pub const KEY_H: PhysicalKey = PhysicalKey(0x7000b);
    /// `keyJ`
    pub const KEY_J: PhysicalKey = PhysicalKey(0x7000d);
    /// `keyK`
    pub const KEY_K: PhysicalKey = PhysicalKey(0x7000e);
    /// `keyL`
    pub const KEY_L: PhysicalKey = PhysicalKey(0x7000f);
    /// `semicolon`
    pub const SEMICOLON: PhysicalKey = PhysicalKey(0x70033);
    /// `quote`
    pub const QUOTE: PhysicalKey = PhysicalKey(0x70034);
    /// `backquote`
    pub const BACKQUOTE: PhysicalKey = PhysicalKey(0x70035);
    /// `shiftLeft`
    pub const SHIFT_LEFT: PhysicalKey = PhysicalKey(0x700e1);
    /// `backslash`
    pub const BACKSLASH: PhysicalKey = PhysicalKey(0x70031);
    /// `keyZ`
    pub const KEY_Z: PhysicalKey = PhysicalKey(0x7001d);
    /// `keyX`
    pub const KEY_X: PhysicalKey = PhysicalKey(0x7001b);
    /// `keyC`
    pub const KEY_C: PhysicalKey = PhysicalKey(0x70006);
    /// `keyV`
    pub const KEY_V: PhysicalKey = PhysicalKey(0x70019);
    /// `keyB`
    pub const KEY_B: PhysicalKey = PhysicalKey(0x70005);
    /// `keyN`
    pub const KEY_N: PhysicalKey = PhysicalKey(0x70011);
    /// `keyM`
    pub const KEY_M: PhysicalKey = PhysicalKey(0x70010);
    /// `comma`
    pub const COMMA: PhysicalKey = PhysicalKey(0x70036);
    /// `period`
    pub const PERIOD: PhysicalKey = PhysicalKey(0x70037);
    /// `slash`
    pub const SLASH: PhysicalKey = PhysicalKey(0x70038);
    /// `shiftRight`
    pub const SHIFT_RIGHT: PhysicalKey = PhysicalKey(0x700e5);
    /// `numpadMultiply`
    pub const NUMPAD_MULTIPLY: PhysicalKey = PhysicalKey(0x70055);
    /// `altLeft`
    pub const ALT_LEFT: PhysicalKey = PhysicalKey(0x700e2);
    /// `space`
    pub const SPACE: PhysicalKey = PhysicalKey(0x7002c);
    /// `capsLock`
    pub const CAPS_LOCK: PhysicalKey = PhysicalKey(0x70039);
    /// `f1`
    pub const F1: PhysicalKey = PhysicalKey(0x7003a);
    /// `f2`
    pub const F2: PhysicalKey = PhysicalKey(0x7003b);
    /// `f3`
    pub const F3: PhysicalKey = PhysicalKey(0x7003c);
    /// `f4`
    pub const F4: PhysicalKey = PhysicalKey(0x7003d);
    /// `f5`
    pub const F5: PhysicalKey = PhysicalKey(0x7003e);
    /// `f6`
    pub const F6: PhysicalKey = PhysicalKey(0x7003f);
    /// `f7`
    pub const F7: PhysicalKey = PhysicalKey(0x70040);
    /// `f8`
    pub const F8: PhysicalKey = PhysicalKey(0x70041);
    /// `f9`
    pub const F9: PhysicalKey = PhysicalKey(0x70042);
    /// `f10`
    pub const F10: PhysicalKey = PhysicalKey(0x70043);
    /// `pause`
    pub const PAUSE: PhysicalKey = PhysicalKey(0x70048);
    /// `scrollLock`
    pub const SCROLL_LOCK: PhysicalKey = PhysicalKey(0x70047);
    /// `numpad7`
    pub const NUMPAD7: PhysicalKey = PhysicalKey(0x7005f);
    /// `numpad8`
    pub const NUMPAD8: PhysicalKey = PhysicalKey(0x70060);
    /// `numpad9`
    pub const NUMPAD9: PhysicalKey = PhysicalKey(0x70061);
    /// `numpadSubtract`
    pub const NUMPAD_SUBTRACT: PhysicalKey = PhysicalKey(0x70056);
    /// `numpad4`
    pub const NUMPAD4: PhysicalKey = PhysicalKey(0x7005c);
    /// `numpad5`
    pub const NUMPAD5: PhysicalKey = PhysicalKey(0x7005d);
    /// `numpad6`
    pub const NUMPAD6: PhysicalKey = PhysicalKey(0x7005e);
    /// `numpadAdd`
    pub const NUMPAD_ADD: PhysicalKey = PhysicalKey(0x70057);
    /// `numpad1`
    pub const NUMPAD1: PhysicalKey = PhysicalKey(0x70059);
    /// `numpad2`
    pub const NUMPAD2: PhysicalKey = PhysicalKey(0x7005a);
    /// `numpad3`
    pub const NUMPAD3: PhysicalKey = PhysicalKey(0x7005b);
    /// `numpad0`
    pub const NUMPAD0: PhysicalKey = PhysicalKey(0x70062);
    /// `numpadDecimal`
    pub const NUMPAD_DECIMAL: PhysicalKey = PhysicalKey(0x70063);
    /// `intlBackslash`
    pub const INTL_BACKSLASH: PhysicalKey = PhysicalKey(0x70064);
    /// `f11`
    pub const F11: PhysicalKey = PhysicalKey(0x70044);
    /// `f12`
    pub const F12: PhysicalKey = PhysicalKey(0x70045);
    /// `numpadEqual`
    pub const NUMPAD_EQUAL: PhysicalKey = PhysicalKey(0x70067);
    /// `f13`
    pub const F13: PhysicalKey = PhysicalKey(0x70068);
    /// `f14`
    pub const F14: PhysicalKey = PhysicalKey(0x70069);
    /// `f15`
    pub const F15: PhysicalKey = PhysicalKey(0x7006a);
    /// `f16`
    pub const F16: PhysicalKey = PhysicalKey(0x7006b);
    /// `f17`
    pub const F17: PhysicalKey = PhysicalKey(0x7006c);
    /// `f18`
    pub const F18: PhysicalKey = PhysicalKey(0x7006d);
    /// `f19`
    pub const F19: PhysicalKey = PhysicalKey(0x7006e);
    /// `f20`
    pub const F20: PhysicalKey = PhysicalKey(0x7006f);
    /// `f21`
    pub const F21: PhysicalKey = PhysicalKey(0x70070);
    /// `f22`
    pub const F22: PhysicalKey = PhysicalKey(0x70071);
    /// `f23`
    pub const F23: PhysicalKey = PhysicalKey(0x70072);
    /// `kanaMode`
    pub const KANA_MODE: PhysicalKey = PhysicalKey(0x70088);
    /// `lang2`
    pub const LANG2: PhysicalKey = PhysicalKey(0x70091);
    /// `lang1`
    pub const LANG1: PhysicalKey = PhysicalKey(0x70090);
    /// `intlRo`
    pub const INTL_RO: PhysicalKey = PhysicalKey(0x70087);
    /// `f24`
    pub const F24: PhysicalKey = PhysicalKey(0x70073);
    /// `lang4`
    pub const LANG4: PhysicalKey = PhysicalKey(0x70093);
    /// `lang3`
    pub const LANG3: PhysicalKey = PhysicalKey(0x70092);
    /// `convert`
    pub const CONVERT: PhysicalKey = PhysicalKey(0x7008a);
    /// `nonConvert`
    pub const NON_CONVERT: PhysicalKey = PhysicalKey(0x7008b);
    /// `intlYen`
    pub const INTL_YEN: PhysicalKey = PhysicalKey(0x70089);
    /// `numpadComma`
    pub const NUMPAD_COMMA: PhysicalKey = PhysicalKey(0x70085);
    /// `usbPostFail`
    pub const USB_POST_FAIL: PhysicalKey = PhysicalKey(0x70002);
    /// `usbErrorRollOver`
    pub const USB_ERROR_ROLL_OVER: PhysicalKey = PhysicalKey(0x70001);
    /// `undo`
    pub const UNDO: PhysicalKey = PhysicalKey(0x7007a);
    /// `paste`
    pub const PASTE: PhysicalKey = PhysicalKey(0x7007d);
    /// `mediaTrackPrevious`
    pub const MEDIA_TRACK_PREVIOUS: PhysicalKey = PhysicalKey(0xc00b6);
    /// `cut`
    pub const CUT: PhysicalKey = PhysicalKey(0x7007b);
    /// `copy`
    pub const COPY: PhysicalKey = PhysicalKey(0x7007c);
    /// `mediaTrackNext`
    pub const MEDIA_TRACK_NEXT: PhysicalKey = PhysicalKey(0xc00b5);
    /// `numpadEnter`
    pub const NUMPAD_ENTER: PhysicalKey = PhysicalKey(0x70058);
    /// `controlRight`
    pub const CONTROL_RIGHT: PhysicalKey = PhysicalKey(0x700e4);
    /// `audioVolumeMute`
    pub const AUDIO_VOLUME_MUTE: PhysicalKey = PhysicalKey(0x7007f);
    /// `launchApp2`
    pub const LAUNCH_APP2: PhysicalKey = PhysicalKey(0xc0192);
    /// `mediaPlayPause`
    pub const MEDIA_PLAY_PAUSE: PhysicalKey = PhysicalKey(0xc00cd);
    /// `mediaStop`
    pub const MEDIA_STOP: PhysicalKey = PhysicalKey(0xc00b7);
    /// `eject`
    pub const EJECT: PhysicalKey = PhysicalKey(0xc00b8);
    /// `audioVolumeDown`
    pub const AUDIO_VOLUME_DOWN: PhysicalKey = PhysicalKey(0x70081);
    /// `audioVolumeUp`
    pub const AUDIO_VOLUME_UP: PhysicalKey = PhysicalKey(0x70080);
    /// `browserHome`
    pub const BROWSER_HOME: PhysicalKey = PhysicalKey(0xc0223);
    /// `numpadDivide`
    pub const NUMPAD_DIVIDE: PhysicalKey = PhysicalKey(0x70054);
    /// `printScreen`
    pub const PRINT_SCREEN: PhysicalKey = PhysicalKey(0x70046);
    /// `altRight`
    pub const ALT_RIGHT: PhysicalKey = PhysicalKey(0x700e6);
    /// `help`
    pub const HELP: PhysicalKey = PhysicalKey(0x70075);
    /// `numLock`
    pub const NUM_LOCK: PhysicalKey = PhysicalKey(0x70053);
    /// `home`
    pub const HOME: PhysicalKey = PhysicalKey(0x7004a);
    /// `arrowUp`
    pub const ARROW_UP: PhysicalKey = PhysicalKey(0x70052);
    /// `pageUp`
    pub const PAGE_UP: PhysicalKey = PhysicalKey(0x7004b);
    /// `arrowLeft`
    pub const ARROW_LEFT: PhysicalKey = PhysicalKey(0x70050);
    /// `arrowRight`
    pub const ARROW_RIGHT: PhysicalKey = PhysicalKey(0x7004f);
    /// `end`
    pub const END: PhysicalKey = PhysicalKey(0x7004d);
    /// `arrowDown`
    pub const ARROW_DOWN: PhysicalKey = PhysicalKey(0x70051);
    /// `pageDown`
    pub const PAGE_DOWN: PhysicalKey = PhysicalKey(0x7004e);
    /// `insert`
    pub const INSERT: PhysicalKey = PhysicalKey(0x70049);
    /// `delete`
    pub const DELETE: PhysicalKey = PhysicalKey(0x7004c);
    /// `metaLeft`
    pub const META_LEFT: PhysicalKey = PhysicalKey(0x700e3);
    /// `metaRight`
    pub const META_RIGHT: PhysicalKey = PhysicalKey(0x700e7);
    /// `contextMenu`
    pub const CONTEXT_MENU: PhysicalKey = PhysicalKey(0x70065);
    /// `power`
    pub const POWER: PhysicalKey = PhysicalKey(0x70066);
    /// `sleep`
    pub const SLEEP: PhysicalKey = PhysicalKey(0x10082);
    /// `wakeUp`
    pub const WAKE_UP: PhysicalKey = PhysicalKey(0x10083);
    /// `browserSearch`
    pub const BROWSER_SEARCH: PhysicalKey = PhysicalKey(0xc0221);
    /// `browserFavorites`
    pub const BROWSER_FAVORITES: PhysicalKey = PhysicalKey(0xc022a);
    /// `browserRefresh`
    pub const BROWSER_REFRESH: PhysicalKey = PhysicalKey(0xc0227);
    /// `browserStop`
    pub const BROWSER_STOP: PhysicalKey = PhysicalKey(0xc0226);
    /// `browserForward`
    pub const BROWSER_FORWARD: PhysicalKey = PhysicalKey(0xc0225);
    /// `browserBack`
    pub const BROWSER_BACK: PhysicalKey = PhysicalKey(0xc0224);
    /// `launchApp1`
    pub const LAUNCH_APP1: PhysicalKey = PhysicalKey(0xc0194);
    /// `launchMail`
    pub const LAUNCH_MAIL: PhysicalKey = PhysicalKey(0xc018a);
    /// `mediaSelect`
    pub const MEDIA_SELECT: PhysicalKey = PhysicalKey(0xc0183);
}

/// What the key means under the current layout.
impl LogicalKey {
    /// `cancel`
    pub const CANCEL: LogicalKey = LogicalKey(0x100000504);
    /// `backspace`
    pub const BACKSPACE: LogicalKey = LogicalKey(0x100000008);
    /// `tab`
    pub const TAB: LogicalKey = LogicalKey(0x100000009);
    /// `clear`
    pub const CLEAR: LogicalKey = LogicalKey(0x100000401);
    /// `enter`
    pub const ENTER: LogicalKey = LogicalKey(0x10000000d);
    /// `shiftLeft`
    pub const SHIFT_LEFT: LogicalKey = LogicalKey(0x200000102);
    /// `controlLeft`
    pub const CONTROL_LEFT: LogicalKey = LogicalKey(0x200000100);
    /// `pause`
    pub const PAUSE: LogicalKey = LogicalKey(0x100000509);
    /// `capsLock`
    pub const CAPS_LOCK: LogicalKey = LogicalKey(0x100000104);
    /// `lang1`
    pub const LANG1: LogicalKey = LogicalKey(0x200000010);
    /// `junjaMode`
    pub const JUNJA_MODE: LogicalKey = LogicalKey(0x100000713);
    /// `finalMode`
    pub const FINAL_MODE: LogicalKey = LogicalKey(0x100000706);
    /// `kanjiMode`
    pub const KANJI_MODE: LogicalKey = LogicalKey(0x100000719);
    /// `escape`
    pub const ESCAPE: LogicalKey = LogicalKey(0x10000001b);
    /// `convert`
    pub const CONVERT: LogicalKey = LogicalKey(0x100000705);
    /// `accept`
    pub const ACCEPT: LogicalKey = LogicalKey(0x100000501);
    /// `modeChange`
    pub const MODE_CHANGE: LogicalKey = LogicalKey(0x10000070b);
    /// `space`
    pub const SPACE: LogicalKey = LogicalKey(0x20);
    /// `pageUp`
    pub const PAGE_UP: LogicalKey = LogicalKey(0x100000308);
    /// `pageDown`
    pub const PAGE_DOWN: LogicalKey = LogicalKey(0x100000307);
    /// `end`
    pub const END: LogicalKey = LogicalKey(0x100000305);
    /// `home`
    pub const HOME: LogicalKey = LogicalKey(0x100000306);
    /// `arrowLeft`
    pub const ARROW_LEFT: LogicalKey = LogicalKey(0x100000302);
    /// `arrowUp`
    pub const ARROW_UP: LogicalKey = LogicalKey(0x100000304);
    /// `arrowRight`
    pub const ARROW_RIGHT: LogicalKey = LogicalKey(0x100000303);
    /// `arrowDown`
    pub const ARROW_DOWN: LogicalKey = LogicalKey(0x100000301);
    /// `select`
    pub const SELECT: LogicalKey = LogicalKey(0x10000050c);
    /// `print`
    pub const PRINT: LogicalKey = LogicalKey(0x100000a0c);
    /// `execute`
    pub const EXECUTE: LogicalKey = LogicalKey(0x100000506);
    /// `printScreen`
    pub const PRINT_SCREEN: LogicalKey = LogicalKey(0x100000608);
    /// `insert`
    pub const INSERT: LogicalKey = LogicalKey(0x100000407);
    /// `delete`
    pub const DELETE: LogicalKey = LogicalKey(0x10000007f);
    /// `help`
    pub const HELP: LogicalKey = LogicalKey(0x100000508);
    /// `metaLeft`
    pub const META_LEFT: LogicalKey = LogicalKey(0x200000106);
    /// `metaRight`
    pub const META_RIGHT: LogicalKey = LogicalKey(0x200000107);
    /// `contextMenu`
    pub const CONTEXT_MENU: LogicalKey = LogicalKey(0x100000505);
    /// `sleep`
    pub const SLEEP: LogicalKey = LogicalKey(0x200000002);
    /// `numpad0`
    pub const NUMPAD0: LogicalKey = LogicalKey(0x200000230);
    /// `numpad1`
    pub const NUMPAD1: LogicalKey = LogicalKey(0x200000231);
    /// `numpad2`
    pub const NUMPAD2: LogicalKey = LogicalKey(0x200000232);
    /// `numpad3`
    pub const NUMPAD3: LogicalKey = LogicalKey(0x200000233);
    /// `numpad4`
    pub const NUMPAD4: LogicalKey = LogicalKey(0x200000234);
    /// `numpad5`
    pub const NUMPAD5: LogicalKey = LogicalKey(0x200000235);
    /// `numpad6`
    pub const NUMPAD6: LogicalKey = LogicalKey(0x200000236);
    /// `numpad7`
    pub const NUMPAD7: LogicalKey = LogicalKey(0x200000237);
    /// `numpad8`
    pub const NUMPAD8: LogicalKey = LogicalKey(0x200000238);
    /// `numpad9`
    pub const NUMPAD9: LogicalKey = LogicalKey(0x200000239);
    /// `numpadMultiply`
    pub const NUMPAD_MULTIPLY: LogicalKey = LogicalKey(0x20000022a);
    /// `numpadAdd`
    pub const NUMPAD_ADD: LogicalKey = LogicalKey(0x20000022b);
    /// `numpadComma`
    pub const NUMPAD_COMMA: LogicalKey = LogicalKey(0x20000022c);
    /// `numpadSubtract`
    pub const NUMPAD_SUBTRACT: LogicalKey = LogicalKey(0x20000022d);
    /// `numpadDecimal`
    pub const NUMPAD_DECIMAL: LogicalKey = LogicalKey(0x20000022e);
    /// `numpadDivide`
    pub const NUMPAD_DIVIDE: LogicalKey = LogicalKey(0x20000022f);
    /// `f1`
    pub const F1: LogicalKey = LogicalKey(0x100000801);
    /// `f2`
    pub const F2: LogicalKey = LogicalKey(0x100000802);
    /// `f3`
    pub const F3: LogicalKey = LogicalKey(0x100000803);
    /// `f4`
    pub const F4: LogicalKey = LogicalKey(0x100000804);
    /// `f5`
    pub const F5: LogicalKey = LogicalKey(0x100000805);
    /// `f6`
    pub const F6: LogicalKey = LogicalKey(0x100000806);
    /// `f7`
    pub const F7: LogicalKey = LogicalKey(0x100000807);
    /// `f8`
    pub const F8: LogicalKey = LogicalKey(0x100000808);
    /// `f9`
    pub const F9: LogicalKey = LogicalKey(0x100000809);
    /// `f10`
    pub const F10: LogicalKey = LogicalKey(0x10000080a);
    /// `f11`
    pub const F11: LogicalKey = LogicalKey(0x10000080b);
    /// `f12`
    pub const F12: LogicalKey = LogicalKey(0x10000080c);
    /// `f13`
    pub const F13: LogicalKey = LogicalKey(0x10000080d);
    /// `f14`
    pub const F14: LogicalKey = LogicalKey(0x10000080e);
    /// `f15`
    pub const F15: LogicalKey = LogicalKey(0x10000080f);
    /// `f16`
    pub const F16: LogicalKey = LogicalKey(0x100000810);
    /// `f17`
    pub const F17: LogicalKey = LogicalKey(0x100000811);
    /// `f18`
    pub const F18: LogicalKey = LogicalKey(0x100000812);
    /// `f19`
    pub const F19: LogicalKey = LogicalKey(0x100000813);
    /// `f20`
    pub const F20: LogicalKey = LogicalKey(0x100000814);
    /// `f21`
    pub const F21: LogicalKey = LogicalKey(0x100000815);
    /// `f22`
    pub const F22: LogicalKey = LogicalKey(0x100000816);
    /// `f23`
    pub const F23: LogicalKey = LogicalKey(0x100000817);
    /// `f24`
    pub const F24: LogicalKey = LogicalKey(0x100000818);
    /// `numLock`
    pub const NUM_LOCK: LogicalKey = LogicalKey(0x10000010a);
    /// `scrollLock`
    pub const SCROLL_LOCK: LogicalKey = LogicalKey(0x10000010c);
    /// `numpadEqual`
    pub const NUMPAD_EQUAL: LogicalKey = LogicalKey(0x20000023d);
    /// `shiftRight`
    pub const SHIFT_RIGHT: LogicalKey = LogicalKey(0x200000103);
    /// `controlRight`
    pub const CONTROL_RIGHT: LogicalKey = LogicalKey(0x200000101);
    /// `altLeft`
    pub const ALT_LEFT: LogicalKey = LogicalKey(0x200000104);
    /// `altRight`
    pub const ALT_RIGHT: LogicalKey = LogicalKey(0x200000105);
    /// `browserBack`
    pub const BROWSER_BACK: LogicalKey = LogicalKey(0x100000c01);
    /// `browserForward`
    pub const BROWSER_FORWARD: LogicalKey = LogicalKey(0x100000c03);
    /// `browserRefresh`
    pub const BROWSER_REFRESH: LogicalKey = LogicalKey(0x100000c05);
    /// `browserStop`
    pub const BROWSER_STOP: LogicalKey = LogicalKey(0x100000c07);
    /// `browserSearch`
    pub const BROWSER_SEARCH: LogicalKey = LogicalKey(0x100000c06);
    /// `browserFavorites`
    pub const BROWSER_FAVORITES: LogicalKey = LogicalKey(0x100000c02);
    /// `browserHome`
    pub const BROWSER_HOME: LogicalKey = LogicalKey(0x100000c04);
    /// `audioVolumeMute`
    pub const AUDIO_VOLUME_MUTE: LogicalKey = LogicalKey(0x100000a11);
    /// `audioVolumeDown`
    pub const AUDIO_VOLUME_DOWN: LogicalKey = LogicalKey(0x100000a0f);
    /// `audioVolumeUp`
    pub const AUDIO_VOLUME_UP: LogicalKey = LogicalKey(0x100000a10);
    /// `mediaStop`
    pub const MEDIA_STOP: LogicalKey = LogicalKey(0x100000a07);
    /// `mediaPlayPause`
    pub const MEDIA_PLAY_PAUSE: LogicalKey = LogicalKey(0x100000a05);
    /// `launchMail`
    pub const LAUNCH_MAIL: LogicalKey = LogicalKey(0x100000b03);
    /// `semicolon`
    pub const SEMICOLON: LogicalKey = LogicalKey(0x3b);
    /// `equal`
    pub const EQUAL: LogicalKey = LogicalKey(0x3d);
    /// `comma`
    pub const COMMA: LogicalKey = LogicalKey(0x2c);
    /// `minus`
    pub const MINUS: LogicalKey = LogicalKey(0x2d);
    /// `period`
    pub const PERIOD: LogicalKey = LogicalKey(0x2e);
    /// `slash`
    pub const SLASH: LogicalKey = LogicalKey(0x2f);
    /// `backquote`
    pub const BACKQUOTE: LogicalKey = LogicalKey(0x60);
    /// `gameButton8`
    pub const GAME_BUTTON8: LogicalKey = LogicalKey(0x200000308);
    /// `gameButton9`
    pub const GAME_BUTTON9: LogicalKey = LogicalKey(0x200000309);
    /// `gameButton10`
    pub const GAME_BUTTON10: LogicalKey = LogicalKey(0x20000030a);
    /// `gameButton11`
    pub const GAME_BUTTON11: LogicalKey = LogicalKey(0x20000030b);
    /// `gameButton12`
    pub const GAME_BUTTON12: LogicalKey = LogicalKey(0x20000030c);
    /// `gameButton13`
    pub const GAME_BUTTON13: LogicalKey = LogicalKey(0x20000030d);
    /// `gameButton14`
    pub const GAME_BUTTON14: LogicalKey = LogicalKey(0x20000030e);
    /// `gameButton15`
    pub const GAME_BUTTON15: LogicalKey = LogicalKey(0x20000030f);
    /// `gameButton16`
    pub const GAME_BUTTON16: LogicalKey = LogicalKey(0x200000310);
    /// `bracketLeft`
    pub const BRACKET_LEFT: LogicalKey = LogicalKey(0x5b);
    /// `backslash`
    pub const BACKSLASH: LogicalKey = LogicalKey(0x5c);
    /// `bracketRight`
    pub const BRACKET_RIGHT: LogicalKey = LogicalKey(0x5d);
    /// `quote`
    pub const QUOTE: LogicalKey = LogicalKey(0x22);
    /// `attn`
    pub const ATTN: LogicalKey = LogicalKey(0x100000503);
    /// `play`
    pub const PLAY: LogicalKey = LogicalKey(0x10000050a);
    /// `altGraph`
    pub const ALT_GRAPH: LogicalKey = LogicalKey(0x100000103);
    /// `hyper`
    pub const HYPER: LogicalKey = LogicalKey(0x100000108);
    /// `superKey`
    pub const SUPER_KEY: LogicalKey = LogicalKey(0x10000010e);
    /// `copy`
    pub const COPY: LogicalKey = LogicalKey(0x100000402);
    /// `cut`
    pub const CUT: LogicalKey = LogicalKey(0x100000404);
    /// `eraseEof`
    pub const ERASE_EOF: LogicalKey = LogicalKey(0x100000405);
    /// `exSel`
    pub const EX_SEL: LogicalKey = LogicalKey(0x100000406);
    /// `paste`
    pub const PASTE: LogicalKey = LogicalKey(0x100000408);
    /// `redo`
    pub const REDO: LogicalKey = LogicalKey(0x100000409);
    /// `undo`
    pub const UNDO: LogicalKey = LogicalKey(0x10000040a);
    /// `find`
    pub const FIND: LogicalKey = LogicalKey(0x100000507);
    /// `zoomIn`
    pub const ZOOM_IN: LogicalKey = LogicalKey(0x10000050d);
    /// `zoomOut`
    pub const ZOOM_OUT: LogicalKey = LogicalKey(0x10000050e);
    /// `brightnessDown`
    pub const BRIGHTNESS_DOWN: LogicalKey = LogicalKey(0x100000601);
    /// `brightnessUp`
    pub const BRIGHTNESS_UP: LogicalKey = LogicalKey(0x100000602);
    /// `eject`
    pub const EJECT: LogicalKey = LogicalKey(0x100000604);
    /// `logOff`
    pub const LOG_OFF: LogicalKey = LogicalKey(0x100000605);
    /// `powerOff`
    pub const POWER_OFF: LogicalKey = LogicalKey(0x100000607);
    /// `standby`
    pub const STANDBY: LogicalKey = LogicalKey(0x10000060a);
    /// `wakeUp`
    pub const WAKE_UP: LogicalKey = LogicalKey(0x10000060b);
    /// `codeInput`
    pub const CODE_INPUT: LogicalKey = LogicalKey(0x100000703);
    /// `groupFirst`
    pub const GROUP_FIRST: LogicalKey = LogicalKey(0x100000707);
    /// `groupLast`
    pub const GROUP_LAST: LogicalKey = LogicalKey(0x100000708);
    /// `groupNext`
    pub const GROUP_NEXT: LogicalKey = LogicalKey(0x100000709);
    /// `groupPrevious`
    pub const GROUP_PREVIOUS: LogicalKey = LogicalKey(0x10000070a);
    /// `previousCandidate`
    pub const PREVIOUS_CANDIDATE: LogicalKey = LogicalKey(0x10000070e);
    /// `singleCandidate`
    pub const SINGLE_CANDIDATE: LogicalKey = LogicalKey(0x100000710);
    /// `hangulMode`
    pub const HANGUL_MODE: LogicalKey = LogicalKey(0x100000711);
    /// `hanjaMode`
    pub const HANJA_MODE: LogicalKey = LogicalKey(0x100000712);
    /// `eisu`
    pub const EISU: LogicalKey = LogicalKey(0x100000714);
    /// `hankaku`
    pub const HANKAKU: LogicalKey = LogicalKey(0x100000715);
    /// `hiragana`
    pub const HIRAGANA: LogicalKey = LogicalKey(0x100000716);
    /// `hiraganaKatakana`
    pub const HIRAGANA_KATAKANA: LogicalKey = LogicalKey(0x100000717);
    /// `katakana`
    pub const KATAKANA: LogicalKey = LogicalKey(0x10000071a);
    /// `romaji`
    pub const ROMAJI: LogicalKey = LogicalKey(0x10000071b);
    /// `zenkaku`
    pub const ZENKAKU: LogicalKey = LogicalKey(0x10000071c);
    /// `zenkakuHankaku`
    pub const ZENKAKU_HANKAKU: LogicalKey = LogicalKey(0x10000071d);
    /// `close`
    pub const CLOSE: LogicalKey = LogicalKey(0x100000a01);
    /// `mailForward`
    pub const MAIL_FORWARD: LogicalKey = LogicalKey(0x100000a02);
    /// `mailReply`
    pub const MAIL_REPLY: LogicalKey = LogicalKey(0x100000a03);
    /// `mailSend`
    pub const MAIL_SEND: LogicalKey = LogicalKey(0x100000a04);
    /// `mediaTrackNext`
    pub const MEDIA_TRACK_NEXT: LogicalKey = LogicalKey(0x100000a08);
    /// `mediaTrackPrevious`
    pub const MEDIA_TRACK_PREVIOUS: LogicalKey = LogicalKey(0x100000a09);
    /// `newKey`
    pub const NEW_KEY: LogicalKey = LogicalKey(0x100000a0a);
    /// `open`
    pub const OPEN: LogicalKey = LogicalKey(0x100000a0b);
    /// `save`
    pub const SAVE: LogicalKey = LogicalKey(0x100000a0d);
    /// `spellCheck`
    pub const SPELL_CHECK: LogicalKey = LogicalKey(0x100000a0e);
    /// `launchCalendar`
    pub const LAUNCH_CALENDAR: LogicalKey = LogicalKey(0x100000b02);
    /// `launchScreenSaver`
    pub const LAUNCH_SCREEN_SAVER: LogicalKey = LogicalKey(0x100000b07);
    /// `launchPhone`
    pub const LAUNCH_PHONE: LogicalKey = LogicalKey(0x100000b0d);
    /// `mediaFastForward`
    pub const MEDIA_FAST_FORWARD: LogicalKey = LogicalKey(0x100000d2c);
    /// `mediaPause`
    pub const MEDIA_PAUSE: LogicalKey = LogicalKey(0x100000d2e);
    /// `mediaPlay`
    pub const MEDIA_PLAY: LogicalKey = LogicalKey(0x100000d2f);
    /// `mediaRecord`
    pub const MEDIA_RECORD: LogicalKey = LogicalKey(0x100000d30);
    /// `mediaRewind`
    pub const MEDIA_REWIND: LogicalKey = LogicalKey(0x100000d31);
    /// `suspend`
    pub const SUSPEND: LogicalKey = LogicalKey(0x200000000);
    /// `intlYen`
    pub const INTL_YEN: LogicalKey = LogicalKey(0x200000022);
    /// `numpadEnter`
    pub const NUMPAD_ENTER: LogicalKey = LogicalKey(0x20000020d);
    /// `control`
    pub const CONTROL: LogicalKey = LogicalKey(0x2000001f0);
    /// `shift`
    pub const SHIFT: LogicalKey = LogicalKey(0x2000001f2);
    /// `alt`
    pub const ALT: LogicalKey = LogicalKey(0x2000001f4);
    /// `meta`
    pub const META: LogicalKey = LogicalKey(0x2000001f6);
    /// `a`
    pub const KEY_A: LogicalKey = LogicalKey(0x61);
    /// `b`
    pub const KEY_B: LogicalKey = LogicalKey(0x62);
    /// `c`
    pub const KEY_C: LogicalKey = LogicalKey(0x63);
    /// `d`
    pub const KEY_D: LogicalKey = LogicalKey(0x64);
    /// `e`
    pub const KEY_E: LogicalKey = LogicalKey(0x65);
    /// `f`
    pub const KEY_F: LogicalKey = LogicalKey(0x66);
    /// `g`
    pub const KEY_G: LogicalKey = LogicalKey(0x67);
    /// `h`
    pub const KEY_H: LogicalKey = LogicalKey(0x68);
    /// `i`
    pub const KEY_I: LogicalKey = LogicalKey(0x69);
    /// `j`
    pub const KEY_J: LogicalKey = LogicalKey(0x6a);
    /// `k`
    pub const KEY_K: LogicalKey = LogicalKey(0x6b);
    /// `l`
    pub const KEY_L: LogicalKey = LogicalKey(0x6c);
    /// `m`
    pub const KEY_M: LogicalKey = LogicalKey(0x6d);
    /// `n`
    pub const KEY_N: LogicalKey = LogicalKey(0x6e);
    /// `o`
    pub const KEY_O: LogicalKey = LogicalKey(0x6f);
    /// `p`
    pub const KEY_P: LogicalKey = LogicalKey(0x70);
    /// `q`
    pub const KEY_Q: LogicalKey = LogicalKey(0x71);
    /// `r`
    pub const KEY_R: LogicalKey = LogicalKey(0x72);
    /// `s`
    pub const KEY_S: LogicalKey = LogicalKey(0x73);
    /// `t`
    pub const KEY_T: LogicalKey = LogicalKey(0x74);
    /// `u`
    pub const KEY_U: LogicalKey = LogicalKey(0x75);
    /// `v`
    pub const KEY_V: LogicalKey = LogicalKey(0x76);
    /// `w`
    pub const KEY_W: LogicalKey = LogicalKey(0x77);
    /// `x`
    pub const KEY_X: LogicalKey = LogicalKey(0x78);
    /// `y`
    pub const KEY_Y: LogicalKey = LogicalKey(0x79);
    /// `z`
    pub const KEY_Z: LogicalKey = LogicalKey(0x7a);
    /// `0`
    pub const DIGIT_0: LogicalKey = LogicalKey(0x30);
    /// `1`
    pub const DIGIT_1: LogicalKey = LogicalKey(0x31);
    /// `2`
    pub const DIGIT_2: LogicalKey = LogicalKey(0x32);
    /// `3`
    pub const DIGIT_3: LogicalKey = LogicalKey(0x33);
    /// `4`
    pub const DIGIT_4: LogicalKey = LogicalKey(0x34);
    /// `5`
    pub const DIGIT_5: LogicalKey = LogicalKey(0x35);
    /// `6`
    pub const DIGIT_6: LogicalKey = LogicalKey(0x36);
    /// `7`
    pub const DIGIT_7: LogicalKey = LogicalKey(0x37);
    /// `8`
    pub const DIGIT_8: LogicalKey = LogicalKey(0x38);
    /// `9`
    pub const DIGIT_9: LogicalKey = LogicalKey(0x39);
}
