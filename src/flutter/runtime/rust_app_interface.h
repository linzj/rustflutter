// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUNTIME_RUST_APP_INTERFACE_H_
#define FLUTTER_RUNTIME_RUST_APP_INTERFACE_H_

#include "flutter/runtime/rust_app_api.h"

namespace flutter {

//------------------------------------------------------------------------------
/// @brief      The framework's function table, checked.
///
///             `rf_app_interface()` answers NULL before a framework registers;
///             this is the same question asked by code that is going to
///             dereference the answer, so it fails loudly instead. Every call
///             out of RuntimeController goes through here:
///
///                 RustApp().begin_frame(app_, micros, number);
///
///             which is deliberately the same shape as the calls that go the
///             other way, through RfAppHost.
///
/// @return     The registered table. Never returns if there is none.
///
const RfAppInterface& RustApp();

}  // namespace flutter

#endif  // FLUTTER_RUNTIME_RUST_APP_INTERFACE_H_
