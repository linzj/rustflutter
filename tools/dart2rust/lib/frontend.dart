// analyzer's resolved AST -> IR.
//
// Everything here leans on *resolution*. `node.element` is why a call can be
// told from a constructor invocation without guessing from capitalisation, why
// `x` in a method body is known to be a field of this class rather than a
// local, and why a `static const` arrives already evaluated. An unresolved
// parse would have to infer all three, and would be wrong often enough that the
// output could not be trusted.
library;

import 'package:analyzer/dart/ast/ast.dart';
import 'package:analyzer/dart/element/element.dart';
import 'package:analyzer/dart/element/type.dart';

import 'ir.dart';

class Frontend {
  Frontend(this.className);

  final String className;

  /// The superclass of the class currently being lowered, so a `super.foo()`
  /// knows which class's body it means.
  String? _superclass;

  IrType _type(DartType? type) {
    if (type == null) return const IrType('dynamic');
    final nullable = type.nullabilitySuffix.name == 'question';
    final element = type.element;
    final name = element?.name ?? type.getDisplayString();
    return IrType(name ?? 'dynamic', nullable: nullable);
  }

  String? _doc(AnnotatedNode node) {
    final comment = node.documentationComment;
    if (comment == null) return null;
    final lines = <String>[];
    for (final token in comment.tokens) {
      var text = token.lexeme;
      if (text.startsWith('///')) text = text.substring(3);
      lines.add(text.trimRight());
    }
    return lines.join('\n').trim();
  }

  // -- Expressions ------------------------------------------------------------

  IrExpr expression(Expression node) {
    if (node is ParenthesizedExpression) return expression(node.expression);

    if (node is IntegerLiteral) {
      return IrLiteral('${node.value}', const IrType('int'));
    }
    if (node is DoubleLiteral) {
      return IrLiteral('${node.value}', const IrType('double'));
    }
    if (node is BooleanLiteral) {
      return IrLiteral('${node.value}', const IrType('bool'));
    }
    if (node is SimpleStringLiteral) {
      return IrLiteral(node.value, const IrType('String'));
    }
    if (node is NullLiteral) {
      return IrLiteral('null', const IrType('Null', nullable: true));
    }
    if (node is ThisExpression) return const IrThis();

    if (node is SimpleIdentifier) return _identifier(node);
    if (node is PrefixedIdentifier) return _prefixed(node);
    if (node is PropertyAccess) return _property(node);

    if (node is BinaryExpression) {
      return IrBinary(
        node.operator.lexeme,
        expression(node.leftOperand),
        expression(node.rightOperand),
      );
    }
    if (node is PrefixExpression) {
      return IrUnary(node.operator.lexeme, expression(node.operand));
    }
    if (node is ConditionalExpression) {
      return IrConditional(
        expression(node.condition),
        expression(node.thenExpression),
        expression(node.elseExpression),
      );
    }
    if (node is IsExpression) {
      return IrIs(
        expression(node.expression),
        _type(node.type.type),
        negated: node.notOperator != null,
      );
    }
    if (node is AsExpression) {
      // A cast the analyser already checked. The IR has no cast node because
      // the backend has nothing to do with one yet -- the value is the value.
      return expression(node.expression);
    }
    if (node is InstanceCreationExpression) return _construct(node);
    if (node is MethodInvocation) return _invoke(node);

    throw Unsupported('expression ${node.runtimeType}', node.toSource());
  }

