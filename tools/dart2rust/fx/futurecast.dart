// A `Future<T>` out of a generic class that was erased to its bound.
//
// `SchedulerBinding.scheduleTask<T>` makes a `_TaskEntry<T>`, whose
// `completer` is a `Completer<T>`, and returns `entry.completer.future`.
// `_TaskEntry<T>` is erased here, so the completer is a
// `Completer<Object>` and the read is a `DartFuture<Rc<dyn DartAny>>`
// where `DartFuture<T>` was declared.

import 'dart:async';

typedef TaskCallback<T> = Future<T> Function();

class Entry<T> {
  Entry(this.label, this.task);
  final String label;
  final TaskCallback<T> task;
  final Completer<T> completer = Completer<T>();
}

Future<T> schedule<T>(String label, T value) {
  final Entry<T> entry = Entry<T>(label, () async => value);
  entry.completer.complete(value);
  return entry.completer.future;
}

String use() {
  String out = '';
  schedule<int>('a', 7).then((int v) {
    out = 'int/$v';
  });
  return out;
}
