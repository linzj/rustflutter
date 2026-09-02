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
}
