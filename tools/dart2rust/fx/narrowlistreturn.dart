/// A `dart:typed_data` list returned where the declaration says
/// `List<int>`.
///
/// `Uint8List` is a `Vec<u8>` here (the backend's typed-data table) and a
/// `List<int>` is a `Vec<i64>`. Rust converts neither the elements nor the
/// `Vec` around them, so the trait forwarder that hands an override's
/// answer back read:
///
///     expected `Result<Vec<i64>, ..>`, found `Result<Vec<u8>, ..>`
///
/// `Uint8Buffer._createBuffer` is the shape: `TypedDataBuffer<E>` declares
/// `List<E> _createBuffer(int)` and `Uint8Buffer` overrides it returning a
/// `Uint8List` (1 stub at ws1102).
library;

import 'dart:typed_data';

abstract class Buf {
  List<int> make(int n);

  int total(int n) => make(n).fold(0, (int a, int b) => a + b);

  String first(int n) => '${make(n).first}';
}

class Bytes extends Buf {
  @override
  Uint8List make(int n) => Uint8List(n)..[0] = 7;
}

class Shorts extends Buf {
  @override
  Int16List make(int n) => Int16List(n)..[0] = -9;
}

class Plain extends Buf {
  @override
  List<int> make(int n) => List<int>.filled(n, 2);
}

String use() {
  final out = <String>[];
  for (final Buf b in <Buf>[Bytes(), Shorts(), Plain()]) {
    out.add('${b.total(3)}/${b.first(3)}');
  }
  return out.join('|');
}
