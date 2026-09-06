// Type parameters the program uses covariantly.
//
// Dart's generics are covariant: a `Route<void>` is a `Route<dynamic>`, and
// the navigator keeps every route as one. Rust's trait objects are not: an
// `Rc<dyn Route<()>>` is no `Rc<dyn Route<Rc<dyn Object>>>`, and nothing
// turns one into the other short of a forwarding object -- a new identity,
// where routes are compared by identity. So a parameter the program ever
// uses covariantly -- a value of `C<A>` reaching a slot of `C<B>` with `A`
// and `B` different -- is *erased*: spelled as its bound, an `Rc<dyn
// Object>`, the values converted at the boundaries by the rules an erased
// bound already has (`_erasedParameter`, `FromDynamic`). Found once over
// the whole component, at every flow site: an argument into a parameter,
// an initialiser or an assignment into a declared type, a return into the
// function's, an element into a literal's, a conditional's arms. Not a
// cast: `this as Animation<double>` is a runtime check the cast table
// answers by instantiation, and counting it erased `Animation<T>`.
// Then handed down: a subclass parameter put in an erased position of its
// supertype (`MaterialPageRoute<T> extends PageRoute<T>`) is erased too.
import 'dart:io';

import 'package:kernel/ast.dart';
import 'package:kernel/type_algebra.dart';
import 'package:kernel/type_environment.dart';

bool _translated(Class c) {
  final uri = c.enclosingLibrary.importUri;
  return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
}

/// The type parameters of `libraries`' classes used covariantly.
Set<TypeParameter> covariantParameters(
  Iterable<Library> libraries,
  TypeEnvironment environment,
) {
  final found = <TypeParameter>{};
  final scan = _FlowScan(environment, found);
  for (final library in libraries) {
    library.accept(scan);
  }
  // Handed down to a subclass parameter in an erased position, to a fixpoint.
  final classes = [for (final library in libraries) ...library.classes];
  var changed = true;
  while (changed) {
    changed = false;
    for (final cls in classes) {
      for (final above in [
        if (cls.supertype != null) cls.supertype!,
        if (cls.mixedInType != null) cls.mixedInType!,
        ...cls.implementedTypes,
      ]) {
        final params = above.classNode.typeParameters;
        for (
          var i = 0;
          i < above.typeArguments.length && i < params.length;
          i++
        ) {
          if (!found.contains(params[i])) continue;
          final arg = above.typeArguments[i];
          if (arg is TypeParameterType &&
              cls.typeParameters.contains(arg.parameter) &&
              found.add(arg.parameter)) {
            changed = true;
          }
        }
      }
    }
  }
  if (Platform.environment['DART2RUST_TRACE_COVARIANT'] != null) {
    for (final p in found) {
      final owner = p.declaration;
      stderr.writeln(
        'TRACE_COVARIANT ${owner is Class ? owner.name : owner} <${p.name}>',
      );
    }
  }
  return found;
}

class _FlowScan extends RecursiveVisitor {
  _FlowScan(this.environment, this.found);

  final TypeEnvironment environment;
  final Set<TypeParameter> found;

  StaticTypeContext? _context;
  final List<FunctionNode> _functions = [];
  Member? _member;

  String _where() {
    final m = _member;
    if (m == null) return '?';
    return '${m.enclosingClass?.name ?? m.enclosingLibrary.name}.${m.name.text}';
  }

  @override
  void visitProcedure(Procedure node) {
    _context = StaticTypeContext(node, environment);
    _member = node;
    super.visitProcedure(node);
    _context = null;
  }

  @override
  void visitConstructor(Constructor node) {
    _context = StaticTypeContext(node, environment);
    _member = node;
    super.visitConstructor(node);
    _context = null;
  }

  @override
  void visitField(Field node) {
    _context = StaticTypeContext(node, environment);
    _member = node;
    final init = node.initializer;
    if (init != null) _flow(init, node.type);
    super.visitField(node);
    _context = null;
  }

