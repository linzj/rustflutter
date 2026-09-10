// A class that *implements* `Future<T>`, awaited.
//
// Dart lets any class be a future by implementing `Future<T>`, and `await x`
// on one is `x.then(..)`. The prelude's future is the *struct*
// `DartFuture<T>`, not a trait, so `implements Future` cannot become a
// supertrait and the interface was dropped: `Rc<Signal>` is no Rust future
// at all ("`Rc<Signal>` is not a future", E0277 -- rustc's own note says it
// "must implement `IntoFuture` to be awaited").
//
// So such a class gets an inherent `dart_into_future` backed by its own
// `then`, and the await site calls it. Inherent rather than
// `impl IntoFuture for Rc<Self>`: `Rc` is not `#[fundamental]`, so that impl
// is not this crate's to write, and the awaited value is always a handle.
//
// `TickerFuture implements Future<void>` is the gallery's: `await
// controller.forward()` and `await controller?.reverse()` are ordinary Dart
// on the animation path (`_MagnifierState.show`/`hide`,
// `_ShrineAppState._onWillPop`).
//
// Nothing is logged before the `await`. A Dart `async` call runs its body up
// to the first await straight away and a Rust `async fn` runs nothing until
// it is polled, so anything written there would differ for a reason that has
// nothing to do with this rule.
import 'dart:async';

class Signal implements Future<void> {
  Signal() : _completer = Completer<void>();

  final Completer<void> _completer;

  void fire() {
    _completer.complete();
  }

  @override
  Stream<void> asStream() => _completer.future.asStream();

  @override
  Future<void> catchError(Function onError, {bool Function(Object)? test}) =>
      _completer.future.catchError(onError, test: test);

  @override
  Future<R> then<R>(FutureOr<R> Function(void) onValue, {Function? onError}) =>
      _completer.future.then<R>(onValue, onError: onError);

  @override
  Future<void> timeout(
    Duration timeLimit, {
    FutureOr<void> Function()? onTimeout,
  }) => _completer.future.timeout(timeLimit, onTimeout: onTimeout);

  @override
  Future<void> whenComplete(FutureOr<void> Function() action) =>
      _completer.future.whenComplete(action);
}

// A library-level list rather than a lent parameter: a `List<T>` parameter
// is lent as `&mut Vec<T>`, and a reference cannot outlive the function it
// was lent to once an `async` body holds it across an await ("borrowed data
// escapes outside of function", E0521). That is the aliasing project under
// 已知欠账, not this rule.
final List<String> _log = <String>[];

Future<void> waitPlain(Signal signal) async {
  await signal;
  _log.add('plain');
}

// ..and the nullable spelling, which is asked before it is awaited.
Future<void> waitMaybe(Signal? signal) async {
  await signal;
  _log.add('maybe');
}

String use() {
  final Signal signal = Signal();
  waitPlain(signal);
  waitMaybe(signal);
  waitMaybe(null);
  _log.add('sync');
  signal.fire();
  _log.add('fired');
  return _log.join(',');
}
