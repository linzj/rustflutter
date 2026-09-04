// Whole-program failure analysis for the Result model.
//
// See STATUS.md, "决定（2026-09-04）：恢复 `Result`". A member *fails* when
// it can complete with an exception: its body throws where no enclosing
// catch-all `try` catches, or it calls a member that fails from such a
// place. Instance calls dispatch to every implementer of the interface
// target in the closed world, so a trait method fails when any
// implementer does and its signature is one for all of them.
//
// Function values (closures, tear-offs) are tracked separately for now:
// a call through a function value does not propagate failure in this
// first version, and the census counts how often that happens.
import 'package:kernel/class_hierarchy.dart';
import 'package:kernel/kernel.dart';

class ThrowsAnalysis {
  ThrowsAnalysis._(
    this.members,
    this.direct,
    this.failing,
    this.rounds,
    this.failingFunctionValues,
    this.functionValueCalls,
    this._callers,
    this._dispatch,
    this._hierarchy,
  );

  final Set<Member> Function(Member) _dispatch;
  final ClosedWorldClassHierarchy _hierarchy;
  final Map<Member, bool> _familyCache = {};

  /// Whether the *signature* of `m` is a failing one: it, an override of
  /// it anywhere below, or the interface member it overrides, fails. A
  /// trait method has one signature for every implementer, so one failing
  /// implementer puts `Result` on all of them.
  bool familyFails(Member m) {
    return _familyCache.putIfAbsent(m, () {
      final cls = m.enclosingClass;
      if (cls == null) return failing.contains(m);
      final name = m.name;
      final setter = m is Procedure && m.isSetter;
      // Every class above (itself included) that declares the name; the
      // dispatch set below each of those is the family.
      final tops = <Class>{};
      final seen = <Class>{};
      void up(Class c) {
        if (!seen.add(c)) return;
        for (final member in c.members) {
          if (member.name == name &&
              (member is Field ||
                  (member is Procedure && member.isSetter == setter))) {
            tops.add(c);
            break;
          }
        }
        for (final s in c.supers) {
          up(s.classNode);
        }
      }

      up(cls);
      for (final top in tops) {
        for (final member in top.members) {
          if (member.name != name) continue;
          if (member is Procedure && member.isSetter != setter) continue;
          if (_dispatch(member).any(failing.contains)) return true;
        }
      }
      return false;
    });
  }

  /// Callee -> members calling it (outside any catch-all).
  final Map<Member, Set<Member>> _callers;

  /// The members that fail through `root`: everything that reaches it
  /// through the call graph. Sets for different roots overlap.
  Set<Member> infectedBy(Member root) {
    final seen = <Member>{root};
    final queue = [root];
    while (queue.isNotEmpty) {
      final m = queue.removeLast();
      for (final caller in _callers[m] ?? const <Member>{}) {
        if (seen.add(caller)) queue.add(caller);
      }
    }
    return seen;
  }

  /// Every member the analysis looked at (translated libraries only).
  final List<Member> members;
  int get considered => members.length;

  /// Members whose own body throws outside a catch-all.
  final Set<Member> direct;

  /// Members that can fail, after propagation.
  final Set<Member> failing;

  final int rounds;
  final int failingFunctionValues;
  final int functionValueCalls;

  bool fails(Member m) => failing.contains(m);

  static bool _translated(Library lib, List<String> prefixes) {
    final uri = lib.importUri.toString();
    return prefixes.any(uri.startsWith);
  }

  static ThrowsAnalysis of(
    Component component,
    ClosedWorldClassHierarchy hierarchy,
    List<String> prefixes,
  ) {
    final members = <Member>[];
    for (final lib in component.libraries) {
      if (!_translated(lib, prefixes)) continue;
      members.addAll(lib.procedures);
      for (final f in lib.fields) {
        if (f.initializer != null) members.add(f);
      }
      for (final cls in lib.classes) {
        members.addAll(cls.procedures);
        members.addAll(cls.constructors);
        for (final f in cls.fields) {
          if (f.initializer != null) members.add(f);
        }
      }
    }
    final considered = members.toSet();
    final direct = <Member>{};
    final calls = <Member, Set<Member>>{};
    var failingFunctionValues = 0;
    var functionValueCalls = 0;
    // Dispatch targets, cached per (class, name): every concrete subclass's
    // member of that name.
    final implementers = <String, Set<Member>>{};
    final subtypes = hierarchy.computeSubtypesInformation();
    Set<Member> dispatch(Member target) {
      final cls = target.enclosingClass;
      if (cls == null) return {target};
      final key =
          '${cls.reference}#${target.name.text}#${target is Procedure && target.isSetter}';
      return implementers.putIfAbsent(key, () {
        final out = <Member>{target};
        final setter = target is Procedure && target.isSetter;
        for (final sub in subtypes.getSubtypesOf(cls)) {
          final found = hierarchy.getDispatchTarget(
            sub,
            target.name,
            setter: setter,
          );
          if (found != null) out.add(found);
        }
        return out;
      });
    }

    for (final m in members) {
      final finder = _CallFinder(dispatch);
      m.accept(finder);
      if (finder.throwsDirectly) direct.add(m);
      calls[m] = finder.callees.where(considered.contains).toSet();
      failingFunctionValues += finder.failingClosures;
      functionValueCalls += finder.functionValueCalls;
    }
    final failing = <Member>{...direct};
    var rounds = 0;
    var changed = true;
    while (changed) {
      changed = false;
      rounds++;
      for (final entry in calls.entries) {
        if (failing.contains(entry.key)) continue;
        if (entry.value.any(failing.contains)) {
          failing.add(entry.key);
          changed = true;
        }
      }
    }
    final callers = <Member, Set<Member>>{};
    for (final entry in calls.entries) {
      for (final callee in entry.value) {
        callers.putIfAbsent(callee, () => {}).add(entry.key);
      }
    }
    return ThrowsAnalysis._(
      members,
      direct,
      failing,
      rounds,
      failingFunctionValues,
      functionValueCalls,
      callers,
      dispatch,
      hierarchy,
    );
  }
}