  @override
  void visitFunctionNode(FunctionNode node) {
    _functions.add(node);
    super.visitFunctionNode(node);
    _functions.removeLast();
  }

  DartType? _typeOf(Expression e) {
    final context = _context;
    if (context == null) return null;
    try {
      return e.getStaticType(context);
    } catch (_) {
      return null;
    }
  }

  void _flow(Expression value, DartType slot) {
    final have = _typeOf(value);
    if (have == null) return;
    _compare(have, slot);
  }

  /// `have` reaching a slot of `slot`: every parameter whose arguments
  /// differ between the two, at any depth, is used covariantly.
  void _compare(DartType have, DartType slot, [int depth = 0]) {
    if (depth > 6) return;
    if (have is FutureOrType) return _compare(have.typeArgument, slot, depth);
    if (slot is FutureOrType) return _compare(have, slot.typeArgument, depth);
    if (have is InterfaceType && slot is InterfaceType) {
      final asSlot = environment.hierarchy.getTypeAsInstanceOf(
        have,
        slot.classNode,
      );
      if (asSlot == null) return;
      final params = slot.classNode.typeParameters;
      for (
        var i = 0;
        i < asSlot.typeArguments.length &&
            i < slot.typeArguments.length &&
            i < params.length;
        i++
      ) {
        final a = asSlot.typeArguments[i];
        final b = slot.typeArguments[i];
        if (_same(a, b)) continue;
        if (_translated(slot.classNode) && found.add(params[i])) {
          if (Platform.environment['DART2RUST_TRACE_COVARIANT'] != null) {
            stderr.writeln(
              'TRACE_COVARIANT_SITE ${slot.classNode.name}<${params[i].name}> '
              'in ${_where()}: $have -> $slot',
            );
          }
        }
        _compare(a, b, depth + 1);
      }
      return;
    }
    if (have is FunctionType && slot is FunctionType) {
      _compare(have.returnType, slot.returnType, depth + 1);
      for (
        var i = 0;
        i < have.positionalParameters.length &&
            i < slot.positionalParameters.length;
        i++
      ) {
        _compare(
          have.positionalParameters[i],
          slot.positionalParameters[i],
          depth + 1,
        );
      }
    }
  }

  /// The same type, nullability aside; Dart's two top types are one type
  /// here (`Object?` is `dynamic`).
  static bool _same(DartType a, DartType b) {
    if (_top(a) && _top(b)) return true;
    // A closure's own parameter and its structural copy in the closure's
    // type are one parameter (`<T>(..) => MaterialPageRoute<T>(..)` into
    // a `PageRoute<T> Function<T>(..)` slot).
    final aName = _parameterName(a), bName = _parameterName(b);
    if (aName != null && bName != null) return aName == bName;
    return a.withDeclaredNullability(Nullability.nonNullable) ==
        b.withDeclaredNullability(Nullability.nonNullable);
  }

  static String? _parameterName(DartType t) => t is TypeParameterType
      ? t.parameter.name
      : t is StructuralParameterType
      ? t.parameter.name
      : null;

  static bool _top(DartType t) =>
      t is DynamicType ||
      (t is InterfaceType &&
          t.classNode.name == 'Object' &&
          t.classNode.enclosingLibrary.importUri.toString() == 'dart:core');

  void _arguments(Arguments arguments, FunctionType type) {
    final positional = type.positionalParameters;
    for (
      var i = 0;
      i < arguments.positional.length && i < positional.length;
      i++
    ) {
      _flow(arguments.positional[i], positional[i]);
    }
    for (final named in arguments.named) {
      for (final p in type.namedParameters) {
        if (p.name == named.name) _flow(named.value, p.type);
      }
    }
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    _arguments(node.arguments, node.functionType);
    super.visitInstanceInvocation(node);
  }

