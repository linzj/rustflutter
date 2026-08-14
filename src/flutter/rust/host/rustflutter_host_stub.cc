// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Placeholder host for platforms whose window layer is not written yet.
//
// Everything above rf_host_run -- Shell, ThreadHost, Animator, Rasterizer, the
// software surface -- is portable; what is missing per platform is a window and
// a message loop. See rustflutter_host_win.cc for the shape a port takes.

#include "flutter/rust/host/rustflutter_host.h"

int32_t rf_host_run(const RfHostOptions* options) {
  return -100;
}
