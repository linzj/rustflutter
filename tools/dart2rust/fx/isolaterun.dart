// `Isolate.run(computation)`. There is one isolate here, so the
// computation is spawned on this one -- what `Future(computation)` already
// is, and the callback has the same `FutureOr<R> Function()` shape.
//
// It could not be written as a `run` on the prelude's `Isolate<T>`: that
// is an unrelated wrapper for a `static` which happens to share the name
// `dart:isolate` uses, and `Isolate::run` had no `T` to infer ("no
// associated function named `run` found for struct `Isolate<_>`").
//
// `foundation`'s `compute()` is a call to it.
import 'dart:isolate';

Future<int> squared(int n) => Isolate.run(() => n * n);

Future<String> named(String s) => Isolate.run(() => s.toUpperCase());

String use() {
  // A synchronous fixture cannot drive the event loop, so what is checked
  // is that the call is made and hands back a future of the right type.
  final Future<int> six = squared(6);
  final Future<String> hi = named('hi');
  return '${six is Future<int>}/${hi is Future<String>}';
}
