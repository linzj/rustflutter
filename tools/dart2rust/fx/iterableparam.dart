// A method that takes `Iterable<T>`, fed the three things Dart lets you feed
// it. `Iterable<T>` is `Rc<dyn DartIterable<T>>` since ws908, so each of
// these has to reach the parameter as the trait handle.
import 'dart:collection';

class Collector {
  final List<String> seen = <String>[];

  void take(Iterable<String> items) {
    for (final item in items) {
      seen.add(item);
    }
  }

  int count(Iterable<String> items) => items.length;
}

String use() {
  final collector = Collector();
  collector.take(<String>['a', 'b']);
  collector.take(<String>{'c'});
  final queue = Queue<String>();
  queue.add('d');
  collector.take(queue);
  final n = collector.count(<String>['x', 'y', 'z']);
  return '${collector.seen.join("-")}/$n';
}
