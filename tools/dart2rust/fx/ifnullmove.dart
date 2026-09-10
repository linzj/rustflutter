/// `a ?? b` where both sides are locals that live on.
///
/// A `match` on a place moves out of it, so the *left* of `??` has been read
/// by clone since ws494. The right is produced by the `None` arm, which
/// moves out of it just the same -- `resolvedBackgroundBuilder ??
/// resolvedForegroundBuilder` in `ButtonStyleButton.build`, whose value is
/// then dropped while the program goes on reading both.
String use() {
  final String Function(String)? decorate = _wrapper();
  final String Function(String)? fallback = _empty();

  // The right side is a local the body reads again below.
  final String Function(String)? picked = fallback ?? decorate;

  // ..and a scalar, where the move is invisible because the type is `Copy`:
  // the rule has to hold for both or it is a rule about types.
  final int? absent = _nothing();
  final int count = absent ?? _seven();

  return '${picked!('a')}/${decorate!('b')}/$count';
}

String Function(String)? _wrapper() {
  return (String s) => '[$s]';
}

String Function(String)? _empty() {
  return null;
}

int? _nothing() {
  return null;
}

int _seven() {
  return 7;
}