/// Throws and callees of one member's body, outside any catch-all `try`.
class _CallFinder extends RecursiveVisitor {
  _CallFinder(this.dispatch);

  final Set<Member> Function(Member) dispatch;
  bool throwsDirectly = false;
  final callees = <Member>{};
  int failingClosures = 0;
  int functionValueCalls = 0;
  int _caught = 0;
  int _closureDepth = 0;
  bool _closureThrows = false;

  static bool _catchesAll(TryCatch node) => node.catches.any((c) {
    final guard = c.guard;
    return guard is DynamicType ||
        (guard is InterfaceType && guard.classNode.name == 'Object');
  });

  @override
  void visitTryCatch(TryCatch node) {
    if (_catchesAll(node)) {
      _caught++;
      node.body.accept(this);
      _caught--;
      // A catch body runs outside the try: its throws and rethrows escape.
      for (final c in node.catches) {
        c.accept(this);
      }
      return;
    }
    super.visitTryCatch(node);
  }

  void _throw() {
    if (_caught > 0) return;
    if (_closureDepth > 0) {
      _closureThrows = true;
    } else {
      throwsDirectly = true;
    }
  }

  void _call(Member target) {
    if (_caught > 0) return;
    if (_closureDepth > 0) {
      // A call inside a closure: the closure fails when the callee does,
      // which the closure's own fixed point would decide. Counted as the
      // member's callee for now -- the closure runs in some member.
    }
    callees.addAll(dispatch(target));
  }

  @override
  void visitThrow(Throw node) {
    // Not the AOT compiler's marker for code it removed: that body never
    // runs, so it never fails.
    final thrown = node.expression;
    final removed =
        thrown is StringLiteral &&
        thrown.value.startsWith('Attempt to execute ') &&
        thrown.value.contains('removed by Dart AOT');
    if (!removed) _throw();
    super.visitThrow(node);
  }

  @override
  void visitRethrow(Rethrow node) {
    _throw();
    super.visitRethrow(node);
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    _call(node.target);
    super.visitStaticInvocation(node);
  }

  @override
  void visitConstructorInvocation(ConstructorInvocation node) {
    _call(node.target);
    super.visitConstructorInvocation(node);
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    _call(node.interfaceTarget);
    super.visitInstanceInvocation(node);
  }

  @override
  void visitInstanceGet(InstanceGet node) {
    _call(node.interfaceTarget);
    super.visitInstanceGet(node);
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    _call(node.interfaceTarget);
    super.visitInstanceSet(node);
  }

  @override
  void visitSuperMethodInvocation(SuperMethodInvocation node) {
    _call(node.interfaceTarget);
    super.visitSuperMethodInvocation(node);
  }

  @override
  void visitSuperPropertyGet(SuperPropertyGet node) {
    _call(node.interfaceTarget);
    super.visitSuperPropertyGet(node);
  }

  @override
  void visitStaticGet(StaticGet node) {
    // A static getter or a lazily initialised static runs code.
    _call(node.target);
    super.visitStaticGet(node);
  }

  @override
  void visitFunctionInvocation(FunctionInvocation node) {
    functionValueCalls++;
    super.visitFunctionInvocation(node);
  }

  @override
  void visitLocalFunctionInvocation(LocalFunctionInvocation node) {
    functionValueCalls++;
    super.visitLocalFunctionInvocation(node);
  }

  @override
  void visitFunctionExpression(FunctionExpression node) {
    _closureDepth++;
    final was = _closureThrows;
    _closureThrows = false;
    super.visitFunctionExpression(node);
    if (_closureThrows) failingClosures++;
    _closureThrows = was;
    _closureDepth--;
  }
}
