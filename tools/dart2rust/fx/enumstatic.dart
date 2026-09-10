// An enum's own statics, and per-variant state that is not a literal.
//
// Two halves of one shape, both from `KeyboardLockMode`:
//
// * A Dart enum may declare a static beside its variants. The front end
//   dropped every field an enum declares ("an enum's own members are its
//   variants and the CFE's bookkeeping"), and the *read* of one was spelled
//   as a variant -- `KeyboardLockMode::_knownLockModes`, a name the Rust
//   enum has never had. A variant is a static const field typed by the enum
//   itself; anything else it declares statically is a constant like any
//   other class's.
//
// * A variant may carry state that is not one of the four literal shapes:
//   `numLock._(LogicalKeyboardKey.numLock)` carries an object. The recovery
//   kept only literals and dropped the whole variant's state, so no getter
//   was written and every member reading it failed (ws939). It now carries
//   the *constant* and lowers it with the same `_constant` that renders
//   `LogicalKeyboardKey::new(..)` anywhere else.
//
// The two meet in the static's initialiser, which reads the carried state:
// `_knownLockModes` is built out of `numLock.logicalKey.keyId`.

class KeyId {
  const KeyId(this.id, this.label);
  final int id;
  final String label;
}

enum Lock {
  // Per-variant state that is an *object*, beside two literals.
  numLock._(KeyId(1, 'num'), 1, 'N'),
  scrollLock._(KeyId(2, 'scroll'), 2, 'S'),
  capsLock._(KeyId(4, 'caps'), 4, 'C');

  const Lock._(this.key, this.bit, this.mark);

  final KeyId key;
  final int bit;
  final String mark;

  // A `static final` computed from the variants' carried state -- the shape
  // that needs both halves at once.
  static final Map<int, Lock> _byId = <int, Lock>{
    numLock.key.id: numLock,
    scrollLock.key.id: scrollLock,
    capsLock.key.id: capsLock,
  };

  // ..and a `static const`, which is a constant rather than a lazy one.
  static const int allBits = 7;

  static Lock? find(int id) => _byId[id];
}

String use() {
  final Lock? found = Lock.find(2);
  final Lock? missing = Lock.find(8);
  // The object the variant carries, read through the generated getter.
  final String label = Lock.capsLock.key.label;
  // The literals beside it still work.
  final String marks = Lock.values.map((Lock l) => l.mark).join(',');
  final int sum = Lock.values.fold<int>(0, (int a, Lock l) => a + l.bit);
  return '${found?.mark}|${missing?.mark}|$label|$marks|$sum|${Lock.allBits}';
}
