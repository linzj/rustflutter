/// `m[k]!` as a *read*: a key that is not there throws.
///
/// Dart's `m[k]` answers null for an absent key and `!` turns that null
/// into a `TypeError` a program can catch. This compiler wrote
/// `.clone().unwrap()` for it (`places.dart:282`), a panic -- 34 across
/// the gallery (work.md step 3). `bangplace` is the same operator on the
/// *left* of an assignment; this is the read.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

class Settings {
  final Map<String, int> counts = <String, int>{'a': 1};
  int? missing;
}

String read(Map<String, int> m, String k) {
  try {
    return 'got:${m[k]!}';
  } on TypeError catch (_) {
    return 'absent:$k';
  }
}

String field(Settings s) {
  try {
    return 'field:${s.missing!}';
  } on TypeError catch (_) {
    return 'field-null';
  }
}

String use() {
  final out = <String>[];
  final s = Settings();
  out.add(read(s.counts, 'a'));
  out.add(read(s.counts, 'b'));
  out.add(field(s));
  s.missing = 5;
  out.add(field(s));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
