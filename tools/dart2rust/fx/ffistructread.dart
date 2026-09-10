/// Reading a `dart:ffi` struct whose base the program owns.
///
/// The CFE strips a `Struct` subclass's declared fields and leaves accessors
/// over a typed-data base the object carries, plus a `#fromTypedDataBase`
/// constructor: `_loadInt64(this._typedDataBase, viewId#offsetOf +
/// this._offsetInBytes)`. Two things had to exist for that to translate --
/// the carried fields (`_Compound`'s `_typedDataBase` and `_offsetInBytes`,
/// which live two levels above `Struct` and so were not found by looking at
/// the base alone), and the load primitives themselves.
///
/// `Struct.create` is the half that needs no native allocator: the base is a
/// `Uint8List` the program owns. It is zeroed, so every field reads zero,
/// and both ends have to say so.
///
/// The *writes* are deliberately absent. `_storeIntNN` cannot be written
/// while a `Uint8List` is a value here: the store would land in a copy and
/// the next load would not see it. `fx/ffistruct.dart` is the red fixture
/// that pins that boundary.
library;

import 'dart:ffi';

final class Reading extends Struct {
  @Int64()
  external int viewId;

  @Int32()
  external int message;

  @Int64()
  external int wParam;
}

String use() {
  final Reading a = Struct.create<Reading>();
  // A second one, so the fixture would notice if every struct shared a base.
  final Reading b = Struct.create<Reading>();
  return '${a.viewId}/${a.message}/${a.wParam}/${b.viewId}/${sizeOf<Reading>()}';
}
