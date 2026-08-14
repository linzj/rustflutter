// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_LIB_UI_SEMANTICS_STRING_ATTRIBUTE_H_
#define FLUTTER_LIB_UI_SEMANTICS_STRING_ATTRIBUTE_H_

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace flutter {

struct StringAttribute;

using StringAttributePtr = std::shared_ptr<flutter::StringAttribute>;
using StringAttributes = std::vector<StringAttributePtr>;

// When adding a new StringAttributeType, the classes in these file must be
// updated as well.
//  * engine/src/flutter/lib/ui/semantics.dart
//  * engine/src/flutter/lib/web_ui/lib/semantics.dart
//  * engine/src/flutter/shell/platform/android/io/flutter/view/AccessibilityBridge.java
//  * engine/src/flutter/shell/platform/embedder/embedder.h
//  * engine/src/flutter/lib/web_ui/test/engine/semantics/semantics_api_test.dart
//  * engine/src/flutter/testing/dart/semantics_test.dart

enum class StringAttributeType : int32_t {
  kSpellOut,
  kLocale,
};

//------------------------------------------------------------------------------
/// The c++ representation of the StringAttribute, this struct serves as an
/// abstract interface for the subclasses and should not be used directly.
struct StringAttribute {
  virtual ~StringAttribute() = default;
  int32_t start = -1;
  int32_t end = -1;
  StringAttributeType type;
};

//------------------------------------------------------------------------------
/// Indicates the string needs to be spelled out character by character when the
/// assistive technologies announce the string.
struct SpellOutStringAttribute : StringAttribute {};

//------------------------------------------------------------------------------
/// Indicates the string needs to be treated as a specific language when the
/// assistive technologies announce the string.
struct LocaleStringAttribute : StringAttribute {
  std::string locale;
};

//------------------------------------------------------------------------------
/// Convenience constructors. Upstream these attributes were only ever built
/// from Dart via NativeStringAttribute; that peer class is gone, so the
/// framework layer constructs them directly.
inline StringAttributePtr MakeSpellOutAttribute(int32_t start, int32_t end) {
  auto attribute = std::make_shared<SpellOutStringAttribute>();
  attribute->start = start;
  attribute->end = end;
  attribute->type = StringAttributeType::kSpellOut;
  return attribute;
}

inline StringAttributePtr MakeLocaleAttribute(int32_t start,
                                              int32_t end,
                                              std::string locale) {
  auto attribute = std::make_shared<LocaleStringAttribute>();
  attribute->start = start;
  attribute->end = end;
  attribute->type = StringAttributeType::kLocale;
  attribute->locale = std::move(locale);
  return attribute;
}

}  // namespace flutter

#endif  // FLUTTER_LIB_UI_SEMANTICS_STRING_ATTRIBUTE_H_
