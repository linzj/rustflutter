// A super function's `__Self` is bounded by traits Dart's `on` clause never
// named, and the bound is inherited by every trait below.
//
// `super.x()` inside a mixin dispatches to the previous class of the
// *application*, which the `on` clause does not mention, so the free function
// asks for that trait and the trait declaration repeats it. A mixin `on` that
// mixin then has the same trait over its own `__Self`, though nothing in its
// Dart chain names it -- and a name the chain declares *once* may be declared
// a second time up there. One member to Dart, two candidates to Rust
// ("multiple applicable items in scope", E0034).
//
// `RenderAbstractLayoutBuilderMixin.layoutInfo` reads `constraints`, which
// only `RenderObject` declares along its `on` chain; its
// `RenderObjectWithLayoutCallbackMixin` reaches `RenderBox.markNeedsLayout`
// through `super`, so `RenderBox::constraints` stands there too and neither
// candidate wins.
//
// The answer is still Dart's: the trait its chain declares the name on. The
// override still arrives, because that trait's impl on the class carries it
// -- `read()` must say `mid`, not `base`.

abstract class Base {
  String get label => 'base';
  String stamp() => 'base';
}

abstract class Mid extends Base {
  @override
  String get label => 'mid';
  @override
  String stamp() => 'mid';
}

// Two different direct bases under `Mid`. `_appliedOver` takes what every
// application of a mixin has *directly* below it, and these two share
// nothing -- so `Mid` stays out of the mixin's own Dart chain, which is the
// gallery's shape. Both are still below `Mid`, so the widened bound holds for
// every implementer, which is what the bound rests on.
abstract class Left extends Mid {}

abstract class Right extends Mid {}

mixin Ticker on Base {
  // `super.stamp()` resolves to `Mid.stamp` -- the previous class of the
  // application, which this `on` clause never named. That is what widens
  // `__Self` here, and `Reader` below inherits the widening.
  String tick() => super.stamp();
}

mixin Reader on Ticker {
  // Dart resolves `label` to `Base.label`: `Mid` is nowhere on the `on`
  // chain. Rust has `Base` and `Mid` both declaring it.
  String read() => label;
}

class Thing extends Left with Ticker, Reader {}

class Plain extends Right with Ticker, Reader {}

String use() {
  // Through the traits, so the members stay on them: called on the class,
  // the chain devirtualises and no trait declares anything.
  final Reader one = Thing();
  final Reader two = Plain();
  final Base three = Thing();
  return '${one.tick()}/${one.read()}/${two.read()}/${three.label}';
}
