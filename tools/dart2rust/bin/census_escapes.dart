// Of the closures handed to a call, how many does the callee *keep*?
//
// A borrowing closure is only sound if the callee is finished with it when it
// returns. If the callee stores it -- a listener list, a widget field -- the
// closure has to own what it captures, and borrowing cannot express that.
import 'package:kernel/ast.dart';
import 'package:kernel/kernel.dart';

/// Every use of a parameter that is not "call it right here".
class _Escapes extends RecursiveVisitor {
  _Escapes(this.param);
  final Object param;
  bool escapes = false;
  int calls = 0;

  @override
  void visitLocalFunctionInvocation(LocalFunctionInvocation node) {
    if (identical(node.variable, param)) {
      calls++;
      node.arguments.accept(this);
      return;
    }
    super.visitLocalFunctionInvocation(node);
  }

  @override
  void visitFunctionInvocation(FunctionInvocation node) {
    final receiver = node.receiver;
    if (receiver is VariableGet && identical(receiver.variable, param)) {
      calls++;
      node.arguments.accept(this);
      return;
    }
    super.visitFunctionInvocation(node);
  }

  @override
  void visitVariableGet(VariableGet node) {
    if (identical(node.variable, param)) escapes = true;
  }
}

void main(List<String> args) {
  final c = loadComponentFromBinary(args.first);
  var handed = 0, kept = 0, calledOnly = 0, unknown = 0;
  final keepers = <String, int>{};

  void look(Arguments arguments, Member? target, String where) {
    final fn = target?.function;
    for (var i = 0; i < arguments.positional.length; i++) {
      if (arguments.positional[i] is! FunctionExpression) continue;
      handed++;
      if (fn == null || i >= fn.positionalParameters.length) {
        unknown++;
        continue;
      }
      final body = fn.body;
      if (body == null) {
        unknown++;
        continue;
      }
      final walk = _Escapes(fn.positionalParameters[i]);
      body.accept(walk);
      if (walk.escapes) {
        kept++;
        final name =
            '${target!.enclosingClass?.name ?? ""}.${target.name.text}';
        keepers[name] = (keepers[name] ?? 0) + 1;
      } else {
        calledOnly++;
      }
    }
  }

  final visitor = _Calls(look);
  for (final lib in c.libraries) {
    if (!lib.importUri.toString().startsWith('package:')) continue;
    lib.accept(visitor);
  }
  print('closures handed to a call: $handed');
  print('  the callee keeps it:     $kept');
  print('  the callee only calls it: $calledOnly');
  print('  cannot tell (no body):   $unknown');
  final top = keepers.entries.toList()
    ..sort((a, b) => b.value.compareTo(a.value));
  for (final e in top.take(8)) {
    print('    ${e.value.toString().padLeft(4)}  ${e.key}');
  }
}

class _Calls extends RecursiveVisitor {
  _Calls(this.look);
  final void Function(Arguments, Member?, String) look;

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    look(node.arguments, node.interfaceTarget, 'instance');
    super.visitInstanceInvocation(node);
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    look(node.arguments, node.target, 'static');
    super.visitStaticInvocation(node);
  }
}
