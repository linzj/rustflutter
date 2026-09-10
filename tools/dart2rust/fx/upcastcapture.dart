// An explicit upcast *moves* its operand, and inside a closure that operand
// is a capture.
//
// `x as Rc<dyn Object>` consumes `x`. Where the cast sits in an `FnMut`
// closure and `x` is a captured handle, Rust refuses: "cannot move out of
// `render_object`, a captured variable in an `FnMut` closure" (E0507).
// `Scaffold.hitTestableAtOrigin` and `CupertinoPageScaffold`'s twin are that
// shape -- `result.path.any((entry) => entry.target == renderObject)`, where
// the `==` against an `Object?` slot upcasts the local.
//
// Cloning an `Rc` is a refcount bump, so the cast's operand is cloned
// wherever it *names a place* someone else holds. A value the expression
// made itself -- a call's result, a construction -- has no place to protect
// and is not cloned.

// Behind an `Rc`, because a closure in its body calls one of its own
// methods. That is what the compiler's `_countedClass` asks, and it is the
// reason `RenderMetaData` is a handle in the gallery: without it this class
// would be a plain struct, the upcast would copy, and the fixture would
// pass without testing anything.
// The interface the comparison's left side is typed by -- `HitTestTarget`
// in the gallery. Upcasting to a *named trait* is a different branch from
// upcasting to `Object`, and only the first one leaves the operand bare.
abstract class Tgt {}

class Meta implements Tgt {
  Meta(this.id);

  final int id;

  int Function() get counter =>
      () => describe();

  int describe() => id;
}

class Entry {
  Entry(this.target);

  // Non-nullable, as `HitTestEntry.target` is: the nullable slot wraps the
  // comparison in `Some(..)` and takes a different coercion path, which is
  // where the clone gets inserted upstream.
  final Tgt target;
}

class Holder {
  Holder(this.meta) : entries = <Entry>[Entry(meta), Entry(Meta(1))];

  final Meta meta;
  final List<Entry> entries;

  // Handed back as `Object?`, so the local below is a *downcast of a call*
  // -- the shape `renderObject` has in `hitTestableAtOrigin`
  // (`context.renderObject! as RenderMetaData`).
  Object? get anyMeta => meta;
}

// The gallery's shape exactly: a static-like function whose *return* is the
// `any(..)`, with the local read once and only inside the closure.
bool hitTestable(Holder h) {
  final Meta m = h.anyMeta! as Meta;
  return h.entries.any((Entry e) => e.target == m);
}

String use() {
  final Holder h = Holder(Meta(7));
  // Read out of somewhere else, and then used *only* inside the closure --
  // as `renderObject` is in `hitTestableAtOrigin`. A local read again after
  // the closure is cloned for that later read anyway, and the cast then has
  // nothing left to move: the first version of this fixture did that and
  // passed without the fix, testing nothing.
  final bool found = hitTestable(h);
  final Meta other = Meta(9);
  final bool absent = h.entries.any((Entry e) => e.target == other);
  // On a *different* instance, so `m` keeps its single use: `id` has to be
  // read or TFA drops it and every `Meta` compares equal (`absent` came out
  // `true` against Dart's `false`); `counter` has to be called or the
  // closure that makes this class counted is shaken away, the class stops
  // being a handle, and the upcast takes a branch this fixture is not
  // about.
  final Meta probe = Meta(3);
  final int seen = probe.describe() + probe.counter();
  return '$found/$absent/$seen';
}
