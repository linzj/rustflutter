/// The prelude's own throws are Dart's, and Dart catches them.
///
/// `int.parse('x')` is a `FormatException`, `{}.first` is a `StateError`,
/// `x << -1` is an `ArgumentError`, and `[1, 2].single` is a `StateError`.
/// Each was a `panic!("uncaught Dart exception: ..")` inside the prelude --
/// 32 of them, none ever touched (work.md step 6). A panic there is a path
/// the program still had and could not take.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

String parseInt(String text) {
  try {
    return '${int.parse(text)}';
  } on FormatException catch (_) {
    return 'not-an-int';
  }
}

String parseDouble(String text) {
  try {
    return '${double.parse(text)}';
  } on FormatException catch (_) {
    return 'not-a-double';
  }
}

String shift(int value, int by) {
  try {
    return '${value << by}';
  } on ArgumentError catch (_) {
    return 'bad-shift';
  }
}

String firstOf(Set<int> xs) {
  try {
    return '${xs.first}';
  } on StateError catch (_) {
    return 'no-element';
  }
}

String singleOf(Set<int> xs) {
  try {
    return '${xs.single}';
  } on StateError catch (_) {
    return 'not-one';
  }
}

String use() {
  final out = <String>[];
  out.add(parseInt('42'));
  out.add(parseInt('forty-two'));
  out.add(parseDouble('1.5'));
  out.add(parseDouble('one point five'));
  out.add(shift(1, 3));
  out.add(shift(1, -1));
  out.add(firstOf(<int>{7, 8}));
  out.add(firstOf(<int>{}));
  out.add(singleOf(<int>{9}));
  out.add(singleOf(<int>{9, 10}));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
