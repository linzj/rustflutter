// A method that merely *shares* a collection mutator's name is not a
// mutation of a place.
//
// `remove` is what a `List` does, so a call named `remove` was routed to the
// place the receiver lives in, borrowed mutably. On a handle there is no
// such place to borrow: the receiver is an object with a method, and the
// mutation happens inside it. Asked for anyway, a closure's captured binding
// was borrowed `&mut` without ever being given one ("cannot borrow `entry`
// as mutable", E0596).
//
// The receiver's type decides it. Where nothing is recorded the old path
// stands: guessing "not a collection" would quietly move a real mutation
// onto a clone, which is the mistake ws944 measured -- `mut` turning an
// honest panic into a silently wrong answer.
//
// `ScaffoldState._buildBottomSheet` holds a `LocalHistoryEntry? entry` and
// calls `entry!.remove()` from inside the closure it hands to the sheet.
// The same lesson was learned for a *field* at ws509 and left the general
// case alone.

class Slot {
  Slot(this.name);

  final String name;

  // Named like a `List`'s mutator; it is just a method here.
  String remove() => 'gone:$name';

  // ..and one named like `Set.add`, to say the rule is about the receiver
  // and not about the one name.
  String add() => 'kept:$name';
}

String pick(bool present) {
  final Slot? entry = present ? Slot('a') : null;
  // Captured by a closure and null-checked inside it: the binding the
  // closure holds is what was being asked for `&mut`.
  final String Function() drop = () => entry!.remove();
  final String Function() keep = () => entry!.add();
  if (entry == null) {
    return 'none';
  }
  return '${drop()}/${keep()}';
}

// A real collection through the same shapes, so the rule cannot be read as
// "never mutate a place": this one must still mutate the list itself.
String counted() {
  final List<String> items = <String>['x', 'y'];
  final void Function() drop = () => items.remove('x');
  drop();
  return items.join('+');
}

String use() => '${pick(true)}/${pick(false)}/${counted()}';
