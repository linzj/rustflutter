/// A `Comparable<T>` used as the `Comparable<Object?>` Dart's covariance
/// says it is.
///
/// The data table demo's `_sort` calls `Comparable.compare(aValue,
/// bValue)` on two `Comparable<T>`s, which is `compareTo` through
/// `Comparable<dynamic>`; `binarySearch<T extends Comparable<Object>>` asks
/// the same of its `T`. The prelude's `Comparable<T>` was not a `DartAny`,
/// so the cast had no method to be called on ("`dart_cast_to` exists ..
/// but its trait bounds were not satisfied", 1 stub since ws1098) -- and
/// made one at ws1102, the cast table answered no wider instantiation and
/// threw at run time. Now every implementor -- the prelude's scalars and
/// every translated class -- carries `Comparable<Object?>` beside its own,
/// bringing the other side down to what it compares with, and the cast
/// table answers it.
library;

class Score implements Comparable<Score> {
  const Score(this.v);
  final int v;

  @override
  int compareTo(Score other) => v.compareTo(other.v);
}

int cmp(Object? a, Object? b) => (a! as Comparable<Object?>).compareTo(b);

String tryCmp(Object? a, Object? b) {
  try {
    return '${cmp(a, b)}';
  } on TypeError {
    return 'type';
  }
}

String use() {
  final List<Object?> results = <Object?>[
    cmp(const Score(1), const Score(3)),
    cmp(3, 1),
    cmp('b', 'a'),
    cmp(2.5, 2),
    cmp(2, 2.0),
    Comparable.compare(const Score(2), const Score(2)),
    Comparable.compare(1, 2.0),
    tryCmp('a', 1),
  ];
  final List<Score> list = <Score>[
    const Score(3),
    const Score(1),
    const Score(2),
  ]..sort();
  return '${results.join(',')}|${list.map((Score s) => s.v).join()}';
}
