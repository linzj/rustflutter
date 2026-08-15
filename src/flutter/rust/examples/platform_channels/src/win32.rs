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

/// Whether this platform lets the probe drive its own window. See android.rs
/// for the platform where it does not.
pub const DRIVES_INPUT: bool = true;
pub type Himc = *mut c_void;

const WM_CLOSE: u32 = 0x0010;
const WM_SETCURSOR: u32 = 0x0020;
const WM_CHAR: u32 = 0x0102;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const MK_LBUTTON: usize = 0x0001;

/// The hit-test code for the client area, which is the only place the window
/// gets to choose the cursor.
const HTCLIENT: isize = 1;

/// `IDC_IBEAM`, the cursor `SystemMouseCursor::Text` maps to. The IDC_ names
/// are integers cast to pointers rather than strings.
const IDC_IBEAM: usize = 32513;

/// The registry value that holds the reader's light/dark choice, and the flags
/// that read it. `RRF_RT_REG_DWORD` refuses to convert from another type, which
/// is what makes a wrong-typed value read as absent rather than as garbage.
const HKEY_CURRENT_USER: isize = -2147483647; // 0x80000001
const RRF_RT_REG_DWORD: u32 = 0x00000018;
const ERROR_SUCCESS: c_long = 0;

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

#[repr(C)]
#[derive(Default)]
struct Point {
    x: c_long,
    y: c_long,
}

/// What `GetCursorInfo` fills in.
#[repr(C)]
#[derive(Default)]
struct CursorInfo {
    size: c_ulong,
    flags: c_ulong,
    cursor: Hwnd,
    x: c_long,
    y: c_long,
}

unsafe extern "system" {
    fn FindWindowW(class_name: *const u16, window_name: *const u16) -> Hwnd;
    fn PostMessageW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> c_int;
    fn SendMessageW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn IsWindow(window: Hwnd) -> c_int;
    fn GetClientRect(window: Hwnd, rect: *mut Rect) -> c_int;
    fn ClientToScreen(window: Hwnd, point: *mut Point) -> c_int;
    fn LoadCursorW(instance: Hwnd, name: usize) -> Hwnd;
    fn GetCursorInfo(info: *mut CursorInfo) -> c_int;
    fn RegGetValueW(
        key: isize,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut c_void,
        size: *mut u32,
    ) -> c_long;
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

/// What the window says when Windows asks it to set the cursor.
///
/// `SendMessage` rather than `PostMessage`: the window proc runs it on the
/// window thread while this call waits, so by the time it returns the cursor
/// has already been applied and can be read back.
///
/// Returns whether the window claimed the message. It has to: a window that
/// falls through to `DefWindowProc` here gets its class cursor put back on the
/// next mouse move, and the framework's choice would last a fraction of a
/// second.
pub fn ask_to_set_cursor(window: Hwnd) -> bool {
    if window.is_null() {
        return false;
    }
    unsafe { SendMessageW(window, WM_SETCURSOR, window as usize, HTCLIENT) != 0 }
}

/// What the cursor actually looks like right now.
///
/// `None` when there is nothing to compare against, which is the usual case
/// and not a failure. The cursor is one shared resource for the whole desktop:
/// Windows hands it to whichever window the pointer is over, and a window that
/// sets it while the pointer is somewhere else is overruled on the next mouse
/// move. So this only answers when the pointer is genuinely inside `window`'s
/// client area, and when the cursor is visible at all -- a machine with no
/// mouse attached, which is a real thing on a build agent.
pub fn current_cursor(window: Hwnd) -> Option<Hwnd> {
    const CURSOR_SHOWING: c_ulong = 0x0001;
    if window.is_null() {
        return None;
    }
    let mut info = CursorInfo {
        size: std::mem::size_of::<CursorInfo>() as c_ulong,
        ..CursorInfo::default()
    };
    if unsafe { GetCursorInfo(&mut info) } == 0 || info.flags & CURSOR_SHOWING == 0 {
        return None;
    }
    // The pointer's position comes back in screen coordinates; the client
    // rectangle is relative to the window, so the window's own origin is what
    // puts them in the same space.
    let mut client = Rect::default();
    let mut origin = Point::default();
    if unsafe { GetClientRect(window, &mut client) } == 0
        || unsafe { ClientToScreen(window, &mut origin) } == 0
    {
        return None;
    }
    let inside = info.x >= origin.x + client.left
        && info.x < origin.x + client.right
        && info.y >= origin.y + client.top
        && info.y < origin.y + client.bottom;
    if !inside {
        return None;
    }
    Some(info.cursor)
}

/// The system I-beam, which is what `SystemMouseCursor::Text` should become.
pub fn text_cursor() -> Hwnd {
    unsafe { LoadCursorW(std::ptr::null_mut(), IDC_IBEAM) }
}

/// Asks the window to close, as the close button does.
pub fn close(window: Hwnd) {
    if !window.is_null() {
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
    }
}

/// Whether the window is still there.
pub fn is_open(window: Hwnd) -> bool {
    !window.is_null() && unsafe { IsWindow(window) != 0 }
}

/// Whether the reader has asked for dark mode, read from the registry the same
/// way the host reads it.
///
/// Read here as well as in the host so the two can be compared: a settings
/// message that says "light" on a machine set to dark is a bug in the host's
/// reader, and nothing inside the framework could tell.
pub fn prefers_dark_theme() -> Option<bool> {
    let subkey: Vec<u16> =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
            .encode_utf16()
            .collect();
    let value: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
    let mut light: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut light as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    // A machine too old to have the value has never heard of dark mode.
    Some(result == ERROR_SUCCESS && light == 0)
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
