// `Completer<T?>.complete()` with the argument left off, in a generic
// abstract class -- `Route<T>._disposeCompleter`, which `Route.dispose`
// completes with no argument (Dart's `complete(null)`).
//
// The emitter wrote `complete(())` against a completer whose type argument
// had come out as `Option<Rc<dyn Object>>`: the omitted argument was lowered
// as `void`, not as the null of the slot the callee declares.
import 'dart:async';

abstract class Task<T> {
  final Completer<T?> _done = Completer<T?>();

  bool get isDone => _done.isCompleted;

  void finish() {
    _done.complete();
  }
}

class Job extends Task<String> {}

String use() {
  final Task<String> job = Job();
  final before = job.isDone;
  job.finish();
  return '$before/${job.isDone}';
}
