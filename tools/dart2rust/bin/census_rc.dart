// What `Rc<Self>` would cost: how far the 189 classes reach.
//
// A closure that calls a method on `this` needs the object, not a field. The
// answer is `Rc<Self>` -- and the price is that every construction and every
// place those classes are *held* changes with it. This counts the blast.
import 'package:kernel/ast.dart';
import 'package:kernel/kernel.dart';

class _Closures extends RecursiveVisitor {
  final found = <FunctionNode>[];
  @override
  void visitFunctionExpression(FunctionExpression node) {
    found.add(node.function);
    super.visitFunctionExpression(node);
  }
}

class _CallsThis extends RecursiveVisitor {
  bool calls = false;
  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    if (node.receiver is ThisExpression)
      calls = true;
    else
      node.receiver.accept(this);
    node.arguments.accept(this);
  }

  @override
  void visitInstanceTearOff(InstanceTearOff node) {
    if (node.receiver is ThisExpression) calls = true;
  }
}

void main(List<String> args) {
  final c = loadComponentFromBinary(args.first);
  bool inScope(Library l) => l.importUri.toString().startsWith('package:');

  final needing = <Class>{};
  for (final lib in c.libraries) {
    if (!inScope(lib)) continue;
    for (final cls in lib.classes) {
      for (final member in cls.members) {
        final walk = _Closures();
        member.accept(walk);
        for (final fn in walk.found) {
          final calls = _CallsThis();
          fn.accept(calls);
          if (calls.calls) needing.add(cls);
        }
      }
    }
  }
  final names = {for (final k in needing) k.name};

  var constructions = 0, fields = 0, params = 0, returns = 0, subclasses = 0;
  for (final lib in c.libraries) {
    if (!inScope(lib)) continue;
    for (final cls in lib.classes) {
      var base = cls.superclass;
      while (base != null) {
        if (names.contains(base.name)) {
          subclasses++;
          break;
        }
        base = base.superclass;
      }
      for (final f in cls.fields) {
        final t = f.type;
        if (t is InterfaceType && names.contains(t.classNode.name)) fields++;
      }
      for (final m in cls.procedures) {
        for (final p in m.function.positionalParameters) {
          final t = p.type;
          if (t is InterfaceType && names.contains(t.classNode.name)) params++;
        }
        final r = m.function.returnType;
        if (r is InterfaceType && names.contains(r.classNode.name)) returns++;
      }
    }
    lib.accept(_Constructions(names, () => constructions++));
  }
  print('classes whose closures call a method on `this`: ${needing.length}');
  print('  places one is constructed:  $constructions');
  print('  fields holding one:         $fields');
  print('  parameters taking one:      $params');
  print('  returns giving one:         $returns');
  print('  classes extending one:      $subclasses');
}

class _Constructions extends RecursiveVisitor {
  _Constructions(this.names, this.count);
  final Set<String> names;
  final void Function() count;
  @override
  void visitConstructorInvocation(ConstructorInvocation node) {
    if (names.contains(node.target.enclosingClass.name)) count();
    super.visitConstructorInvocation(node);
  }
}
