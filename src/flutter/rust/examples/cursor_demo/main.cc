// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Shim entry point.
//
// The executable is C++ because the engine it links against is C++; the app
// itself lives entirely in Rust. This file exists only to hand argc/argv to
// rustflutter_app_main and should not grow.

extern "C" int rustflutter_app_main(int argc, const char** argv);

int main(int argc, const char** argv) {
  return rustflutter_app_main(argc, argv);
}
