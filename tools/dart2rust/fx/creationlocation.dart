/// `dart:developer`'s `CreationLocation.of`.
///
/// It hands back a location when the object implements the private
/// `_HasCreationLocation` -- the interface the `--track-widget-creation`
/// kernel transform makes a tracked class implement. Neither this
/// fixture's dill nor the gallery's is built with that transform, so `of`
/// is null for every object on both sides.
///
/// `debugIsWidgetLocalCreation` is the shape: TFA removed everything it
/// does with the answer and left `return false`, and what was left was a
/// `let` naming a type this compiler did not have -- "cannot find type
/// `CreationLocation` in this scope" (1 stub at ws1109).
library;

import 'dart:developer' as developer;

class Tracked {
  Tracked(this.n);

  final int n;
}

String describe(Object o) {
  final developer.CreationLocation? at = developer.CreationLocation.of(o);
  return at == null ? 'none' : '${at.file}:${at.line}';
}

String use() {
  final List<String> out = <String>[];
  out.add(describe(Tracked(1)));
  out.add(describe('a string'));
  out.add(describe(7));
  out.add(describe(<int>[1, 2]));
  return out.join('|');
}
