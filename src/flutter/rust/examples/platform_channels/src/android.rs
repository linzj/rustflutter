// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The Android half of the probe, which is the half that cannot exist.
//!
//! On Windows the probe drives the host's own window: it posts `WM_CHAR`, opens
//! a composition through IMM32, asks for a cursor and reads it back, and sends
//! a `WM_CLOSE` the application refuses. All of that works because a Win32
//! window accepts messages from any process, including its own.
//!
//! Android does not work that way, and it is not an oversight. Injecting input
//! needs `INJECT_EVENTS`, which is a system permission an application cannot
//! hold; the IME is a separate process that decides for itself when to compose;
//! there is no mouse cursor on a touch screen; and a window is closed by the
//! system, not by a message the application can send itself. An application
//! that could do these things to itself would be an application that could do
//! them to any other, which is the reason the platform says no.
//!
//! So the input-driven half of this example reports SKIP on Android rather than
//! pretending. What still runs is everything that is a *channel* rather than a
//! gesture -- the clipboard, the lifecycle, an unserved channel, an unserved
//! method, the settings, the languages, and the exit handshake -- and that is
//! the half this example exists to check, because it is the half that crosses
//! the C ABI.
//!
//! Everything skipped here is covered by hand instead: see PORTING_STATUS.md.

use std::ffi::c_void;

pub type Hwnd = *mut c_void;

/// Whether this platform lets the probe drive its own window.
///
/// The one thing main.rs branches on. Where it is false, the steps that need a
/// synthetic click, keystroke, composition or close are not run at all, and the
/// report says so instead of reporting a pass nobody earned.
pub const DRIVES_INPUT: bool = false;

pub fn find_window() -> Hwnd {
    std::ptr::null_mut()
}

pub fn click_centre(_window: Hwnd) {}

pub fn type_char(_window: Hwnd, _character: char) {}

pub fn compose(_window: Hwnd, _text: &str) -> bool {
    false
}

pub fn commit_composition(_window: Hwnd) -> bool {
    false
}

pub fn ask_to_set_cursor(_window: Hwnd) -> bool {
    false
}

pub fn current_cursor(_window: Hwnd) -> Option<Hwnd> {
    None
}

pub fn text_cursor() -> Hwnd {
    std::ptr::null_mut()
}

pub fn close(_window: Hwnd) {}

pub fn is_open(_window: Hwnd) -> bool {
    true
}

/// What the platform says the theme is, read a second way.
///
/// `None` because there is no second way here: Android reports the night mode
/// through `Configuration`, which is the same source the host already read and
/// sent on `flutter/settings`. Comparing a value with itself would check
/// nothing, and saying so is better than a check that cannot fail.
pub fn prefers_dark_theme() -> Option<bool> {
    None
}
