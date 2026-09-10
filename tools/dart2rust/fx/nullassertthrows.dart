/// `x!` on a null throws, and Dart catches it.
///
/// Dart's null-assert raises `TypeError` with the message "Null check
/// operator used on a null value", and `on TypeError catch` around one is
/// ordinary Dart. This compiler had the *message* right and the behaviour
/// wrong: `dart_null_check_failed` panicked, and an `x!` on a local lowered
/// to `Option::unwrap`. Either way the program lost a path it still had.
///
/// The rule this belongs to: **a panic is never a pass.** A Dart `throw` is
/// a `Result` on this side.
library;

class Box {
  Box(this.value);
  int? value;
}

String read(int? x) {
  try {
    return 'got ${x!}';
  } on TypeError catch (_) {
    return 'null';
  }
}

String readField(Box b) {
  try {
    return 'field ${b.value!}';
  } on TypeError catch (_) {
    return 'field null';
  }
}

String chained(List<int?> xs, int i) {
  try {
    return 'at$i=${xs[i]!}';
  } on TypeError catch (_) {
    return 'at$i=null';
  }
}

String use() {
  final List<String> out = <String>[];
  out.add(read(5));
  out.add(read(null));
  out.add(readField(Box(7)));
  out.add(readField(Box(null)));
  out.add(chained(<int?>[1, null, 3], 0));
  out.add(chained(<int?>[1, null, 3], 1));
  // The throw does not stop the program, which is the whole point.
  out.add('alive');
  return out.join('|');
}
