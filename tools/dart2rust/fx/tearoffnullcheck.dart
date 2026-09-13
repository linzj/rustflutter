/// A tear-off of a method of `this.field!`, handed on from an abstract
/// class's body.
///
/// A tear-off rooted at `this` (`this.controller.dispose` as a value) is the
/// closure that calls it, and in a counted class that closure holds the
/// handle. The root was read through field reads only, so `this.child!.at`
/// -- the same object, null-checked on the way -- was rooted nowhere: not
/// held, not bound, and the closure borrowed `this_`, which in an abstract
/// class's body is `&dyn Padded`. Boxed into the `'static` slot it read:
///
///     lifetime may not live long enough
///     coercion requires that `'1` must outlive `'static`
///
/// `RenderSliverEdgeInsetsPadding.hitTestChildren` hands
/// `addWithAxisOffset(.., hitTest: child!.hitTest)` (1 stub at ws1115).
library;

typedef Test = bool Function(double at, {required double slack});

/// Calls the callback twice; keeps nothing.
String probe(double at, Test test) {
  final List<String> out = <String>[];
  for (final double p in <double>[at, at + 1]) {
    out.add('${test(p, slack: 0.5)}');
  }
  return out.join(',');
}

abstract class Probe {
  bool at(double x, {required double slack});
}

class Gate implements Probe {
  Gate(this.edge);
  final double edge;
  @override
  bool at(double x, {required double slack}) => x + slack >= edge;
}

/// `child` is declared here *and* re-declared below, so a read of it in
/// `Padded`'s body is spelled through a named trait path -- as
/// `RenderSliverEdgeInsetsPadding`'s `child` is, declared by the mixin and
/// again by the class.
abstract class HasChild {
  Probe? get child;
}

abstract class Padded extends HasChild {
  double get pad;
  @override
  Probe? get child;
  String hits(double at) => child == null ? 'none' : probe(at - pad, child!.at);

  /// A closure calling a method of `this` is what makes the class counted
  /// -- and it is the abstract class that has to be, as
  /// `RenderSliverEdgeInsetsPadding` is.
  String Function() get later =>
      () => hits(1);
}

class Box extends Padded {
  Box(this.pad, this.child);
  @override
  final double pad;
  @override
  final Probe? child;
}

String use() {
  final Box a = Box(1, Gate(2));
  final Box b = Box(0, null);
  return '${a.hits(2)}|${a.hits(3)}|${b.hits(5)}|${a.later()}';
}
