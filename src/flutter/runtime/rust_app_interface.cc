// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/runtime/rust_app_api.h"

#include "flutter/fml/logging.h"

namespace {

// The framework's table, from rf_set_app_interface.
//
// One per process, like the framework it points into: the shell can hold
// several RuntimeControllers -- one per Shell -- but they all reach the same
// Rust crate, and it is the crate that registers.
//
// Not atomic and not locked. It is written once, from the platform thread,
// before the shell that will read it exists; every read is from the UI thread
// of a shell that was started afterwards, and starting a shell is itself the
// happens-before edge.
const RfAppInterface* g_app_interface = nullptr;

}  // namespace

void rf_set_app_interface(const RfAppInterface* app_interface) {
  g_app_interface = app_interface;
}

const RfAppInterface* rf_app_interface() {
  return g_app_interface;
}

namespace flutter {

const RfAppInterface& RustApp() {
  // A shell started with no framework registered. Upstream's equivalent is an
  // isolate that never got a snapshot, and it is fatal there too: everything
  // downstream of here dereferences the result, so the alternative is the same
  // crash without the sentence explaining it.
  FML_CHECK(g_app_interface != nullptr)
      << "No Rust framework is registered. The framework calls "
         "rf_set_app_interface on its way to rf_host_run; a shell started "
         "without going through rustflutter::run() has to call it itself.";
  return *g_app_interface;
}

}  // namespace flutter
