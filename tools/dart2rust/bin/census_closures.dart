// What the closures that reach `this` actually do with it.
//
// `closure capturing \`this\`` is the largest single refusal -- 792 of them,
// plus 495 torn-off methods, which is the same thing written shorter. The
// standing plan is an ownership arrangement, and the arrangements differ a
// lot in cost:
//
//   * only reads fields of `this`  ->  `Rc<Self>` is enough
//   * writes a field, or calls a method that writes one  ->  `RefCell` too
//   * reaches a *local* as well  ->  the local has to be moved or shared
//
// So the question worth answering before building any of it is which of those
// the 792 are. Rounds 40 and 44 both saved a lot of work by measuring first
// and then not building the thing; this is the same move.
//
//     dart run --packages=<kernel config> \
//         tools/dart2rust/bin/census_closures.dart app.dill [package:] \
//         [--examples]
//
// Build the dill and the config with `bin/dill.py`.

import 'dart:io';

import 'package:kernel/ast.dart';
import 'package:kernel/kernel.dart';

/// What one closure needs from `this`.
///
/// Ordered by cost: a closure is filed under the most expensive thing it does,
/// because that is what decides the arrangement it needs.
enum Need {
  /// Never mentions `this`. Already translated; counted to keep the total
  /// honest about what share the refusal really is.
  none('captures nothing'),

  /// Reads fields only: `() => _controller.value`.
  reads('reads fields of `this`'),

  /// Calls a method on `this`: `() => setState(..)`. Whether that method
  /// mutates is a question for the `_mutating` fixpoint, not for this ruler --
  /// filed above reads because it may.
  calls('calls a method on `this`'),

  /// Assigns a field: `() { _value = x; }`. Needs interior mutability.
  writes('writes a field of `this`');

  const Need(this.label);
  final String label;
}

class _Visit extends RecursiveVisitor {
  bool reads = false;
  bool calls = false;
  bool writes = false;

  /// Nested closures count as part of the one being examined: their capture is
  /// the outer closure's problem too.
  @override
  void visitThisExpression(ThisExpression node) {
    // Bare `this`, handed to something else. The most demanding case there is,
    // so it is filed as a call rather than a read.
    calls = true;
  }

  @override
  void visitInstanceGet(InstanceGet node) {
    if (node.receiver is ThisExpression) {
      reads = true;
    } else {
      node.receiver.accept(this);
    }
    // The interface target is not visited: a field read is the read, and its
    // declaration is somewhere else entirely.
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    if (node.receiver is ThisExpression) {
      writes = true;
    } else {
      node.receiver.accept(this);
    }
    node.value.accept(this);
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    if (node.receiver is ThisExpression) {
      calls = true;
    } else {
      node.receiver.accept(this);
    }
    node.arguments.accept(this);
  }

  @override
  void visitInstanceTearOff(InstanceTearOff node) {
    if (node.receiver is ThisExpression) {
      calls = true;
    } else {
      node.receiver.accept(this);
    }
  }

  @override
  void visitSuperMethodInvocation(SuperMethodInvocation node) {
    calls = true;
    node.arguments.accept(this);
  }

  @override
  void visitSuperPropertyGet(SuperPropertyGet node) => reads = true;

  @override
  void visitSuperPropertySet(SuperPropertySet node) {
    writes = true;
    node.value.accept(this);
  }

  Need get need {
    if (writes) return Need.writes;
    if (calls) return Need.calls;
    if (reads) return Need.reads;
    return Need.none;
  }
}

/// Every closure written inside a member, including nested ones.
class _Closures extends RecursiveVisitor {
  _Closures(this.found);

  final List<FunctionNode> found;

  @override
  void visitFunctionExpression(FunctionExpression node) {
    found.add(node.function);
    super.visitFunctionExpression(node);
  }

  @override
  void visitFunctionDeclaration(FunctionDeclaration node) {
    found.add(node.function);
    super.visitFunctionDeclaration(node);
  }
}

void main(List<String> args) {
  if (args.isEmpty) {
    stderr.writeln(
      'usage: census_closures.dart <app.dill> [uri prefix] [--examples]',
    );
    exit(2);
  }
  final prefix = args.length > 1 && !args[1].startsWith('--')
      ? args[1]
      : 'package:';
  final examples = args.contains('--examples');

  final component = loadComponentFromBinary(args.first);
  final counts = {for (final n in Need.values) n: 0};
  final samples = {for (final n in Need.values) n: <String>[]};
  var members = 0;

  for (final library in component.libraries) {
    if (!library.importUri.toString().startsWith(prefix)) continue;
    for (final cls in library.classes) {
      for (final member in cls.members) {
        members++;
        final closures = <FunctionNode>[];
        member.accept(_Closures(closures));
        for (final closure in closures) {
          final visit = _Visit();
          closure.accept(visit);
          counts[visit.need] = counts[visit.need]! + 1;
          final where = samples[visit.need]!;
          if (examples && where.length < 5) {
            where.add('${cls.name}.${member.name.text}');
          }
        }
      }
    }
  }

  final total = counts.values.fold(0, (a, b) => a + b);
  print('$prefix: $members members, $total closures inside classes\n');
  for (final need in Need.values) {
    final n = counts[need]!;
    final share = total == 0 ? 0 : (n * 100 / total).round();
    print('${n.toString().padLeft(6)}  $share%  ${need.label}');
    for (final sample in samples[need]!) {
      print('          $sample');
    }
  }

  final reaching = total - counts[Need.none]!;
  final shared = counts[Need.reads]!;
  print('');
  print('reaching `this`: $reaching');
  if (reaching > 0) {
    final share = (shared * 100 / reaching).round();
    print('of those, read-only: $shared ($share%) -- `Rc<Self>` would do');
  }
}
