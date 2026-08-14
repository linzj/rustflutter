// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Staticlib wrapper around the `rustflutter` rlib.
//!
//! rustc only emits `extern "C"` symbols into a `staticlib`, not an `rlib`, so
//! C++ targets that just want the framework's exported symbols (the FFI unit
//! tests, for instance) link this rather than the rlib directly. Apps do not
//! need it: they are staticlibs themselves and re-export their own entry point.

pub use rustflutter::*;
