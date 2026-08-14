// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <cstring>

#include "flutter/testing/testing.h"

// Declared by //flutter/rust:ffi (flutter/rust/ffi/src/lib.rs). Kept hand
// written for M0; once the FFI surface grows past a handful of symbols this
// header should be generated (cbindgen) so the two sides cannot drift.
extern "C" {
const char* rustflutter_version();
int rustflutter_smoke_increment(int value);
}

namespace flutter {
namespace testing {

TEST(RustFFI, CallsIntoRustAndGetsAValueBack) {
  EXPECT_EQ(rustflutter_smoke_increment(41), 42);
}

TEST(RustFFI, ReturnsAReadableStaticString) {
  const char* version = rustflutter_version();
  ASSERT_NE(version, nullptr);
  EXPECT_STREQ(version, "0.1.0-m0");
}

}  // namespace testing
}  // namespace flutter
