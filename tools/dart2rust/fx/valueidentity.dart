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
/// **Constants are in scope now** (ws1064). Dart canonicalises them, so two
/// `const Frozen(1)` are one object, and a constant is never the object a
/// constructor built. The front end used to rebuild an evaluated constant as
/// a constructor call because it reads like the source -- and that call mints
/// a fresh token, which would have made two canonicalised constants into two
/// objects. For a class carrying a token the constant stays a constant, and
/// two tokenless values compare by their fields, which is exactly what
/// `identical` means for them.
library;

/// Const-constructible, and built both ways below.
///
/// `label` is read further down on purpose: a field nothing reads is shaken
/// out by TFA, and a `Frozen` of one `int` is a class this compiler treats as
/// a pure value -- `Copy`, in a `Cell`, read by `get()` -- which carries no
/// token by design. An earlier draft of this fixture never read it and tested
/// nothing.
class Frozen {
  const Frozen(this.n, this.label);
  final int n;
  final String label;
}

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

  // Canonicalised constants are one object; two runtime instances are two;
  // and a constant is never the one a constructor built.
  const Frozen c1 = Frozen(1, 'x');
  const Frozen c2 = Frozen(1, 'x');
  const Frozen c3 = Frozen(2, 'x');
  final Frozen made = Frozen(1, 'x');
  final Frozen alsoMade = Frozen(1, 'x');
  out.add(
    '${identical(c1, c2)}/${identical(c1, c3)}/'
    '${identical(c1, made)}/${identical(made, alsoMade)}',
  );
  out.add(
    '${identityHashCode(c1) == identityHashCode(c2)}/'
    '${identityHashCode(made) == identityHashCode(alsoMade)}',
  );
  out.add('${c1.label}${c3.n}${made.label}');

  return out.join('|');
}
