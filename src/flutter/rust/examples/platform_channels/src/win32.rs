// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Just enough Win32 to drive the probe's own window.
//!
//! None of this belongs in an application. It is here because the paths being
//! checked -- focusing a field, typing into it, composing with an IME -- only
//! run when a person does something, and there is no person. The album's
//! keyboard test does the same from Python; doing it in-process keeps this
//! check runnable on its own.

use std::os::raw::{c_int, c_long, c_ulong, c_void};

pub type Hwnd = *mut c_void;
pub type Himc = *mut c_void;

const WM_CHAR: u32 = 0x0102;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const MK_LBUTTON: usize = 0x0001;

// ImmSetCompositionString's index, and ImmNotifyIME's action and index.
const SCS_SETSTR: u32 = 0x0009;
const NI_COMPOSITIONSTR: u32 = 0x0015;
const CPS_COMPLETE: u32 = 0x0001;

#[repr(C)]
#[derive(Default)]
struct Rect {
    left: c_long,
    top: c_long,
    right: c_long,
    bottom: c_long,
}

unsafe extern "system" {
    fn FindWindowW(class_name: *const u16, window_name: *const u16) -> Hwnd;
    fn PostMessageW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> c_int;
    fn GetClientRect(window: Hwnd, rect: *mut Rect) -> c_int;
    fn ImmGetContext(window: Hwnd) -> Himc;
    fn ImmReleaseContext(window: Hwnd, context: Himc) -> c_int;
    fn ImmSetCompositionStringW(
        context: Himc,
        index: u32,
        composition: *const c_void,
        composition_length: c_ulong,
        reading: *const c_void,
        reading_length: c_ulong,
    ) -> c_int;
    fn ImmNotifyIME(context: Himc, action: u32, index: u32, value: u32) -> c_int;
    fn ImmGetOpenStatus(context: Himc) -> c_int;
    fn ImmSetOpenStatus(context: Himc, open: c_int) -> c_int;
}

/// The host's window, by the class name it registers.
pub fn find_window() -> Hwnd {
    let class: Vec<u16> = "RustflutterHostWindow\0".encode_utf16().collect();
    unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) }
}

/// Clicks the middle of the client area, which is where the field is.
///
/// A down and an up close enough together to be a tap rather than a drag --
/// the gesture router decides that, and it is the same decision a mouse makes.
pub fn click_centre(window: Hwnd) {
    if window.is_null() {
        return;
    }
    let mut rect = Rect::default();
    if unsafe { GetClientRect(window, &mut rect) } == 0 {
        return;
    }
    let x = (rect.right - rect.left) / 2;
    let y = (rect.bottom - rect.top) / 2;
    // Client coordinates, packed low word first, which is what the window proc
    // unpacks with GET_X_LPARAM.
    let position = ((y as isize) << 16) | (x as isize & 0xFFFF);
    unsafe {
        PostMessageW(window, WM_LBUTTONDOWN, MK_LBUTTON, position);
        PostMessageW(window, WM_LBUTTONUP, 0, position);
    }
}

/// Types one character, as the keyboard would once the layout has had its say.
/// This is the path with no IME in it.
pub fn type_char(window: Hwnd, character: char) {
    if window.is_null() {
        return;
    }
    unsafe { PostMessageW(window, WM_CHAR, character as usize, 0) };
}

/// Drives a composition the way an IME does, without needing one installed.
///
/// `ImmSetCompositionStringW` is how an IME tells the input context what is
/// being composed, and the context is what sends `WM_IME_COMPOSITION` to the
/// window. Calling it directly exercises the same messages, the same
/// `ImmGetCompositionString` reads and the same commit that a reader typing
/// pinyin would -- minus the reader.
///
/// Returns false when there is no input context to compose in, which is not a
/// failure: a machine with the IME disabled has nothing to check here.
pub fn compose(window: Hwnd, text: &str) -> bool {
    if window.is_null() {
        return false;
    }
    let context = unsafe { ImmGetContext(window) };
    if context.is_null() {
        return false;
    }
    if unsafe { ImmGetOpenStatus(context) } == 0 {
        unsafe { ImmSetOpenStatus(context, 1) };
    }
    let units: Vec<u16> = text.encode_utf16().collect();
    let bytes = (units.len() * std::mem::size_of::<u16>()) as c_ulong;
    let ok = unsafe {
        ImmSetCompositionStringW(
            context,
            SCS_SETSTR,
            units.as_ptr() as *const c_void,
            bytes,
            std::ptr::null(),
            0,
        )
    };
    unsafe { ImmReleaseContext(window, context) };
    ok != 0
}

/// Commits whatever is being composed, as choosing a candidate would.
pub fn commit_composition(window: Hwnd) -> bool {
    if window.is_null() {
        return false;
    }
    let context = unsafe { ImmGetContext(window) };
    if context.is_null() {
        return false;
    }
    let ok = unsafe { ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_COMPLETE, 0) };
    unsafe { ImmReleaseContext(window, context) };
    ok != 0
}
