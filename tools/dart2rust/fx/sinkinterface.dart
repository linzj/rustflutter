/// A class that `implements Sink<T>`, handed to something that takes one.
///
/// `Sink<T>` is `Rc<dyn DartSink<T>>` here, and a translated class that
/// implements it got no `DartSink` impl at all:
///
///     the trait bound `DigestSink: DartSink<Digest>` is not satisfied
///
/// `crypto`'s `Hash.convert` is the shape: it makes a `DigestSink`, hands
/// it to `startChunkedConversion(Sink<Digest>)`, adds to the sink it gets
/// back and closes it (2 stubs at ws1112).
library;

class Digest {
  Digest(this.n);

  final int n;

  @override
  String toString() => 'd$n';
}

/// The class under test.
class Collect implements Sink<Digest> {
  final List<Digest> seen = <Digest>[];

  bool closed = false;

  @override
  void add(Digest d) {
    seen.add(d);
  }

  @override
  void close() {
    closed = true;
  }
}

/// ..and one whose `add` throws, so the forwarding impl's error channel is
/// the one Dart's is. With a field of its own, like the gallery's two: a
/// field-less class is a value struct here and reaching a trait handle
/// from one is a separate gap, not this fixture's subject.
class Refuses implements Sink<Digest> {
  int tried = 0;

  @override
  void add(Digest d) {
    tried += 1;
    throw StateError('no room for ${d.n}');
  }

  @override
  void close() {}
}

String fill(Sink<Digest> into, List<int> ns) {
  try {
    for (final int n in ns) {
      into.add(Digest(n));
    }
    into.close();
    return 'filled';
  } on StateError catch (e) {
    return 'caught ${e.message}';
  }
}

String use() {
  final Collect c = Collect();
  final List<String> out = <String>[];
  out.add(fill(c, <int>[1, 2, 3]));
  out.add(c.seen.join(','));
  out.add('${c.closed}');
  out.add(fill(Refuses(), <int>[7]));
  return out.join('|');
}
