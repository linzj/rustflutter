/// A generic method called on a trait object with a callback typed at the
/// call's own `T`.
///
/// `SchedulerBinding.instance.scheduleTask<List<LicenseParagraph>>(
/// license.paragraphs.toList, Priority.animation, ..)`: the receiver is
/// the binding's trait object, so the call goes to the method's erased
/// twin, whose callback slot is `Fn() -> FutureOr<Rc<dyn DartAny>>`. The
/// tear-off had been adapted to `Fn() -> FutureOr<Vec<LicenseParagraph>>`
/// -- the call's `T` -- and the twin refused it ("expected `Rc<dyn
/// DartAny>`, found `Vec<LicenseParagraph>`", 1 stub at ws1119). The
/// twin's arguments go through the coercion at the twin's own slots, and
/// a `FutureOr<A>` into a `FutureOr<B>` maps each case.
library;

import 'dart:async';

class Para {
  Para(this.text);
  final String text;
}

class Entry {
  Entry(this.paras);
  final List<Para> paras;

  Iterable<Para> get paragraphs => paras.where((Para p) => p.text.isNotEmpty);
}

typedef Task<T> = FutureOr<T> Function();

abstract class Runner {
  /// Runs `task`, whatever `T` is. `void`, not `Future<T>`: a Rust future
  /// runs nothing until polled where Dart's body runs to its first
  /// `await`, and `Future<T>.value(FutureOr<T>)` is a question of its
  /// own. What is pinned here is the callback's slot.
  void run<T>(Task<T> task, {String? label});
}

class Immediate extends Runner {
  final List<String> log = <String>[];

  @override
  void run<T>(Task<T> task, {String? label}) {
    log.add(label ?? '-');
    task();
  }
}

Runner? _runner;

/// The way `SchedulerBinding.instance` is reached: a static typed by the
/// abstract class, so every call on it is on the trait object.
Runner get runner => _runner ??= Immediate();

/// Only what runs before the first `await` is observed: the label each
/// call logs, and the second task's own mark. The first call is the
/// tear-off shape; it is the compile that pins it.
String use() {
  final Entry e = Entry(<Para>[Para('a'), Para(''), Para('b')]);
  final Immediate r = runner as Immediate;
  runner.run<List<Para>>(e.paragraphs.toList, label: 'paras');
  runner.run<int>(() {
    r.log.add('counted');
    return 3;
  }, label: 'count');
  return r.log.join(',');
}