  IrExpr _identifier(SimpleIdentifier node) {
    final element = node.element;
    if (element == null) {
      throw Unsupported('unresolved identifier', node.toSource());
    }
    _refusePrivate(node.name, node);
    // A getter or field on this class, referred to without `this.`.
    if (element is PropertyAccessorElement || element is FieldElement) {
      final enclosing = element.enclosingElement;
      if (enclosing is ClassElement) {
        final isStatic = element is FieldElement
            ? element.isStatic
            : (element as PropertyAccessorElement).isStatic;
        if (isStatic) {
          return IrStatic(enclosing.name ?? '?', node.name);
        }
        // A getter is a method in Rust, a field is a field, and Dart spells
        // both `a.x`. Analyzer resolves a *field* read to the field's implicit
        // accessor, so "is it a PropertyAccessorElement" is not the question --
        // `isSynthetic` is. A synthetic accessor is the one the analyser made
        // up for a field; a real `get x => ...` is not synthetic.
        if (element is PropertyAccessorElement && !element.isSynthetic) {
          return IrCall(null, node.name, const []);
        }
        return IrField(null, node.name);
      }
    }
    if (element is FormalParameterElement || element is LocalVariableElement) {
      return IrLocal(node.name);
    }
    throw Unsupported('identifier of ${element.runtimeType}', node.toSource());
  }

  IrExpr _prefixed(PrefixedIdentifier node) {
    final target = node.prefix.element;
    // `Alignment.topLeft` -- the prefix resolves to the class itself.
    if (target is ClassElement) {
      return IrStatic(target.name ?? '?', node.identifier.name);
    }
    final accessor = node.identifier.element;
    if (accessor is PropertyAccessorElement && !accessor.isSynthetic) {
      return IrCall(expression(node.prefix), node.identifier.name, const []);
    }
    return IrField(expression(node.prefix), node.identifier.name);
  }

  IrExpr _property(PropertyAccess node) {
    final target = node.target;
    if (target == null) {
      throw Unsupported('cascade', node.toSource());
    }
    final accessor = node.propertyName.element;
    if (accessor is PropertyAccessorElement && !accessor.isSynthetic) {
      return IrCall(expression(target), node.propertyName.name, const []);
    }
    return IrField(expression(target), node.propertyName.name);
  }

  IrExpr _construct(InstanceCreationExpression node) {
    final type = _type(node.staticType);
    final name = node.constructorName.name?.name;
    return IrNew(
      type,
      _arguments(node.argumentList, node.constructorName.element, node),
      constructor: name,
    );
  }

  /// Lowers an argument list into the callee's **declaration order**.
  ///
  /// Rust has no named arguments, so a Dart call has to be flattened to a
  /// positional one. Flattening it in *call-site* order would be wrong -- Dart
  /// lets `Rect.fromLTRB(top: a, left: b)` name them in any order, and the two
  /// arguments would silently swap. So the order comes from the callee's own
  /// parameter list, which resolution makes reachable from the call site (all
  /// 130 sites in `edge_insets.dart`, when this was checked).
  ///
  /// An omitted optional parameter still has to be passed something, because
  /// the emitted Rust function takes every parameter positionally:
  ///
  /// * an explicit default that is a literal is lowered from its source text
  /// * an omitted nullable parameter becomes `None`, which is the value Dart
  ///   gives it
  /// * anything else stops, rather than inventing a value. A default this
  ///   compiler guessed would be indistinguishable from one upstream wrote,
  ///   which is the failure mode that costs the most to find later.
  List<IrExpr> _arguments(
    ArgumentList list,
    ExecutableElement? callee,
    AstNode site,
  ) {
    final positional = <Expression>[];
    final named = <String, Expression>{};
    for (final argument in list.arguments) {
      if (argument is NamedExpression) {
        named[argument.name.label.name] = argument.expression;
      } else {
        positional.add(argument);
      }
    }
    // Not `if (named.isEmpty) return positional`. That shortcut is wrong for a
    // call that omits every optional argument -- `weigh()` has no named
    // arguments to reorder and still needs all three defaults filled in, and
    // the shortcut emitted `weigh()` against a three-parameter function.
    // Whenever the callee is known, every parameter is accounted for.
    if (callee == null) {
      if (named.isNotEmpty) {
        throw Unsupported(
            'named argument with no resolved callee', site.toSource());
      }
      return [for (final a in positional) expression(a)];
    }

    final out = <IrExpr>[];
    var next = 0;
    for (final param in callee.formalParameters) {
      final name = param.name;
      if (param.isNamed) {
        final supplied = name == null ? null : named.remove(name);
        if (supplied != null) {
          out.add(expression(supplied));
          continue;
        }
      } else if (next < positional.length) {
        out.add(expression(positional[next++]));
        continue;
      }
      out.add(_omitted(param, site));
    }
    if (named.isNotEmpty) {
      throw Unsupported(
          'named argument `${named.keys.first}` not in the callee', site.toSource());
    }
    return out;
  }

