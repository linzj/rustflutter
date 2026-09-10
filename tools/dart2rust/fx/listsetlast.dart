// `list.last = value` writes through the last slot.
//
// Two things were missing and either one alone is wrong:
//
//   * the prelude had no `set_last` at all -- `Vec`'s `last` is Rust's own
//     slice method, which is why rustc offered "a method `last` with a
//     similar name, but with different arguments";
//   * and the receiver has to be the *place*. A field read comes out of its
//     cell already cloned (`{ let __r = self.f.borrow().clone(); __r }`), so
//     with the method alone the write lands on the copy, compiles, and
//     changes nothing -- the silent kind of wrong. `set_last` is in
//     `mutatingRustOnlyNames` for that reason.
//
// `DiagnosticsNode.write`'s `_wrappableRanges.last = wrapEnd` is the
// gallery's.

class Ranges {
  Ranges(this.values);

  final List<int> values;

  // A closure in the body calling one of this class's own methods is what
  // makes it counted, so `values` is kept in a cell and the read/write
  // distinction above is real. Called below, or TFA shakes it away and the
  // class stops being counted.
  int Function() get totalLater =>
      () => sum();

  int sum() => values.fold(0, (int a, int b) => a + b);

  void setLast(int v) {
    values.last = v;
  }
}

String use() {
  final Ranges r = Ranges(<int>[1, 2, 3]);
  r.setLast(9);
  // Read back through the object, not through a local copy: the whole
  // question is whether the write reached what the object holds.
  return '${r.values.join(",")}/${r.sum()}/${r.totalLater()}';
}
