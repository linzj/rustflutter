/// A **generic** local function: `T? effectiveValue<T>(..)` written inside a
/// build method, which is how `ButtonStyleButton`, `DatePickerDialog` and
/// `YearPicker` all read a property out of three styles.
///
/// Rust has neither a generic closure nor a nested `fn` that can see the
/// enclosing locals this one reads, so the declaration is written at each
/// type parameter's *bound* and every call site speaks those erased terms.
/// The two halves have to agree: a declaration erased on its own leaves the
/// argument closure returning `Option<f64>` into a slot that wants
/// `Option<Rc<dyn Object>>`.
///
/// The lifetimes are the other half. A local function that is only called
/// may borrow its captures, and such a closure lives exactly as long as its
/// `let`; one that is called from inside another function written beside it
/// outlives that, and has to own. Owning means the closures that call it
/// clone the binding in rather than moving it -- `scale` below is called
/// once from inside `twiceOver` and once after it.
class Style {
  const Style({this.width, this.tint, this.decorate});

  final double? width;
  final String? tint;
  final String Function(String)? decorate;
}

String use() {
  final Style? widgetStyle = const Style(width: 3.0);
  final Style? themeStyle = const Style(tint: 'red');
  final Style? defaultStyle = Style(
    width: 1.0,
    tint: 'black',
    decorate: (String s) => '[$s]',
  );

  // Three captured locals, and two different `T` at the call sites.
  T? effectiveValue<T>(T? Function(Style?) getProperty) {
    final T? widgetValue = getProperty(widgetStyle);
    final T? themeValue = getProperty(themeStyle);
    final T? defaultValue = getProperty(defaultStyle);
    return widgetValue ?? themeValue ?? defaultValue;
  }

  // A sibling local function that calls it. The call sits inside another
  // function node, so the binding is used past the `let` a borrowing
  // closure would end at.
  T? through<T>(T? Function(Style?) getProperty) {
    return effectiveValue<T>(getProperty);
  }

  final double? width = effectiveValue<double>((Style? s) => s?.width);
  final String? tint = through<String>((Style? s) => s?.tint);

  // A *function* value out of the erased slot: `Object?` holds it, and the
  // call site asks for it back at the type it was declared with.
  final String Function(String)? decorate = through<String Function(String)>(
    (Style? s) => s?.decorate,
  );

  // A plain local function called from inside an escaping closure, and
  // again after it: the closure clones the binding in.
  double scale(double v) {
    return v * (width ?? 1.0);
  }

  final String Function() twiceOver = () {
    return '${scale(2.0)}';
  };
  final double once = scale(4.0);

  return '$width/$tint/${decorate == null ? 'none' : decorate('x')}'
      '/${twiceOver()}/$once';
}
