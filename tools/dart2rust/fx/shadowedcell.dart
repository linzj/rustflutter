// A pattern binding declares the name again, and its reads are its own.
//
// A local a closure writes lives in a cell here, and every read and write
// of that *name* goes through it (`_cellLocals`). A pattern binding then
// declares the same name a second time -- `Actions.maybeFind` ends with
//
//     if (action case final Action<T>? action) { return action; }
//
// and the CFE hoists that binding to the enclosing scope, so the emitted
// Rust has a plain `let mut action` after the cell's. Rust resolves the
// name to it, while the reads still said `action.borrow()` -- "no method
// named `borrow` found for enum `Option<T>`", which stubbed the function.
//
// Both halves are pinned here: the pattern's binding reads as itself, and
// the cell comes back where a *branch-scoped* shadow ends.

String use() {
  // A local the closure below writes: a cell.
  String? found;
  final List<String> items = <String>['a', 'bb', 'ccc'];
  items.forEach((String item) {
    if (item.length == 2) {
      found = item;
    }
  });

  // The shape that was wrong: the name declared again by a pattern, in the
  // enclosing scope, and read through the new binding.
  String matched = '-';
  if (found case final String? found) {
    matched = found == null ? 'none' : found.toUpperCase();
  }

  // A shadow scoped to a branch: it ends with the branch.
  String inBranch = '-';
  if (found != null) {
    final String found2 = 'inner/${found!}';
    inBranch = found2;
  }

  // The cell read again after both, and written again by a second closure:
  // neither shadow stopped it being one.
  final String after = found ?? 'null';
  items.forEach((String item) {
    if (item.length == 3) {
      found = item;
    }
  });

  return '$matched|$inBranch|$after|$found';
}
