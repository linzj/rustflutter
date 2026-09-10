// `a += b!` has to apply the null-assert to `b`.
//
// `RenderTable._computeColumnWidths` writes `newTotalFlex += flexes[x]!`
// over a `List<double?>`, and the emitted Rust was
// `new_total_flex + flexes[x].clone()` -- the `!` dropped, so `f64 +
// Option<f64>`, which nobody implements.
//
// The same `!` in a plain expression (`remainingWidth * flexes[x]! /
// totalFlex`, two lines up in the same function) was already right, so this
// is about the compound-assignment path rather than about `!`.

String use() {
  final List<double?> flexes = <double?>[1.5, null, 2.5];
  double total = 0.0;
  for (int i = 0; i < flexes.length; i++) {
    if (flexes[i] != null) {
      // The shape that was wrong.
      total += flexes[i]!;
    }
  }
  // ..and the plain-expression form of the same read, which was not.
  final double scaled = 2.0 * flexes[0]! / total;

  // The other compound operators reach the same lowering.
  double taken = 10.0;
  taken -= flexes[2]!;
  taken *= flexes[0]!;

  // ..and on a nullable local rather than a list element.
  final int? step = <int?>[3, null][0];
  int count = 1;
  count += step!;

  return '$total/$scaled/$taken/$count';
}
