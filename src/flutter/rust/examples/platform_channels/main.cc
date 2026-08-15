// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Shim entry point. See hello_world/main.cc.

extern "C" int rustflutter_app_main(int argc, const char** argv);

int main(int argc, const char** argv) {
  return rustflutter_app_main(argc, argv);
}
