/// An enum's carried field, beside `index` and `name`.
///
/// Dart writes `final Weight weight;` on the enum and a constant of it on
/// each value; there is no storage here, so this compiler writes an
/// accessor whose body is a `match` over those constants. That accessor's
/// signature is the compiler's own, and the constants in it are Dart
/// expressions: `const Weight(1)` is a constructor, and every constructor
/// here returns `Result`. Written `-> Weight` the accessor had nowhere to
/// put that and ended `.unwrap()` (work.md's "其余" bucket, 3 in the
/// gallery: `KeyboardLockMode.logicalKey`).
///
/// `index` and `name` are Dart's own members of every enum -- the position
/// and the spelling -- and stay the total functions they are: a `?` on one
/// is E0277, which is how the first cut of this rule cost 23 stubs.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

class Weight {
  const Weight(this.value);

  final int value;

  @override
  String toString() => 'w$value';
}

enum Level {
  low(Weight(1)),
  middle(Weight(5)),
  high(Weight(9));

  const Level(this.weight);

  final Weight weight;
}

String describe(Level l) => '${l.name}/${l.index}/${l.weight}';

String use() {
  final out = <String>[];
  for (final l in Level.values) {
    out.add(describe(l));
  }
  out.add('${Level.high.index > Level.low.index}');
  out.add('${Level.middle.weight.value}');
  out.add('alive');
  return out.join('|');
}
