// A `null` argument that lands in a *hoisted temporary* has to carry the
// parameter's type, not `Null`.
//
// The backend spells a null literal `None::<T>` when it knows the slot and
// has nothing to infer from when it does not: an argument hoisted beside
// another that needs a temporary came out `let __t12: Option<Null> = None`
// where the callee wanted an `Option<Rc<dyn ScrollController>>`
// (`AboutDialog._detailPageRoute`).
class Ticker {
  const Ticker(this.label);

  final String label;
}

class Panel {
  Panel(this.ticker, this.title);

  final Ticker? ticker;
  final String title;

  String get shown => '${ticker?.label ?? "none"}/$title';
}

String? _cache;

String use() {
  // `null` beside an argument that needs a temporary of its own, so the
  // null is hoisted rather than written straight into the call.
  final panels = <Panel>[
    Panel(null, _cache ??= 'first'),
    Panel(const Ticker('t'), _cache ??= 'second'),
    Panel(null, _cache ?? 'third'),
  ];
  return panels.map((p) => p.shown).join('|');
}
