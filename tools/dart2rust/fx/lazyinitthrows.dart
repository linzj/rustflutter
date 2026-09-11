/// A lazy `late final` field whose initialiser throws, read through the
/// interface the class implements.
///
/// That read is emitted twice: once as the class's own accessor, and once
/// as the trait slot `implements Holder` asks for. The slot's forwarder
/// returns `Result` -- it is written `Ok(..)` -- but the expression inside
/// it was emitted with no `_failure` in scope, so the initialiser's own
/// failure came out as `.unwrap()`: 235 of them across the gallery
/// (work.md step 1). Dart re-runs the initialiser on the next read after
/// one throws, and catches what it threw; a panic takes the process.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

abstract class Holder {
  List<String> get value;
  void flip();
}

class Boom implements Holder {
  Boom(this.fail);

  bool fail;

  // Through a call, so the initialiser the emitter writes has to carry
  // the callee's `Result` out with a `?` rather than unwrap it.
  @override
  late final List<String> value = _make();

  List<String> _make() {
    if (fail) throw StateError('no value');
    return <String>['seven'];
  }

  @override
  void flip() {
    fail = !fail;
  }
}

String read(Holder h) {
  try {
    return 'got:${h.value.join(",")}';
  } on StateError catch (e) {
    return 'caught:${e.message}';
  }
}

String use() {
  final out = <String>[];
  final holders = <Holder>[Boom(false), Boom(true)];
  out.add(read(holders[0]));
  out.add(read(holders[1]));
  // ..a second read of the same object runs the initialiser again, which
  // is what Dart does after one throws.
  out.add(read(holders[1]));
  holders[1].flip();
  out.add(read(holders[1]));
  out.add('alive');
  return out.join('|');
}
