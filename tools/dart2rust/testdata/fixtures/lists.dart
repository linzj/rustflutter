// `List<T>` is `Vec<T>`.
//
// Measured before it was decided: across `package:flutter/` the calls on a
// List or Iterable are `[]` 687, `add` 548, `iterator` 410, `length` 343,
// `[]=` 141, `toList` 105 and `isEmpty`/`isNotEmpty` 181. Every one of those
// is a `Vec`, which is what made this an easy decision.
//
// `Map` is not, and is not here: its literal is insertion-ordered and
// `HashMap` is not, so the 109 places upstream iterates a map would silently
// come out in a different order. That one waits for a decision about what to
// represent it with.
//
// REFUSES: a `const` cannot hold a collection
//
// The lengths and values below are all different so a wrong index cannot pass.

class Marks {
  Marks(this.marks);

  List<int> marks;

  int get total {
    int sum = 0;
    for (int i = 0; i < marks.length; i = i + 1) {
      sum = sum + marks[i];
    }
    return sum;
  }

  /// `for (final x in xs)`. The CFE writes this as an iterator loop -- bind
  /// `xs.iterator`, loop while `moveNext()`, read `current` -- and 405 of
  /// upstream's 592 `for` statements are really this shape. Put back together
  /// rather than carried across in pieces, so both front ends land on Rust's
  /// own `for x in &xs`.
  int get doubledTotal {
    int sum = 0;
    for (final mark in marks) {
      sum = sum + mark * 2;
    }
    return sum;
  }

  void note(int mark) {
    marks.add(mark);
  }

  void overwriteFirst(int mark) {
    marks[0] = mark;
  }

  bool get empty {
    return marks.isEmpty;
  }

  /// A list literal, with its element type coming from the declaration rather
  /// than from the elements -- an empty one still has to know what it holds.
  static List<int> starting() {
    return <int>[3, 11, 29];
  }

  bool get any {
    return marks.isNotEmpty;
  }

  int get highest {
    return marks.last;
  }

  int get lowest {
    return marks.first;
  }

  /// A chain that is collected right there. Dart's `map` and Rust's are only
  /// the same when the chain ends -- `xs.iter().map(f)` is lazy and its
  /// elements are references -- so the whole chain is recognised at once. 72
  /// of upstream's 126 such calls are collected like this; the 54 that escape
  /// as a lazy Iterable are refused.
  List<int> tripled() {
    return marks.map((int m) => m * 3).toList();
  }

  /// A `const` list. Rust cannot build a `Vec` at compile time, so this one is
  /// refused rather than emitted as a constant that will not compile -- one
  /// broken constant would take the whole file with it.
  static const List<int> fixed = <int>[2, 4, 8];

  /// A map in a field, which is what stops the class being `Copy`.
  Map<String, int> notes = <String, int>{};

  /// A `static final`: computed once on first use, which is what Rust's
  /// `LazyLock` is. It goes at *module* scope, because an `impl` block may
  /// hold a `const` and not a `static`, and its name carries the class so two
  /// classes' `defaults` cannot collide.
  static final List<int> defaults = <int>[5, 13];

  static int defaultsTotal() {
    int sum = 0;
    for (final d in defaults) {
      sum = sum + d;
    }
    return sum;
  }

  /// A record, which is a Rust tuple. Positional only: a named field would
  /// need a struct with a name, and there is no name to give it.
  static (int, int) span() {
    return (3, 29);
  }

  /// Reading a record's fields back. Dart counts them from one and Rust
  /// counts tuple fields from zero.
  static int spanWidth() {
    final s = span();
    return s.$2 - s.$1;
  }

  /// A map literal. `HashMap::from([..])`, which is safe because the members
  /// that would need the insertion order are refused wherever they are used.
  static Map<String, int> sizes() {
    return <String, int>{'small': 4, 'large': 40};
  }

  /// A lookup-only map. `keys`, `values`, `entries` and `forEach` are refused
  /// because a Dart map iterates in insertion order and a `HashMap` does not,
  /// which would quietly reorder 109 places upstream.
  static int lookUp(Map<String, int> sizes, String name) {
    if (sizes.containsKey(name)) {
      return sizes[name]!;
    }
    return -1;
  }
}

/// The `List` and `Map` members Rust says differently rather than renames.
///
/// Measured rather than guessed at: 232 refusals named a `List` member and 22
/// a `Map` one, and the ones here are the ones whose Rust is exact. `sort`
/// takes a comparator returning an `int` where Rust wants an `Ordering`, and
/// `forEach` hands its closure a value where `iter()` hands it a reference --
/// those are not renames and are still refused.
class Members {
  const Members();

  static bool anyOver(List<int> xs, int limit) {
    return xs.any((int x) => x > limit);
  }

  static bool allOver(List<int> xs, int limit) {
    return xs.every((int x) => x > limit);
  }

  static String joined(List<String> xs) {
    return xs.join('-');
  }

  static String run(List<String> xs) {
    return xs.join();
  }

  static List<int> withInserted(List<int> xs, int at, int value) {
    xs.insert(at, value);
    return xs;
  }

  static int takenOut(List<int> xs, int at) {
    return xs.removeAt(at);
  }

  static int nth(List<int> xs, int at) {
    return xs.elementAt(at);
  }

  static List<int> middle(List<int> xs, int from, int to) {
    return xs.sublist(from, to);
  }

  static List<int> tail(List<int> xs, int from) {
    return xs.sublist(from);
  }

  static List<int> backwards(List<int> xs) {
    return xs.reversed.toList();
  }

  /// `Map.isNotEmpty` was on the `List` map and not the `Map` one, which is
  /// the same shape as every other "and the other half?" in this compiler.
  static bool hasAny(Map<String, int> m) {
    return m.isNotEmpty;
  }
}
