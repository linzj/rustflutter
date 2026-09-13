/// `ByteConversionSink.from(sink)`, on a sink this program wrote.
///
/// In `dart:convert` that is a *redirecting* factory --
/// `factory ByteConversionSink.from(Sink<List<int>> sink) = _ByteAdapterSink;`
/// -- and the dill spells a call to it as its target, `_ByteAdapterSink(sink)`.
/// A private `dart:` class is constructed here as the public type it
/// implements, and that dropped the class's own name: `withCallback`'s
/// `_ByteCallbackSink()` and `from`'s `_ByteAdapterSink()` both became the
/// one prelude constructor, and the sink went where a callback was wanted:
///
///     expected an `Fn(Vec<i64>)` closure, found `_Sha256Sink`
///
/// `Sha256.startChunkedConversion` is the shape (1 stub at ws1114). Here
/// `ByteConversionSink` *is* `Sink<List<int>>` -- the same handle under
/// another name -- so `from` hands the sink back, and what was added
/// through the adapter is what the sink saw.
library;

import 'dart:convert';

class Bytes implements Sink<List<int>> {
  final List<int> all = <int>[];

  bool closed = false;

  @override
  void add(List<int> data) {
    all.addAll(data);
  }

  @override
  void close() {
    closed = true;
  }
}

String use() {
  final Bytes inner = Bytes();
  final ByteConversionSink outer = ByteConversionSink.from(inner);
  outer.add(<int>[1, 2]);
  outer.add(<int>[3]);
  outer.close();
  // ..and the other factory beside it, so the two stay two.
  final List<int> got = <int>[];
  final ByteConversionSink cb = ByteConversionSink.withCallback(
    (List<int> bytes) => got.addAll(bytes),
  );
  cb.add(<int>[9, 8]);
  cb.close();
  return '${inner.all.join(',')}|${inner.closed}|${got.join(',')}';
}
