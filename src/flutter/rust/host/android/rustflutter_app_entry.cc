// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Hands the application's entry point to the host, at load time.
//
// On a desktop the application owns `main` and calls `rustflutter_app_main`
// itself; there is nothing to register and this file is not built. On Android
// the Activity owns the process and the host decides when the application
// starts -- when there is a Surface -- so the host has to be able to reach the
// entry point. It used to declare it and call it by name, which stops working
// the moment the host is inside a shared library the application links against:
// the call then points out of the library and up into its consumer.
//
// So the application pushes the pointer down, the same way it pushes its
// framework table down through rf_set_app_interface, and this is where that
// happens for an application built by GN. A Cargo application registers from
// the framework crate instead, which has an `.init_array` entry of its own for
// exactly this -- see `android_entry` in app.rs.
//
// This is a source_set, so its object goes on the link line directly rather
// than into an archive the linker is free to skip. An initialiser nothing
// references would not survive being archived.

#include "flutter/rust/host/rustflutter_host.h"

extern "C" {
// The application's own entry point. Declared rather than included, because
// this must not depend on any one application.
int32_t rustflutter_app_main(int32_t argc, const char** argv);
}

namespace {

// Runs when the library is loaded, which on Android is `System.loadLibrary`
// from the Activity's onCreate -- long before the Surface exists, and so long
// before anything asks for what it registers.
struct RegisterAppMain {
  RegisterAppMain() { rf_set_app_main(&rustflutter_app_main); }
};

RegisterAppMain g_register;

}  // namespace
