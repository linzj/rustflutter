// Which fields would have to be shared, if only the ones a closure needs were.
//
// A closure that outlives its call cannot borrow `this`. For a `final` field a
// copy is the field (round 97). For a *mutable* one it is not: the closure and
// the object have to see the same cell. The question this answers is how many
// fields that is -- per-field sharing instead of per-object.
import 'package:kernel/ast.dart';
import 'package:kernel/kernel.dart';

class _ReadsInClosure extends RecursiveVisitor {
  final mutable = <String>{};
  final finals = <String>{};
  bool calls = false;
  bool writes = false;

  @override
  void visitInstanceGet(InstanceGet node) {
    if (node.receiver is ThisExpression) {
      final target = node.interfaceTarget;
      if (target is Field) {
        (target.isFinal ? finals : mutable).add(
          '${target.enclosingClass?.name}.${target.name.text}',
        );
      }
    } else {
      node.receiver.accept(this);
    }
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    if (node.receiver is ThisExpression) {
      writes = true;
      final target = node.interfaceTarget;
      if (target is Field) {
        mutable.add('${target.enclosingClass?.name}.${target.name.text}');
      }
    } else {
      node.receiver.accept(this);
    }
    node.value.accept(this);
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    if (node.receiver is ThisExpression)
      calls = true;
    else
      node.receiver.accept(this);
    node.arguments.accept(this);
  }
}

class _Closures extends RecursiveVisitor {
  final found = <FunctionNode>[];
  @override
  void visitFunctionExpression(FunctionExpression node) {
    found.add(node.function);
    super.visitFunctionExpression(node);
  }
}

void main(List<String> args) {
  final c = loadComponentFromBinary(args.first);
  final shared = <String>{};
  final alsoCalls = <String>{};
  var closures = 0, needing = 0, callers = 0;
  for (final lib in c.libraries) {
    if (!lib.importUri.toString().startsWith('package:')) continue;
    for (final cls in lib.classes) {
      for (final member in cls.members) {
        final walk = _Closures();
        member.accept(walk);
        for (final fn in walk.found) {
          closures++;
          final reads = _ReadsInClosure();
          fn.accept(reads);
          if (reads.mutable.isEmpty && !reads.calls) continue;
          if (reads.calls) {
            callers++;
            alsoCalls.add('${cls.name}');
          }
          if (reads.mutable.isNotEmpty) {
            needing++;
            shared.addAll(reads.mutable);
          }
        }
      }
    }
  }
  print('closures inside classes: $closures');
  print('  needing a shared *field*: $needing');
  print('  calling a method on `this` (needs the object): $callers');
  print('distinct fields that would be shared: ${shared.length}');
  print('classes whose methods a closure calls: ${alsoCalls.length}');
}
