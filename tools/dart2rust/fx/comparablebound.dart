/// `compareTo` on a type parameter bounded by `Comparable<Object>`.
///
/// `binarySearch<T extends Comparable<Object>>(List<T> sortedList, T value)`
/// calls `element.compareTo(value)` on a `T`. A Rust type parameter has
/// no methods of its own and the bound names no trait here, so the call
/// had nothing to land on ("no method named `compare_to` found for type
/// parameter `T`", 1 stub since ws1098). The receiver goes through the
/// object to the `Comparable<Object?>` every implementor answers (ws1130)
/// and the argument is handed over as the object the method takes.
library;

class Score implements Comparable<Score> {
  const Score(this.v);
  final int v;

  @override
  int compareTo(Score other) => v.compareTo(other.v);

  @override
  String toString() => 's$v';
}

int find<T extends Comparable<Object>>(List<T> sorted, T value) {
  int min = 0;
  int max = sorted.length;
  while (min < max) {
    final int mid = min + ((max - min) >> 1);
    final T element = sorted[mid];
    final int comp = element.compareTo(value);
    if (comp == 0) {
      return mid;
    }
    if (comp < 0) {
      min = mid + 1;
    } else {
      max = mid;
    }
  }
  return -1;
}

String use() {
  final List<int> ints = <int>[1, 3, 5, 7];
  final List<String> words = <String>['a', 'c', 'e'];
  final List<Score> scores = <Score>[
    const Score(2),
    const Score(4),
    const Score(6),
  ];
  return '${find<int>(ints, 5)},${find<int>(ints, 4)},${find<String>(words, 'e')},${find<Score>(scores, const Score(4))},${find<Score>(scores, const Score(5))}';
}
