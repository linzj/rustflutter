/// A `StreamController` whose `onListen` feeds the stream and closes it.
///
/// `LicenseRegistry.licenses` is that shape: `late final controller =
/// StreamController(onListen: () async { for (collector in _collectors)
/// await controller.addStream(collector()); await controller.close(); })`,
/// returned as `controller.stream` and folded by the licenses page. The
/// prelude's stream held only events known in advance, and there was no
/// controller at all ("cannot find type `StreamController`", 1 stub, and
/// the page's `fold` on it a second, ws1120).
///
/// `sync: true`, so that Dart delivers on the caller's stack as the prelude
/// does -- for events added *after* `listen`; Dart holds back what is
/// added inside `onListen` even then, so the licenses shape below is
/// listened to for the compile and the observed log comes from `drive`.
library;

import 'dart:async';

final List<String> log = <String>[];

/// The licenses shape.
Stream<int> feed() {
  late final StreamController<int> controller;
  controller = StreamController<int>(
    onListen: () {
      controller.add(1);
      controller.close();
    },
  );
  return controller.stream;
}

/// What a listener sees on the caller's stack.
void drive() {
  final StreamController<int> c = StreamController<int>(
    sync: true,
    onListen: () => log.add('listen'),
  );
  c.stream.listen((int v) => log.add('v$v'), onDone: () => log.add('done'));
  c.add(1);
  c.add(2);
  c.close();
}

String use() {
  feed().listen((int v) {});
  drive();
  log.add('after');
  return log.join(',');
}
