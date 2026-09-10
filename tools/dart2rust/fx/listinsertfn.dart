// An argument that goes into a list's *element type* has to be coerced into
// it, and `insert` was not on the list of members that do.
//
// `_CupertinoDatePickerDateTimeState.build` writes
// `pickerBuilders.insert(1, _buildTimeSeparatorWidget)` into a
// `List<_ColumnBuilder>`, whose element is a function type -- so the element
// is `Rc<dyn Fn(..)>` here and a method tear-off is a closure. The list
// *literal* on the line above coerces (`{ let __f: Rc<dyn Fn..> = Rc::new(..);
// __f }`); `insert` handed the closure over bare, and rustc said "expected
// `Rc<dyn Fn(..)>`, found closure".
//
// The set that coerced was `{remove, indexOf, lastIndexOf}` -- the members
// that *compare* an element. The members that *put* one in are the same
// question and were missing: `add`, `insert`.
//
// Both tear-off shapes are here because they fail differently: an instance
// method captures `this` and is a closure, a top-level function is a bare
// `fn` item ("expected `dyn Fn`, found fn item", `cupertino_route.rs`).

typedef Step = int Function(int);

int thrice(int v) => v * 3;

class Steps {
  Steps(this.base);

  final int base;

  // An instance method: the tear-off captures `this`, so it is a closure.
  int scaled(int v) => v * base;

  int negated(int v) => -v - base;

  List<Step> build() {
    // The literal coerces already -- this is the half that worked.
    final List<Step> steps = <Step>[scaled];
    // ..and these are the two that did not.
    steps.insert(0, negated);
    steps.add(thrice);
    return steps;
  }
}

String use() {
  final List<Step> steps = Steps(10).build();
  final List<String> out = <String>[];
  for (final Step s in steps) {
    out.add('${s(2)}');
  }
  return out.join(',');
}
