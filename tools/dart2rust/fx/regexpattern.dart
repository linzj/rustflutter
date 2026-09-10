// `String.replaceAll(Pattern, String)` where the pattern is a `RegExp`.
//
// Dart's `Pattern` is a `String` or a `RegExp` and `dart:core` dispatches on
// which one arrived; Rust has no such union, so the prelude's
// `DartString::replace_all` takes a `String` and only a `String`.
// `_DateFormatQuotedField._patchQuotes` passes a `RegExp` and the call came
// out as `s.replace_all(re, "'")` -- "expected `String`, found `RegExp`" --
// which stubbed the function.
//
// The regular expression's own `replace_all_in` is the same member for a
// `RegExp` pattern, written over `all_matches` so both halves agree on what
// a match is. What this fixture pins is that the two really do compute the
// same string as Dart: overlapping candidates, a match at either end, a
// replacement longer than what it replaces, and a pattern that matches
// nothing at all.

final RegExp _twoQuotes = RegExp("''");
final RegExp _digits = RegExp(r'[0-9]+');

String use() {
  // The intl shape: a static final `RegExp` read as the pattern.
  final String quoted = "''ab''cd''";
  final String patched = quoted.replaceAll(_twoQuotes, "'");

  // A run replaced by something longer, and one at each end.
  final String runs = '12ab345cd6';
  final String widened = runs.replaceAll(_digits, '<n>');

  // A pattern that never matches leaves the string alone.
  final String untouched = 'abc'.replaceAll(_digits, '!');

  // Three quotes: the first two match and the third is left, which is where
  // an overlapping scan and a left-to-right one part company.
  final String odd = "a'''b".replaceAll(_twoQuotes, '-');

  // The empty string, and a string that is nothing but matches.
  final String empty = ''.replaceAll(_digits, 'x');
  final String all = '999'.replaceAll(_digits, '');

  // A string pattern still goes the other way, through `DartString`.
  final String plain = 'a.b.c'.replaceAll('.', '/');

  return '$patched|$widened|$untouched|$odd|$empty|$all|$plain';
}
