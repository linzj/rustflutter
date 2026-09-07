// Classes mutated through an alias.
//
// A Dart object is a reference: `writeValue(buffer, value)` fills the
// caller's `WriteBuffer`. A translated class is a Rust *value* unless the
// front end counts it (`_counted`), and a value handed to a method is a
// copy -- the callee filled the copy, and every platform message came out
// empty (run509). This pass finds, once over the whole component, every
// translated class that has a mutating member -- one writing a field of
// `this`, calling a mutating method of a field of `this`, or calling
// another such member -- invoked on a receiver that is not `this`. Such a
// class is counted: an identity, shared by handle, is what its Dart is.
import 'package:kernel/ast.dart';

/// Members of the prelude's collections and byte views that change the
/// receiver, by Dart name.
const _mutatingNames = {
  '[]=',
  'add',
  'addAll',
  'insert',
  'insertAll',
  'remove',
  'removeAt',
  'removeLast',
  'removeWhere',
  'retainWhere',
  'clear',
  'setRange',
  'fillRange',
  'replaceRange',
  'removeRange',
  'setAll',
  'sort',
  'shuffle',
  'addFirst',
  'addLast',
  'removeFirst',
  'putIfAbsent',
  'update',
  'updateAll',
  'setInt8',
  'setUint8',
  'setInt16',
  'setUint16',
  'setInt32',
  'setUint32',
  'setInt64',
  'setUint64',
  'setFloat32',
  'setFloat64',
};

bool _translated(Class c) {
  final uri = c.enclosingLibrary.importUri;
  return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
}

/// The classes of `libraries` mutated through an alias (see the file's
/// comment).
Set<Class> aliasMutatedClasses(Iterable<Library> libraries) {
  // Step one: each class's mutating members, to a fixpoint over the calls
  // they make on `this`.
  final mutators = <Class, Set<Member>>{};
  final direct = <Class, Map<Procedure, _MutatorFinder>>{};
  for (final library in libraries) {
    for (final cls in library.classes) {
      if (!_translated(cls)) continue;
      final found = <Procedure, _MutatorFinder>{};
      for (final p in cls.procedures) {
        if (p.isStatic || p.isAbstract || p.function.body == null) continue;
        final finder = _MutatorFinder();
        p.function.body!.accept(finder);
        found[p] = finder;
      }
      direct[cls] = found;
      mutators[cls] = {
        for (final e in found.entries)
          if (e.value.writes) e.key,
        // A setter writes by definition; so does a field's own setter.
        for (final p in cls.procedures)
          if (p.isSetter && !p.isStatic) p,
        for (final f in cls.fields)
          if (!f.isFinal && !f.isStatic) f,
      };
    }
  }
  var changed = true;
  while (changed) {
    changed = false;
    for (final entry in direct.entries) {
      final set = mutators[entry.key]!;
      for (final e in entry.value.entries) {
        if (set.contains(e.key)) continue;
        if (e.value.thisCalls.any(
          (m) => mutators[m.enclosingClass]?.contains(m) ?? false,
        )) {
          set.add(e.key);
          changed = true;
        }
      }
    }
  }
  // Step two: every invocation of such a member on a receiver that is not
  // `this`.
  final out = <Class>{};
  final scan = _AliasScan(mutators, out);
  for (final library in libraries) {
    library.accept(scan);
  }
  return out;
}

class _MutatorFinder extends RecursiveVisitor {
  bool writes = false;
  final thisCalls = <Member>{};

  static bool _onThis(Expression receiver) =>
      receiver is ThisExpression ||
      (receiver is InstanceGet && receiver.receiver is ThisExpression);

  @override
  void visitInstanceSet(InstanceSet node) {
    if (_onThis(node.receiver)) writes = true;
    super.visitInstanceSet(node);
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    if (node.receiver is ThisExpression) {
      thisCalls.add(node.interfaceTarget);
    } else if (_onThis(node.receiver) &&
        _mutatingNames.contains(node.name.text)) {
      writes = true;
    }
    super.visitInstanceInvocation(node);
  }

  @override
  void visitFunctionNode(FunctionNode node) {
    // A closure inside: its writes count as the method's.
    super.visitFunctionNode(node);
  }
}

class _AliasScan extends RecursiveVisitor {
  _AliasScan(this.mutators, this.out);
  final Map<Class, Set<Member>> mutators;
  final Set<Class> out;

  /// The functions being walked, innermost last: a local written to from
  /// a *nested* function was captured, and the write must reach the
  /// enclosing function's object (`flag.value = true` inside a callback,
  /// the dynfall fixture).
  final List<FunctionNode> _functions = [];

  @override
  void visitFunctionNode(FunctionNode node) {
    _functions.add(node);
    super.visitFunctionNode(node);
    _functions.removeLast();
  }

  /// Whether `v` is a local of the innermost function being walked.
  bool _ownLocal(Variable v) {
    if (v.parent is FunctionNode) return false;
    TreeNode? p = v.parent;
    while (p != null && p is! FunctionNode) {
      p = p.parent;
    }
    return _functions.isNotEmpty && identical(p, _functions.last);
  }

  void _mark(Member target, Expression receiver) {
    if (receiver is ThisExpression) return;
    final cls = target.enclosingClass;
    if (cls == null || !_translated(cls)) return;
    if (mutators[cls]?.contains(target) ?? false) out.add(cls);
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    _mark(node.interfaceTarget, node.receiver);
    super.visitInstanceInvocation(node);
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    _mark(node.interfaceTarget, node.receiver);
    // A *field* written through another object's reference -- a
    // parameter, a field, a call's result -- is the same alias mutation
    // a mutating method is: `entry._owner = this` in `LocalHistoryRoute.
    // addLocalHistoryEntry` fills the caller's entry (run672). A local
    // that owns the value is Rust's `let mut`, and `this`'s own fields
    // are `&mut self`: neither needs the class counted.
    final target = node.interfaceTarget;
    final receiver = node.receiver;
    if (target is Field) {
      final localOwner =
          receiver is VariableGet && _ownLocal(receiver.variable);
      final onThis =
          receiver is ThisExpression ||
          (receiver is InstanceGet && receiver.receiver is ThisExpression);
      final cls = target.enclosingClass;
      if (!localOwner && !onThis && cls != null && _translated(cls)) {
        out.add(cls);
      }
    }
    super.visitInstanceSet(node);
  }
}
