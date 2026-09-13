/// A member declared in terms of an erased class parameter, read through a
/// receiver whose class erased it, and handed back as the method's own `T`.
///
/// `SchedulerBinding.scheduleTask<T>` makes a `_TaskEntry<T>`, queues it in
/// a `List<_TaskEntry<dynamic>>` -- which erases `_TaskEntry`'s `T` -- and
/// returns `entry.completer.future`. The field reads at the erased
/// spelling (`Completer<Rc<dyn DartAny>>`), but `.future` on it was typed
/// by Kernel's substitution, `Future<scheduleTask.T>`, so no conversion
/// was asked for at the return: "expected `DartFuture<T>`, found
/// `DartFuture<Rc<dyn DartAny>>`" (2 stubs, the base's super function and
/// `WidgetsBinding`'s copy, since ws1098). A member whose declared type
/// *mentions* the erased parameter reads at the erased spelling too, and
/// the return then maps the future's value back into `T`.
library;

import 'dart:async';

class Entry<T> {
  Entry(this.completer, this.label);
  final Completer<T> completer;
  final String label;

  /// Completed from inside, as `_TaskEntry.run` does: through the erased
  /// receiver from outside, `complete(value)` is a slot of its own.
  void finish(T value) {
    completer.complete(value);
  }
}

class Runner {
  final List<Entry<Object?>> queue = <Entry<Object?>>[];

  Future<T> run<T>(T value, String label) {
    final Entry<T> entry = Entry<T>(Completer<T>(), label);
    queue.add(entry);
    entry.finish(value);
    return entry.completer.future;
  }
}

String use() {
  final Runner r = Runner();
  r.run<int>(3, 'three');
  r.run<String>('s', 'ess');
  // The futures resolve later; the calls are the compile's pin.
  return '${r.queue.length}:${r.queue.map((Entry<Object?> e) => e.label).join(',')}';
}
