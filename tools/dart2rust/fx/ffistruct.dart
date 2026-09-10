/// A `dart:ffi` struct is not an ordinary class.
///
/// The CFE strips its declared fields and leaves accessors that read and
/// write bytes at an offset into a typed-data base the object carries:
/// `_loadInt64(this._typedDataBase, viewId#offsetOf + this._offsetInBytes)`.
/// The offsets themselves are a 24-entry constant list indexed by `_abi()`
/// -- one entry per ABI in `dart:ffi`'s `Abi.values` -- so a struct's layout
/// is a runtime question about the machine, not a compile-time one.
///
/// `Struct.create` is the half of this that needs no native allocator: the
/// base is a `Uint8List` the program owns, which is memory this compiler can
/// have. The Windows windowing structs in the gallery take their base from
/// `calloc`, and that path still dies where it should -- at the DLL call.
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

String use() {
  final Msg m = Struct.create<Msg>();
  m.viewId = 7;
  m.message = 3;
  m.wParam = -9;
  return '${m.viewId}/${m.message}/${m.wParam}/${sizeOf<Msg>()}';
}