  /// The value an omitted optional parameter stands for.
  IrExpr _omitted(FormalParameterElement param, AstNode site) {
    final code = param.defaultValueCode;
    if (code != null) {
      final literal = _literalFromSource(code);
      if (literal != null) return literal;
      throw Unsupported(
          'default `$code` is not a literal', site.toSource());
    }
    if (param.type.nullabilitySuffix.name == 'question') {
      return const IrLiteral('null', IrType('Null', nullable: true));
    }
    throw Unsupported(
        'omitted parameter `${param.name}` has no default', site.toSource());
  }

  /// A default value's source text, when it is a literal this IR can hold.
  ///
  /// Read from text because the callee may be declared in another file, whose
  /// AST this compiler is not holding. Only the shapes that cannot be
  /// misconstrued are accepted.
  IrExpr? _literalFromSource(String code) {
    if (code == 'true' || code == 'false') {
      return IrLiteral(code, const IrType('bool'));
    }
    if (code == 'null') {
      return const IrLiteral('null', IrType('Null', nullable: true));
    }
    if (RegExp(r'^-?\d+$').hasMatch(code)) {
      return IrLiteral(code, const IrType('int'));
    }
    if (RegExp(r'^-?\d+\.\d+$').hasMatch(code)) {
      return IrLiteral(code, const IrType('double'));
    }
    return null;
  }

  /// Private members are skipped when declarations are lowered, so a *call* to
  /// one has to stop the member that makes it.
  ///
  /// Without this the compiler emitted `Alignment::_stringify(...)` from
  /// `toString`, referring to a method it had never emitted -- output that
  /// looks complete and does not compile. Refusing the caller is the honest
  /// answer: the translation of `toString` genuinely is not available until
  /// private members are translated too.
  // `_refusePrivate` used to live here and refuse any reference to a private
  // member. It is gone for the reason given in `frontend_kernel.dart`: private
  // members are where a Flutter program keeps its implementation, so skipping
  // them translates the surface and none of the substance.
  void _refusePrivate(String name, AstNode node) {}

  IrExpr _invoke(MethodInvocation node) {
    final element = node.methodName.element;
    _refusePrivate(node.methodName.name, node);
    final args = _arguments(
      node.argumentList,
      element is ExecutableElement ? element : null,
      node,
    );
    if (element is MethodElement && element.isStatic) {
      final owner = element.enclosingElement;
      return IrStaticCall(
        owner is ClassElement ? (owner.name ?? '?') : '?',
        node.methodName.name,
        args,
      );
    }
    // A top-level function is not a method on `this`. Without this check
    // `clampDouble(a, b, c)` came out as `self.clamp_double(a, b, c)`, which is
    // both wrong and a place the two front ends disagreed -- the Kernel one
    // refuses a top-level call outright.
    if (element != null && element.enclosingElement is! ClassElement) {
      final enclosing = element.enclosingElement;
      if (enclosing is! InterfaceElement) {
        throw Unsupported(
            'top-level call `${node.methodName.name}`', node.toSource());
      }
    }
    final target = node.target;
    if (target is SuperExpression) {
      final base = _superclass;
      if (base == null) {
        throw Unsupported('super call with no superclass', node.toSource());
      }
      return IrSuperCall(base, node.methodName.name, args);
    }
    return IrCall(
      target == null ? null : expression(target),
      node.methodName.name,
      args,
    );
  }

  // -- Statements -------------------------------------------------------------

