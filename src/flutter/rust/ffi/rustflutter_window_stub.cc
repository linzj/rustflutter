// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Presenter stub for platforms that do not have one yet. See
// rustflutter_window_win.cc for the Windows implementation and for why this
// layer is a stopgap.

#include "flutter/rust/ffi/rustflutter_ffi.h"

int32_t rf_window_show(int32_t width,
                       int32_t height,
                       const uint8_t* bgra,
                       size_t bgra_len,
                       const char* title) {
  (void)width;
  (void)height;
  (void)bgra;
  (void)bgra_len;
  (void)title;
  return -100;  // Unimplemented on this platform.
}
