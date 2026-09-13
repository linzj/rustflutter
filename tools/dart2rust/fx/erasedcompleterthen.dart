/// `then` on the future of an erased class's `Completer<T?>` field, with a
/// callback that takes `dynamic`.
///
/// `Route<T>._disposeCompleter` is a `Completer<T?>` and `T` is erased,
/// so the field is a `Completer<Option<Rc<dyn DartAny>>>`; `_RouteEntry.
/// handleDidPopNext` calls `poppedRoute._disposeCompleter.future.then(
/// (dynamic result) async {..})`. The read of `.future` carried no type
/// (its declared `Future<T>` only *mentions* the erased `T`, and what the
/// receiver put there is the nullable `dynamic`), so the callback was
/// typed by Kernel -- `Rc<dyn DartAny>` -- against a future of
/// `Option<Rc<dyn DartAny>>`: "expected closure signature
/// `fn(Option<Rc<_>>)`" (1 stub since ws1098).
library;

import 'dart:async';

final List<String> log = <String>[];

class Box<T> {
  Box(this.label);
  final String label;
  final Completer<T?> done = Completer<T?>();

  /// Completed from inside, as `Route.didPop` completes its own: through
  /// the erased receiver from outside, `complete(value)` is a slot of its
  /// own.
  void finish(T? value) {
    done.complete(value);
  }
}

/// The wider slot every box reaches: `T` erased. `dynamic`, as
/// `poppedRoute` is a `Route<dynamic>`: Kernel folds the field's `T?`
/// to `dynamic` there, and the future the field holds is mapped into
/// that on the way to `then`.
void watch(Box<dynamic> box) {
  log.add('watch:${box.label}');
  box.done.future.then((dynamic result) {
    log.add('done:${box.label}');
  });
}

/// Every box kept as a `Box<Object?>`, which is what erases `Box`'s `T`
/// (a `Route<void>` is kept as a `Route<dynamic>` by the navigator).
final List<Box<Object?>> all = <Box<Object?>>[];

String use() {
  final Box<int> a = Box<int>('a');
  final Box<String> b = Box<String>('b');
  all.add(a);
  all.add(b);
  watch(a);
  watch(b);
  a.finish(3);
  b.finish(null);
  // The callbacks run later; the calls are the compile's pin.
  return '${log.join(',')}|${all.length}';
}