  IrStmt statement(Statement node) {
    if (node is ReturnStatement) {
      return IrReturn(node.expression == null ? null : expression(node.expression!));
    }
    if (node is Block) {
      return IrBlock([for (final s in node.statements) statement(s)]);
    }
    if (node is IfStatement) {
      return IrIf(
        expression(node.expression),
        statement(node.thenStatement),
        node.elseStatement == null ? null : statement(node.elseStatement!),
      );
    }
    if (node is VariableDeclarationStatement) {
      final declared = node.variables.variables;
      if (declared.length != 1) {
        throw Unsupported('multiple declarators', node.toSource());
      }
      final v = declared.single;
      return IrLocalDecl(
        v.name.lexeme,
        node.variables.type == null ? null : _type(node.variables.type!.type),
        v.initializer == null ? null : expression(v.initializer!),
      );
    }
    if (node is ExpressionStatement) {
      return IrExprStmt(expression(node.expression));
    }
    if (node is AssertStatement) {
      return _assert(node.condition, node.message);
    }
    throw Unsupported('statement ${node.runtimeType}', node.toSource());
  }

  /// Lowers `assert(condition, message)`.
  ///
  /// The message is diagnostics, not contract: what an assert *means* is its
  /// condition, and whether it fires does not depend on the message. So a
  /// message that is a plain string is carried across, and one that is an
  /// interpolation or a call is kept as source in a comment rather than
  /// translated. Translating it would drag string formatting -- and everything
  /// the message calls -- into the dependency set of every assert in the tree,
  /// to change what a debug build prints when something has already gone wrong.
  IrAssert _assert(Expression condition, Expression? message) {
    if (message is SimpleStringLiteral) {
      return IrAssert(expression(condition), literalMessage: message.value);
    }
    return IrAssert(
      expression(condition),
      message: message?.toSource(),
    );
  }

  IrStmt body(FunctionBody node) {
    if (node is ExpressionFunctionBody) {
      return IrReturn(expression(node.expression));
    }
    if (node is BlockFunctionBody) {
      return statement(node.block);
    }
    throw Unsupported('body ${node.runtimeType}', node.toSource());
  }

  // -- Declarations -----------------------------------------------------------

  /// Lowers every class in a compilation unit.
  ///
  /// The hierarchy is why this exists. `impl AlignmentGeometry for Alignment`
  /// needs to know that `AlignmentGeometry` is abstract and what it requires,
  /// and neither fact can be seen from inside `Alignment`.
  (IrLibrary, List<String>) lowerLibrary(CompilationUnit unit) {
    final classes = <IrClass>[];
    final refused = <String>[];
    for (final declaration in unit.declarations) {
      if (declaration is! ClassDeclaration) continue;
      final (cls, problems) = lowerClass(declaration);
      classes.add(cls);
      refused.addAll(problems.map((p) => '${cls.name}: $p'));
    }
    return (IrLibrary(classes), refused);
  }

  /// Lowers one class, collecting what it could not translate rather than
  /// stopping at the first refusal: a report of eleven unsupported members is
  /// worth more than a report of the first one.
  (IrClass, List<String>) lowerClass(ClassDeclaration node) {
    final element = node.declaredFragment?.element;
    final cls = IrClass(
      node.name.lexeme,
      superclass: node.extendsClause?.superclass.name.lexeme,
      isAbstract: node.abstractKeyword != null,
      doc: _doc(node),
    );
    _superclass = cls.superclass;
    final refused = <String>[];

    for (final member in node.members) {
      try {
        if (member is FieldDeclaration) {
          _lowerField(cls, member);
        } else if (member is ConstructorDeclaration) {
          _lowerConstructor(cls, member);
        } else if (member is MethodDeclaration) {
          _lowerMethod(cls, member);
        }
      } on Unsupported catch (error) {
        refused.add('$error');
      }
    }
    if (element == null) refused.add('class element did not resolve');
    return (cls, refused);
  }

