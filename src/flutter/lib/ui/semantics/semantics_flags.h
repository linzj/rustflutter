// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_LIB_UI_SEMANTICS_SEMANTICS_FLAGS_H_
#define FLUTTER_LIB_UI_SEMANTICS_SEMANTICS_FLAGS_H_

#include <cstdint>

namespace flutter {

enum class SemanticsTristate : int32_t {
  kNone = 0,
  kTrue = 1,
  kFalse = 2,
};
enum class SemanticsCheckState : int32_t {
  kNone = 0,
  kTrue = 1,
  kFalse = 2,
  kMixed = 3,
};

struct SemanticsFlags {
  SemanticsCheckState isChecked = SemanticsCheckState::kNone;
  SemanticsTristate isSelected = SemanticsTristate::kNone;
  SemanticsTristate isEnabled = SemanticsTristate::kNone;
  SemanticsTristate isToggled = SemanticsTristate::kNone;
  SemanticsTristate isExpanded = SemanticsTristate::kNone;
  SemanticsTristate isRequired = SemanticsTristate::kNone;
  SemanticsTristate isFocused = SemanticsTristate::kNone;
  bool isButton = false;
  bool isTextField = false;
  bool isInMutuallyExclusiveGroup = false;
  bool isHeader = false;
  bool isObscured = false;
  bool scopesRoute = false;
  bool namesRoute = false;
  bool isHidden = false;
  bool isImage = false;
  bool isLiveRegion = false;
  bool hasImplicitScrolling = false;
  bool isMultiline = false;
  bool isReadOnly = false;
  bool isLink = false;
  bool isSlider = false;
  bool isKeyboardKey = false;
  bool isAccessibilityFocusBlocked = false;
};

}  // namespace flutter

#endif  // FLUTTER_LIB_UI_SEMANTICS_SEMANTICS_FLAGS_H_
