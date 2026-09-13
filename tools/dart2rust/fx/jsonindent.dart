/// `const JsonEncoder.withIndent('  ').convert(map)`.
///
/// `Platform.toJson` (package:platform) writes itself that way. A const
/// instance of a class outside the file is refused unless the prelude
/// knows it (`_constInstance`), and `JsonEncoder` was not one of those:
/// "unsupported const instance of `JsonEncoder`, which is not in this
/// file" (1 refusal at ws1122). The prelude's encoder lays out an object
/// or array with members one member per line, indented by the string
/// given per level, an empty one as `{}` / `[]`, and `: ` after a key --
/// the layout Dart's `JsonEncoder.withIndent` writes.
library;

import 'dart:convert';

String use() {
  final Map<String, dynamic> value = <String, dynamic>{
    'name': 'gallery',
    'count': 3,
    'ratio': 0.5,
    'flags': <dynamic>[true, false, null],
    'empty': <dynamic>[],
    'nothing': <String, dynamic>{},
    'nested': <String, dynamic>{
      'quote': 'a "b" \\ c',
      'inner': <dynamic>[
        1,
        <String, dynamic>{'k': 'v'},
      ],
    },
  };
  final String pretty = const JsonEncoder.withIndent('  ').convert(value);
  final String compact = const JsonEncoder().convert(value);
  // One line: the harness reads the last line, so the newlines the layout
  // is made of are shown, not printed.
  return '${pretty.replaceAll('\n', r'\n')}|$compact';
}
