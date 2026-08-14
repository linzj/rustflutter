// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! rustflutter FFI layer.
//!
//! This crate is where the Rust side of the engine boundary lives. Upstream,
//! that boundary is the 231 bindings registered in `lib/ui/dart_ui.cc` plus the
//! 20 `tonic::DartPersistentValue` callbacks held by `PlatformConfiguration`;
//! both are replaced here by a plain `extern "C"` ABI over the same C++ objects.
//!
//! M0 scope: prove that a Rust staticlib links into the engine's C++ build and
//! that calls cross the boundary. Nothing here touches the engine yet -- the
//! first real binding is M1's `LayerTree` construction.

use std::os::raw::{c_char, c_int};

/// Version string for the Rust side, as a NUL-terminated C string.
///
/// The returned pointer has static lifetime; the caller must not free it.
#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_version() -> *const c_char {
    // Built by GN via rustc directly, not cargo, so there is no CARGO_PKG_*
    // to read here. The NUL is embedded so this stays allocation-free.
    "0.1.0-m0\0".as_ptr() as *const c_char
}

/// Round-trip smoke check: returns `value + 1`, wrapping at the boundary.
///
/// Exists so the M0 build has something that observably *executes* Rust code
/// rather than merely linking it.
#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_smoke_increment(value: c_int) -> c_int {
    value.wrapping_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_wraps_instead_of_panicking() {
        assert_eq!(rustflutter_smoke_increment(41), 42);
        assert_eq!(rustflutter_smoke_increment(c_int::MAX), c_int::MIN);
    }
}