  /// The declared parameter types, substituted: `computeFunctionType`
  /// copies a callee's own parameters into structural ones, which then
  /// compare unequal to the declaration's and marked every generic
  /// constructor call covariant.
  void _declaredArguments(
    Arguments arguments,
    FunctionNode fn,
    Substitution substitution,
  ) {
    final positional = fn.positionalParameters;
    for (
      var i = 0;
      i < arguments.positional.length && i < positional.length;
      i++
    ) {
      _flow(
        arguments.positional[i],
        substitution.substituteType(positional[i].type),
      );
    }
    for (final named in arguments.named) {
      for (final p in fn.namedParameters) {
        if (p.parameterName == named.name) {
          _flow(named.value, substitution.substituteType(p.type));
        }
      }
    }
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    final fn = node.target.function;
    final substitution = fn.typeParameters.isEmpty
        ? Substitution.empty
        : Substitution.fromPairs(
            fn.typeParameters,
            fn.typeParameters.length == node.arguments.types.length
                ? node.arguments.types
                : [for (final p in fn.typeParameters) p.bound],
          );
    _declaredArguments(node.arguments, fn, substitution);
    super.visitStaticInvocation(node);
  }

  @override
  void visitConstructorInvocation(ConstructorInvocation node) {
    _declaredArguments(
      node.arguments,
      node.target.function,
      Substitution.fromInterfaceType(node.constructedType),
    );
    super.visitConstructorInvocation(node);
  }

  @override
  void visitFunctionInvocation(FunctionInvocation node) {
    final type = node.functionType;
    if (type != null) _arguments(node.arguments, type);
    super.visitFunctionInvocation(node);
  }

  @override
  void visitVariableDeclaration(VariableDeclaration node) {
    final init = node.initializer;
    if (init != null) _flow(init, node.variable.type);
    super.visitVariableDeclaration(node);
  }

  @override
  void visitVariableSet(VariableSet node) {
    _flow(node.value, node.variable.type);
    super.visitVariableSet(node);
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    final target = node.interfaceTarget;
    final owner = target.enclosingClass;
    final receiverType = _typeOf(node.receiver);
    var slot = target.setterType;
    if (owner != null && receiverType is InterfaceType) {
      final asOwner = environment.hierarchy.getTypeAsInstanceOf(
        receiverType,
        owner,
      );
      if (asOwner != null) {
        slot = Substitution.fromTypeDeclarationType(asOwner)
            .substituteType(slot);
      }
    }
    _flow(node.value, slot);
    super.visitInstanceSet(node);
  }

  @override
  void visitStaticSet(StaticSet node) {
    _flow(node.value, node.target.setterType);
    super.visitStaticSet(node);
  }

  @override
  void visitFieldInitializer(FieldInitializer node) {
    _flow(node.value, node.field.type);
    super.visitFieldInitializer(node);
  }

  @override
  void visitReturnStatement(ReturnStatement node) {
    final value = node.expression;
    if (value != null && _functions.isNotEmpty) {
      final fn = _functions.last;
      var slot = fn.returnType;
      if (fn.asyncMarker == AsyncMarker.Async) {
        slot = slot is InterfaceType && slot.typeArguments.length == 1
            ? slot.typeArguments.single
            : slot is FutureOrType
            ? slot.typeArgument
            : const DynamicType();
      }
      if (fn.asyncMarker == AsyncMarker.Sync) _flow(value, slot);
    }
    super.visitReturnStatement(node);
  }

  @override
  void visitListLiteral(ListLiteral node) {
    for (final e in node.expressions) {
      _flow(e, node.typeArgument);
    }
    super.visitListLiteral(node);
  }

  @override
  void visitSetLiteral(SetLiteral node) {
    for (final e in node.expressions) {
      _flow(e, node.typeArgument);
    }
    super.visitSetLiteral(node);
  }

  @override
  void visitMapLiteral(MapLiteral node) {
    for (final e in node.entries) {
      _flow(e.key, node.keyType);
      _flow(e.value, node.valueType);
    }
    super.visitMapLiteral(node);
  }

  @override
  void visitConditionalExpression(ConditionalExpression node) {
    _flow(node.then, node.staticType);
    _flow(node.otherwise, node.staticType);
    super.visitConditionalExpression(node);
  }
}
