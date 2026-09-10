// A cast to a *raw* `List`. Dart's bare `List` is `List<dynamic>`, and this
// compiler spells a `List<dynamic>` element as the `Rc<dyn Object>` a
// `dynamic` is -- but the raw spelling came out as a bare `Vec`, which is
// no Rust type at all ("missing generics for struct `Vec`", E0107).
//
// `PredictiveBackEvent.fromMap` casts a platform message's field that way:
// `map['touchOffset'] as List?`.
String use() {
  final Map<String, Object?> map = <String, Object?>{
    'touchOffset': <Object?>[1, 2],
    'missing': null,
  };
  // The cast target is written without a type argument.
  final List? offset = map['touchOffset'] as List?;
  // Not `map['missing'] as List?`: a Dart `null` *stored in a map* comes
  // back as `Some(dart_null_object())` rather than `None`, and the cast's
  // downcast then unwraps a `None`. That is a different defect from the raw
  // spelling this fixture is about; see 已知欠账.
  return '${offset?.length}/${offset?.first}';
}
