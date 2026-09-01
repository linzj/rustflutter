// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The macOS half of the probe, which does not exist yet.
//!
//! On Windows the probe drives the host's own window: it posts `WM_CHAR`, opens
//! a composition through IMM32, asks for a cursor and reads it back, and sends
//! a `WM_CLOSE` the application refuses. The macOS host has no window to drive
//! yet -- see `rustflutter_host_stub.cc` -- so the input-driven half of this
//! example reports SKIP rather than pretending. (Even with a window, posting a
//! synthetic event from this process would take the accessibility trust a user
//! grants in System Settings, which a self-checking example cannot assume it
//! has; an application may not inject input into itself unasked, which is
//! Android's reason and iOS's too.)
//!
//! What still runs is everything that is a *channel* rather than a gesture --
//! the clipboard, the lifecycle, an unserved channel, an unserved method, the
//! settings, the languages, and the exit handshake -- and that is the half this
//! example exists to check, because it is the half that crosses the C ABI.

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
/// `None` because there is no second way here: there is no macOS host window
/// whose setting could be compared against what arrived on `flutter/settings`.
pub fn prefers_dark_theme() -> Option<bool> {
    None
}
