/// A Dart `operator +` can throw, and the caller catches it.
///
/// `operator +` is an ordinary method in Dart: its body may throw and
/// `try { a + b } catch (e)` is ordinary code. This compiler wrote it as
/// `impl std::ops::Add`, whose signature is std's and cannot say `Result` --
/// "inside it a failing call unwraps", which is a panic. The 60 `impl
/// std::ops::*` in the gallery were read by nothing but this compiler's own
/// `a + b`, which is a spelling it chooses, so the impls are gone and the
/// call sites say `a.op_add(b)?`.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

class Money {
  const Money(this.cents);

  final int cents;

  Money operator +(Money other) {
    if (cents + other.cents > 100) {
      throw StateError('over a pound: ${cents + other.cents}');
    }
    return Money(cents + other.cents);
  }

  Money operator -(Money other) => Money(cents - other.cents);

  Money operator *(int by) => Money(cents * by);

  Money operator -() => Money(-cents);

  @override
  String toString() => '${cents}c';
}

String add(Money a, Money b) {
  try {
    return '${a + b}';
  } on StateError catch (e) {
    return '$e';
  }
}

String use() {
  final out = <String>[];
  out.add(add(const Money(30), const Money(40)));
  out.add(add(const Money(60), const Money(70)));
  out.add('${const Money(90) - const Money(15)}');
  out.add('${const Money(7) * 3}');
  out.add('${-const Money(5)}');
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
