/// A `return switch (event) { .. }` in a `void` method, with object-pattern
/// arms that are null-aware calls and a `_ => null` arm.
///
/// `RenderPointerListener.handleEvent` is that shape. Each arm is a
/// `void?`, the switch is a `void`, and the temporary the CFE binds the
/// arms to takes whatever the first arm leaves: an `Option<()>` from a
/// null-aware call, where an arm the AOT compiler folded (a callback no
/// one in the program sets) leaves `()`. "expected enum `Option<()>`,
/// found unit type `()`" (1 stub at ws1116).
library;

abstract class Event {
  int get n;
}

class Down implements Event {
  Down(this.n);
  @override
  final int n;
}

class Move implements Event {
  Move(this.n);
  @override
  final int n;
}

class Up implements Event {
  Up(this.n);
  @override
  final int n;
}

class Other implements Event {
  Other(this.n);
  @override
  final int n;
}

typedef Handler = void Function(Event);

class Listener {
  Listener({this.onDown, this.onMove, this.onUp});

  final Handler? onDown;

  /// Never set anywhere in this program, so the arm below is folded.
  final Handler? onMove;
  final Handler? onUp;

  void handle(Event event) {
    return switch (event) {
      Down() => onDown?.call(event),
      Move() => onMove?.call(event),
      Up() => onUp?.call(event),
      _ => null,
    };
  }
}

String use() {
  final List<String> out = <String>[];
  final Listener l = Listener(
    onDown: (Event e) => out.add('down${e.n}'),
    onUp: (Event e) => out.add('up${e.n}'),
  );
  // A second listener with no `onDown`, so that `onDown` is nullable in
  // this program and its arm stays a null-aware call (the AOT compiler
  // folds one that is set at every construction into a plain call).
  final Listener bare = Listener(onUp: (Event e) => out.add('bare${e.n}'));
  for (final Event e in <Event>[Down(1), Move(2), Up(3), Other(4)]) {
    l.handle(e);
    bare.handle(e);
  }
  return out.join(',');
}
