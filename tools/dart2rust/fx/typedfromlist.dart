// `Uint32List.fromList(<int>[..])` with a value above 2^31. The conversion
// is written `src.iter().map(|v| *v as u32).collect::<Vec<u32>>()`, and the
// *source* had no type: a Dart `List<int>` is a `Vec<i64>`, but nothing
// said so, and Rust's integer defaulting made the literals `i32` --
// "literal out of range for `i32`", which is denied.
//
// crypto's `Sha256Sink` starts from exactly such a list: the eight SHA-256
// initial hash values, five of which are above 2^31.
import 'dart:typed_data';

String use() {
  final Uint32List big = Uint32List.fromList(<int>[
    0x6a09e667,
    0xbb67ae85,
    0x3c6ef372,
    1,
  ]);
  // ..and a narrowing conversion whose source is doubles, which is the
  // same shape with another element type.
  final Float32List small = Float32List.fromList(<double>[1.5, 2.5]);
  return '${big[0]}/${big[1]}/${big[3]}/${small[0]}/${small.length}';
}
