/// What identity means for a class this compiler copies by value.
///
/// A translated value class is a Rust struct, and handing it around clones
/// it, so the address of a copy answers nothing: `identical`,
/// `identityHashCode` and `super.==`/`super.hashCode` on such a class were
/// all refused. The token is the missing fact -- one `Rc<()>` that the clones
/// of an object share and a separately built object does not.
///
/// **Which classes this is even about**, since three other rules get there
/// first and each of them is right. A class that hands `this` out, that is
/// mutated through an alias, or that holds itself is *counted* already, and a
/// counted object has a real address; earlier drafts of this fixture tested
/// nothing because a list literal, and then a write through an alias, made
/// the class counted. `_ScribbleCacheKey` in `EditableText` is the shape that
/// is left: immutable, never handed out, and asked `identical(other, this)`.
///
/// A class with a `const` constructor is also left out. Dart canonicalises
/// constants, so two `const Frozen(1)` are one object -- but a local
/// `const Frozen(1)` reaches the backend as a *call* to the constructor, and
/// a fresh token per call would call them different objects. That is the
/// constant half, and it is a round of its own.
library;

class Tag {
  Tag(this.name, this.rank);
  final String name;
  final int rank;

  /// `_ScribbleCacheKey.compare`'s shape: identity first, fields after.
  String against(Tag other) {
    if (identical(other, this)) {
      return 'same';
    }
    return name == other.name && rank == other.rank ? 'equal' : 'different';
  }
}

String use() {
  final List<String> out = <String>[];

  final Tag a = Tag('a', 1);
  final Tag alias = a;
  final Tag twin = Tag('a', 1);
  final Tag other = Tag('b', 2);

  // Assigned from: the same object. Built separately, field for field
  // equal: not the same object.
  out.add('${identical(a, alias)}/${identical(a, twin)}');

  // Through the method that asks it of `this`, which is the shape the
  // gallery refused.
  out.add('${a.against(alias)}/${a.against(twin)}/${a.against(other)}');

  // The hash agrees with that, and is stable across reads -- it has to be,
  // or a map keyed by identity would lose things. Note `a` and `twin` are
  // equal field for field, so a structural hash would call these all true.
  out.add(
    '${identityHashCode(a) == identityHashCode(alias)}/'
    '${identityHashCode(a) == identityHashCode(twin)}/'
    '${identityHashCode(a) == identityHashCode(a)}',
  );

  return out.join('|');
}
