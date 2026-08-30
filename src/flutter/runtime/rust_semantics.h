// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUNTIME_RUST_SEMANTICS_H_
#define FLUTTER_RUNTIME_RUST_SEMANTICS_H_

#include <cstddef>

#include "flutter/lib/ui/semantics/semantics_node.h"
#include "flutter/runtime/rust_app_api.h"

namespace flutter {

//------------------------------------------------------------------------------
/// Copies one frame's semantics tree out of the C ABI and into the engine's
/// own nodes.
///
/// This is the second half of a pair. The framework fills in RfSemanticsNode
/// (`PackedSemantics::of` in rust/rustflutter/src/app.rs); this reads it back
/// out. Both are long stretches of field-by-field copying, which is the one
/// shape of code where a value can be read off the wrong node, or off the
/// right node and written into the wrong slot, while every part in isolation
/// stays correct.
///
/// # Why it is a free function, and why it has its own header
///
/// It never needed the controller: it reads its argument and returns a value.
/// Living inside RuntimeController::OnUpdateSemantics made it unreachable, and
/// reaching it would have meant standing up a RuntimeController and a
/// RuntimeDelegate to test forty lines that touch neither.
///
/// The header is separate for the same practical reason rather than for
/// tidiness. runtime_controller.h reaches runtime_delegate.h and from there
/// the font collection, which a test binary that only wants to convert some
/// structs has no business linking. What the conversion actually needs is two
/// headers, and those are the two above.
///
/// `nodes` may be null when `count` is zero -- a frame in which everything
/// went away is an empty tree, not an absent one.
SemanticsNodeUpdates RustSemanticsNodesToUpdates(const RfSemanticsNode* nodes,
                                                 size_t count);

}  // namespace flutter

#endif  // FLUTTER_RUNTIME_RUST_SEMANTICS_H_
