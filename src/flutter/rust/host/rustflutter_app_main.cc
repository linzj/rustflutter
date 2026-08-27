// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/rust/host/rustflutter_host.h"

#include <cstddef>

namespace {

// The application's entry point, from rf_set_app_main.
//
// Compiled on every platform even though only the Android host reads it, so
// that what registers it -- a load-time initialiser in the application, which
// does not know which host it was built for -- is the same code everywhere.
//
// Not atomic and not locked, for the same reason the interface table is not:
// it is written by an initialiser, before the process has a second thread, and
// read from the platform thread long afterwards.
RfAppMain g_app_main = nullptr;

}  // namespace

void rf_set_app_main(RfAppMain app_main) {
  g_app_main = app_main;
}

RfAppMain rf_app_main() {
  return g_app_main;
}
