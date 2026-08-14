// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Shim entry point for the rustflutter CLI. See the comment on
// rustflutter_cli_main in src/main.rs for why the tool is a staticlib.

extern "C" int rustflutter_cli_main();

int main() {
  return rustflutter_cli_main();
}
