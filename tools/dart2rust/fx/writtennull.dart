// `null?.anything` is `null`.
//
// Dart evaluates nothing to the right of `?.` when the left is null, so a
// chain whose receiver is a `null` the *lowering itself* wrote -- an omitted
// argument, a const field -- has nothing to map. Mapped anyway, the closure
// binds a value that does not exist and types nothing:
// `None.as_ref().map(|it| ..)` is "type annotations needed for `&_`" (E0282).
//
// A string interpolation of a nullable is such a chain: `'$x'` asks `x` for
// its `toString`, and an omitted `String? package` reaches it as the literal
// null. The branch is dead in Dart -- `package == null` picked the other one
// -- but Rust still has to type it.
//
// `_WidgetStateTextStyle.new` is exactly this: `TextStyle`'s constructor is
// inlined with `package` omitted and computes
// `'packages/$package/$fontFamily'` in the branch that is never taken.
// `ChangeNotifierProvider.value` is the same null reaching an *adapter*
// instead of an interpolation -- the omitted `updateShouldNotify` being
// widened for `_ValueInheritedProvider<Object>`.
//
// Only a null the lowering wrote is folded. A receiver that is merely
// nullable is still asked at run time, which `viaField` below holds down.

class Base {
  Base({String? package, String? family})
    : fontFamily = package == null ? family : 'packages/$package/$family';

  final String? fontFamily;
}

class Bare extends Base {
  // Both omitted: the lowering writes the `null`s itself.
  Bare() : super();
}

class Packaged extends Base {
  Packaged() : super(package: 'p', family: 'f');
}

// A receiver that is nullable but *not* written null: it has to be asked.
String viaField(String? given) => 'via:${given?.toUpperCase()}';

String use() {
  final Bare bare = Bare();
  final Packaged packaged = Packaged();
  return '${bare.fontFamily}/${packaged.fontFamily}'
      '/${viaField(null)}/${viaField('x')}';
}
