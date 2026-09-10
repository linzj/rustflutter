/// A `dart:ffi` struct's layout is a question about the *machine*.
///
/// The CFE's ffi transform writes `sizeOf<T>()` and every field's offset as
/// a constant list with one entry per ABI, indexed by `_abi()`:
///
///     Msg.#sizeOf => const <int>[16, 24, 16, 24, ..][_abi()]
///
/// so a compiler with no `_abi()` refuses every one of them -- 7 of the
/// gallery's 45 refusals, all in the Windows windowing plugin. With it they
/// are arithmetic, and the arithmetic has to come out the same as Dart's on
/// the same machine: the two structs below are laid out differently on a
/// 32-bit ABI than on a 64-bit one, and `Small` is where alignment shows.
library;

import 'dart:ffi';

final class Msg extends Struct {
  @Int64()
  external int viewId;

  @Int32()
  external int message;

  @Int64()
  external int wParam;
}

final class Small extends Struct {
  @Int8()
  external int flag;

  @Int16()
  external int code;
}

final class Wide extends Struct {
  @Int32()
  external int a;

  external Pointer<Void> p;

  @Double()
  external double d;
}

String use() {
  return '${sizeOf<Msg>()}/${sizeOf<Small>()}/${sizeOf<Wide>()}';
}
