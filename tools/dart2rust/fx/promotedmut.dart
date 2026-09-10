/// A **promoted** local as the receiver of a call the backend routes through
/// `_mutPlace`.
///
/// `resolvedPadding!.add(..)` in `ButtonStyleButton.build`: `add` is a
/// mutating name, so the receiver is emitted as the place it names --
/// `resolved_padding.as_mut().unwrap()` -- while `_WalkSelf`, which decides
/// whether the binding is written `let mut`, only ever looked at a bare
/// local receiver. `_mutPlace` peels the `!` to reach the local; the walker
/// did not, so the declaration came out immutable (ws893).
///
/// The receiver here is a handle to a trait (`Rc<dyn Pad>`) and `add`
/// returns a new value rather than changing the receiver -- which is what
/// upstream's `EdgeInsetsGeometry.add` does too. Whether the name is a
/// mutator is the backend's question; whether the binding says `mut` has to
/// give the same answer.
abstract class Pad {
  double get value;

  Pad add(Pad other);
}

class Inset implements Pad {
  const Inset(this.value);

  @override
  final double value;

  @override
  Pad add(Pad other) {
    return Inset(value + other.value);
  }
}

String use() {
  final Pad? resolved = _maybePad();
  final Pad total = resolved!.add(const Inset(2.0));

  // ..and a plain collection through the same promotion, which already
  // worked: the rule has to keep answering for both.
  final List<String>? items = _maybe();
  items!.add('b');

  return '${total.value}/${resolved.value}/${items.join(",")}';
}

Pad? _maybePad() {
  return const Inset(3.0);
}

List<String>? _maybe() {
  return <String>['a'];
}
