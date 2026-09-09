part of '../backend_rust.dart';

augment class RustBackend {
  // -- Mutability -------------------------------------------------------------

  /// Methods of this class that need `&mut self`.
  ///
  /// Seeded with the ones that write a field of `this`, then closed over calls:
  /// a method that calls a mutating method **on itself** is mutating too. That
  /// closure is the answer to "who decides how far `&mut` spreads" -- nobody
  /// decides, it is computed, and it stops at the class boundary because a call
  /// on another object is refused for other reasons already.
  ///
  /// A fixpoint rather than one pass: `a` may call `b` which calls `c`, and only
  /// `c` writes. One pass would find `b` and miss `a`.
  late final Set<String> _mutating = _computeMutating();

  Set<String> _computeMutating() {
    final writes = <String>{};
    final calls = <String, Set<String>>{};
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      final key = _rustName(method);
      final found = _WalkSelf();
      found.statement(method.body);
      // No special case for setters. One was written here -- "a setter exists
      // to change something, so mark it mutating" -- and the mutation sweep
      // could not kill it: a setter that only delegates is already reached by
      // the contagion below, and one that writes nothing needs no `&mut self`.
      // A rule with no observable difference should not be written.
      if (found.writesFields) writes.add(key);
      calls[key] = found.selfCalls;
    }
    var changed = true;
    while (changed) {
      changed = false;
      for (final entry in calls.entries) {
        if (writes.contains(entry.key)) continue;
        if (entry.value.any((c) => writes.contains(snake(c)))) {
          writes.add(entry.key);
          changed = true;
        }
      }
    }
    return writes;
  }

  /// `&self` or `&mut self`, and a refusal where the signature is not ours.
  ///
  /// Two shapes have a fixed receiver and cannot become `&mut self`:
  ///
  /// * an operator that became an `impl std::ops::*`, whose method takes `self`
  ///   by value because the trait says so;
  /// * a method an abstract base declares, whose receiver is the trait's, not
  ///   this class's -- changing it would have to change the trait and every
  ///   other implementor.
  ///
  /// Both are refused rather than emitted with the wrong receiver. Upstream's
  /// operators do not assign, so the first is a guard rather than a loss.
  String _receiverOf(IrMethod method) {
    // A method that hands out a closure holding `this` takes the handle, not
    // a borrow: the closure keeps a clone of it, and only an `Rc` clones into
    // something that outlives the call.
    if (cls.counted && _handles.contains(_rustName(method))) {
      _selfIsHandle = true;
      return 'self: &std::rc::Rc<Self>';
    }
    _selfIsHandle = false;
    // A counted class never takes `&mut self`: an `Rc` hands out shared
    // access, and every mutable field of one is in a cell for that reason.
    if (cls.counted) return '&self';
    // A trait's method has one signature for every class implementing it,
    // so it is `&mut self` when *any* of them writes a field in it -- and
    // so is every implementation and forwarder, whether or not that one
    // writes. Decided per class, `ChangeNotifier::add_listener(self, ..)`
    // under a `&self` forwarder was 10 "types differ in mutability" and 17
    // E0596s.
    if (_sharedMutation(method)) return '&mut self';
    if (!_mutating.contains(_rustName(method))) return '&self';
    if (method.operator != null &&
        operatorTraits.containsKey(method.operator)) {
      throw Unsupported(
        'a field write inside `operator ${method.operator}`',
        'std::ops takes `self`, so the receiver is not this class\'s to change',
      );
    }
    final base = library[cls.superclass];
    if (base != null &&
        base.isAbstract &&
        (base.abstractMethods.any((m) => m.name == method.name) ||
            base.methods.any((m) => m.name == method.name))) {
      throw Unsupported(
        'a field write inside `${method.name}`, which `${base.name}` declares',
        'the receiver belongs to the trait, not to this class',
      );
    }
    return '&mut self';
  }

  /// `_mutating` of every class, by class name, computed once per library.
  static final Expando<Map<String, Set<String>>> _mutatingCache = Expando();

  /// The concrete, uncounted classes under each trait, once per library.
  static final Expando<Map<String, List<IrClass>>> _implementersCache =
      Expando();

  Set<String> _mutatingOf(IrClass other) {
    if (identical(other, cls)) return _mutating;
    final cache = _mutatingCache[library] ??= {};
    return cache[other.name] ??= RustBackend(other, library: library)._mutating;
  }

  Iterable<String?> _supertypeNames(IrClass c) => [
    c.superclass,
    for (final m in c.mixins) m.name,
    for (final i in c.interfaces) i.name,
  ];

  bool _isSubtypeOf(IrClass c, String trait, Set<String> seen) {
    if (c.name == trait) return true;
    if (!seen.add(c.name)) return false;
    for (final name in _supertypeNames(c)) {
      if (name == null) continue;
      if (name == trait) return true;
      final s = library[name];
      if (s != null && _isSubtypeOf(s, trait, seen)) return true;
    }
    return false;
  }

  /// Crate-wide, not this file's: the trait's declaration and an impl in
  /// another file must agree on `&mut self`, and each was deciding from
  /// the implementers it could see (`DirectionalFocusTraversalPolicyMixin
  /// .inDirection`: `&mut` in `focus_traversal`, `&self` in `radio_group`,
  /// 4 errors outside any function at ws373).
  List<IrClass> _implementersOf(String trait) {
    final cache = _implementersCache[library] ??= {};
    return cache[trait] ??= [
      for (final c in {
        for (final c in library.elsewhere.values) c.name: c,
        for (final c in library.classes) c.name: c,
      }.values)
        if (!c.isAbstract &&
            !c.counted &&
            c.name != trait &&
            _isSubtypeOf(c, trait, {}))
          c,
    ];
  }

  /// Whether a class implementing a trait that declares this method writes
  /// a field in it. See `_receiverOf`.
  bool _sharedMutation(IrMethod method) {
    if (method.isStatic) return false;
    final name = _rustName(method);
    final traits = <String>{};
    void collect(IrClass c, Set<String> seen) {
      if (!seen.add(c.name)) return;
      if (c.isAbstract &&
          (c.methods.any((m) => m.name == method.name) ||
              c.abstractMethods.any((m) => m.name == method.name))) {
        traits.add(c.name);
      }
      for (final n in _supertypeNames(c)) {
        final s = library[n];
        if (s != null) collect(s, seen);
      }
    }

    collect(cls, {});
    return traits.any(
      (t) => _implementersOf(t).any((c) => _mutatingOf(c).contains(name)),
    );
  }

  /// A member's name in Rust.
  ///
  /// `get x` and `set x` are the same name in Dart and cannot be in Rust, so a
  /// setter becomes `set_x`. Everything keyed by member -- the mutability set
  /// especially -- keys on *this* name, because keying on the Dart name would
  /// make a getter and its setter one entry and mark the getter `&mut self`.
  String _rustName(IrMethod method) =>
      method.isSetter ? 'set_${snake(method.name)}' : _identifier(method.name);
}
