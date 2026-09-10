/// A `num` method called on a `dynamic` slot, whose value then goes into an
/// `Object` slot.
///
/// The compiler lowers `n.abs()` on a `dynamic` receiver by narrowing it to
/// the `f64` a `num` is here and calling that method -- so what is in hand
/// is an `f64`, not the `Rc<dyn Object>` Dart's static type says. Handed to
/// a parameter declared `Object` it has to be boxed, and the *recorded*
/// type is what decides that: the narrowed call was left untyped, so the
/// temporary the CFE binds for an interpolation was declared `Rc<dyn
/// Object>` over an `f64` with nothing between (`NumberFormat.format` and
/// `_formatFixed`).
///
/// Only a `double` here. A `dynamic` holding an **int** takes the same
/// narrowing and `downcast_ref::<f64>()` finds nothing -- a defect of its
/// own, measured and written up in STATUS, not fixed by this rule.
library;

String use() {
  final dynamic n = _asDynamic(-3.5);
  return '${_show(n.abs())}/${_show(n.round())}/${_show(n.abs() + 1.0)}'
      '/${_show(n.isNaN)}/${_show(n.toStringAsFixed(2))}/${_show(n.toInt())}';
}

dynamic _asDynamic(double value) {
  return value;
}

String _show(Object value) {
  return '$value';
}
