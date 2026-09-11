/// Reading a `late` before it is written throws, and Dart catches it.
///
/// `late int n;` read before assignment throws `LateInitializationError` --
/// an `Error`, and `try { .. } catch (e) { .. }` around it is ordinary Dart.
/// This compiler lowered every `late` read to `.unwrap()` on the `Option`
/// that holds the field, which is a *panic*: the program lost a path it
/// still had, and the process simply died where Dart would have kept going.
///
/// The rule this fixture belongs to: **a panic is never a pass.** A Dart
/// `throw` is a `Result` on this side, so a Rust panic means a translated
/// program cannot do something the Dart one can.
///
/// Three shapes, because they are three emission points: a field read
/// through the struct, a field read through a *trait accessor* (the base
/// class's field seen from a subtype, `emit_impl.dart`), and a local.
library;

class Box {
  late int n;
  late final String s;

  int readN() => n;
  String readS() => s;
}

abstract class Holder {
  late int slot;
}

class Held extends Holder {
  int through() => slot;
}

String field() {
  final b = Box();
  try {
    return 'n=${b.readN()}';
  } catch (_) {
    return 'n-unset';
  }
}

String finalField() {
  final b = Box();
  final out = <String>[];
  try {
    out.add(b.readS());
  } catch (_) {
    out.add('s-unset');
  }
  b.s = 'written';
  out.add(b.readS());
  return out.join(',');
}

String accessor() {
  final h = Held();
  final out = <String>[];
  try {
    out.add('${h.through()}');
  } catch (_) {
    out.add('slot-unset');
  }
  h.slot = 7;
  out.add('${h.through()}');
  return out.join(',');
}

String local(bool write) {
  // Read by a closure, which is how a `late` local comes to be held in a
  // cell -- and `if (write)` is there because Dart *refuses to compile* a
  // read of a local it can prove is unassigned. Only a local it cannot
  // prove either way can throw, which is the case this fixture wants.
  late int x;
  int read() => x + 1;
  final out = <String>[];
  if (write) x = 41;
  try {
    out.add('${read()}');
  } catch (_) {
    out.add('x-unset');
  }
  x = 1;
  out.add('${read()}');
  return out.join(',');
}

String use() {
  final out = <String>[];
  out.add(field());
  out.add(finalField());
  out.add(accessor());
  out.add(local(false));
  out.add(local(true));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
