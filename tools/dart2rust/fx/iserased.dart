// `x is T` where `T` is a class type parameter the compiler erased.
//
// `NotificationListener<T>`'s element asks exactly this, and with `T` erased
// to its bound the test is always true: a `ScrollMetricsNotification` walked
// into a listener that only takes `ScrollNotification` and `EditableText`
// unwrapped a `None` (run903).
abstract class Note {
  String get label;
}

class ScrollNote extends Note {
  @override
  String get label => 'scroll';
}

class MetricsNote extends Note {
  @override
  String get label => 'metrics';
}

class Sink<T extends Note> {
  bool accepts(Note note) => note is T;
}

String use() {
  final Sink<ScrollNote> sink = Sink<ScrollNote>();
  final yes = sink.accepts(ScrollNote());
  final no = sink.accepts(MetricsNote());
  return '$yes/$no';
}