  void _lowerField(IrClass cls, FieldDeclaration member) {
    for (final v in member.fields.variables) {
      final element = v.declaredFragment?.element;
      final type = _type(member.fields.type?.type ?? element?.type);
      if (member.isStatic) {
        if (element is! FieldElement || !element.isConst) {
          throw Unsupported('non-const static field', v.toSource());
        }
        // The initialiser is lowered from source rather than from the evaluated
        // constant. Both are available; the source keeps `Alignment(-1, -1)`
        // recognisable, and the evaluated value is what the driver prints to
        // check the two agree.
        final init = v.initializer;
        if (init == null) throw Unsupported('const without initialiser', v.toSource());
        cls.constants.add(IrConstDecl(
          v.name.lexeme,
          type,
          expression(init),
          doc: _doc(member),
        ));
      } else {
        cls.fields.add(IrFieldDecl(
          v.name.lexeme,
          type,
          isFinal: member.fields.isFinal,
          doc: _doc(member),
        ));
      }
    }
  }

  void _lowerConstructor(IrClass cls, ConstructorDeclaration member) {
    final name = member.name?.lexeme;
    if (member.factoryKeyword != null) {
      // A factory may return a cached instance or a subclass, so it is not an
      // associated function that builds Self -- it needs the hierarchy.
      throw Unsupported('factory constructor', member.toSource());
    }
    final params = <IrParam>[];
    final inits = <String, IrExpr>{};
    for (final p in member.parameters.parameters) {
      final inner = p is DefaultFormalParameter ? p.parameter : p;
      final name = inner.name?.lexeme;
      if (name == null) throw Unsupported('unnamed parameter', p.toSource());
      final element = inner.declaredFragment?.element;
      params.add(IrParam(name, _type(element?.type), named: p.isNamed));
      if (inner is FieldFormalParameter) {
        inits[name] = IrLocal(name);
      }
    }
    final asserts = <IrAssert>[];
    for (final init in member.initializers) {
      if (init is ConstructorFieldInitializer) {
        inits[init.fieldName.name] = expression(init.expression);
      } else if (init is AssertInitializer) {
        asserts.add(_assert(init.condition, init.message));
      } else if (init is RedirectingConstructorInvocation) {
        // `EdgeInsets.all(v) : this.fromLTRB(v, v, v, v)`. Nothing here
        // prevents lowering it to a call to the other constructor -- but the
        // other constructor's own field initialisers would then have to be
        // reachable, and they are not yet, so this stops rather than emitting
        // a constructor that sets no fields.
        throw Unsupported('redirecting constructor', init.toSource());
      } else if (init is SuperConstructorInvocation) {
        throw Unsupported('super constructor call', init.toSource());
      } else {
        throw Unsupported('initialiser ${init.runtimeType}', init.toSource());
      }
    }
    cls.constructors.add(IrConstructor(
      params,
      inits,
      isConst: member.constKeyword != null,
      name: name,
      asserts: asserts,
      doc: _doc(member),
    ));
  }

  void _lowerMethod(IrClass cls, MethodDeclaration member) {
    // A private member used to be skipped here, and the ordering of that skip
    // against the abstract and setter checks mattered enough to be worth a
    // round of its own. Both are gone: private members are translated now, so
    // there is nothing for the ordering to decide.
    if (member.isSetter) throw Unsupported('setter', member.toSource());

    final params = <IrParam>[];
    for (final p in member.parameters?.parameters ?? const <FormalParameter>[]) {
      final inner = p is DefaultFormalParameter ? p.parameter : p;
      final name = inner.name?.lexeme;
      if (name == null) throw Unsupported('unnamed parameter', p.toSource());
      params.add(IrParam(
        name,
        _type(inner.declaredFragment?.element.type),
        named: p.isNamed,
      ));
    }

    final isOperator = member.operatorKeyword != null;
    // An abstract member has no body to lower, so it goes on a separate list:
    // it is what the trait *requires*, not what the trait *provides*.
    final target = member.isAbstract ? cls.abstractMethods : cls.methods;
    target.add(IrMethod(
      member.name.lexeme,
      params,
      _type(member.returnType?.type),
      member.isAbstract ? const IrBlock([]) : body(member.body),
      isStatic: member.isStatic,
      isGetter: member.isGetter,
      operator: isOperator
          ? (params.isEmpty && member.name.lexeme == '-'
              ? 'unary-'
              : member.name.lexeme)
          : null,
      doc: _doc(member),
    ));
  }
}
