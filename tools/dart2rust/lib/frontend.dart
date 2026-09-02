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
import 'package:analyzer/dart/ast/visitor.dart';
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
    if (type is FunctionType) {
      return IrType.function(
        [for (final p in type.formalParameters) _type(p.type)],
        _type(type.returnType),
        nullable: nullable,
      );
    }
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
      // `x == null` and `x != null` are their own thing in Rust, and Kernel
      // already treats them so. Lowering them here as an ordinary comparison
      // would leave the two front ends emitting different Rust for identical
      // Dart -- in 2524 places.
      final operator = node.operator.lexeme;
      if (operator == '==' || operator == '!=') {
        final left = node.leftOperand;
        final right = node.rightOperand;
        if (right is NullLiteral || left is NullLiteral) {
          final value = right is NullLiteral ? left : right;
          final test = IrIsNull(expression(value));
          return operator == '==' ? test : IrUnary('!', test);
        }
      }
      if (operator == '??') {
        final right = node.rightOperand;
        return IrIfNull(
          expression(node.leftOperand),
          expression(right),
          nullableResult: node.staticType?.nullabilitySuffix.name == 'question',
          eager: right is Literal,
        );
      }
      return IrBinary(
        operator,
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
    if (node is PostfixExpression) {
      // `!` only. `x++` and `x--` are postfix too and are assignments in
      // disguise; they belong with the mutability work, not here.
      if (node.operator.lexeme != '!') {
        throw Unsupported('postfix `${node.operator.lexeme}`', node.toSource());
      }
      return IrNullCheck(expression(node.operand));
    }
    if (node is FunctionExpressionInvocation) {
      return IrCallValue(
        expression(node.function),
        _arguments(node.argumentList, node.element, node),
      );
    }
    if (node is ThrowExpression) {
      // A `throw` used for its value has none in Rust; the statement form is
      // the one that translates.
      throw Unsupported('throw used as an expression', node.toSource());
    }
    if (node is CascadeExpression) return _cascade(node);
    if (node is FunctionExpression) return _closure(node);
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
    // A top-level `const`/`final` is a module constant in Rust too. Analyzer
    // models one as a *synthetic* getter -- the same distinction that separates
    // a field from a real getter -- so a non-synthetic top-level getter is a
    // computed `get foo => ...` and still stops.
    if (element is GetterElement &&
        element.isSynthetic &&
        (enclosingOf(element) == null)) {
      return IrTopLevel(node.name);
    }
    // The refusal names *where* the thing is declared, not what analyzer's
    // class for it happens to be called. "identifier of GetterElementImpl"
    // says nothing about the work; "top-level getter" says it is a module
    // constant and "getter on an extension" says it is something else again.
    final enclosing = element.enclosingElement;
    final where = switch (enclosing) {
      null => 'top-level',
      LibraryElement() => 'top-level',
      LibraryFragment() => 'top-level',
      ExtensionElement() => 'on an extension',
      MixinElement() => 'on a mixin',
      EnumElement() => 'on an enum',
      InterfaceElement() => 'on a class',
      _ => 'in ${enclosing.runtimeType}',
    };
    final kind = switch (element) {
      GetterElement() => 'getter',
      SetterElement() => 'setter',
      MethodElement() => 'method',
      FieldElement() => 'field',
      _ => element.runtimeType.toString(),
    };
    throw Unsupported('$where $kind `${node.name}`', node.toSource());
  }

  /// The class, mixin or enum a member belongs to, or null when it is
  /// top-level. Analyzer reaches a library through a fragment, so "no enclosing
  /// interface" is the question, not "enclosing is a library".
  InterfaceElement? enclosingOf(Element element) {
    final enclosing = element.enclosingElement;
    return enclosing is InterfaceElement ? enclosing : null;
  }

  IrExpr _prefixed(PrefixedIdentifier node) {
    final target = node.prefix.element;
    // `Alignment.topLeft` -- the prefix resolves to the class itself. An enum
    // is an `EnumElement`, not a `ClassElement`, which is why every reference
    // to an enum value used to be refused as "identifier of EnumElementImpl".
    if (target is EnumElement) {
      return IrStatic(
        target.name ?? '?',
        node.identifier.name,
        isEnumValue: true,
      );
    }
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
    // `a?.b`. Kernel has already lowered this to a Let and the Kernel front end
    // restores it; here it is still written as itself, so it is simply read.
    if (node.isNullAware) {
      return IrNullAware(
        expression(target),
        _memberOn(const IrBound(), node.propertyName),
      );
    }
    final accessor = node.propertyName.element;
    if (accessor is PropertyAccessorElement && !accessor.isSynthetic) {
      return IrCall(expression(target), node.propertyName.name, const []);
    }
    return IrField(expression(target), node.propertyName.name);
  }

  /// A getter or field read on an already-lowered receiver.
  IrExpr _memberOn(IrExpr receiver, SimpleIdentifier name) {
    final element = name.element;
    if (element is PropertyAccessorElement && !element.isSynthetic) {
      return IrCall(receiver, name.name, const []);
    }
    return IrField(receiver, name.name);
  }

  /// `a..b = 1..c()` -- bind the receiver, do the steps, produce the binding.
  ///
  /// Kernel has already lowered this to a block expression and the Kernel front
  /// end restores it; here it is still written as itself. Both have to arrive
  /// at the same IR, which is why this is not left to be refused.
  IrExpr _cascade(CascadeExpression node) {
    final previous = _cascadeTarget;
    _cascadeTarget = node.target;
    try {
      final steps = <IrStmt>[
        IrLocalDecl(
          _cascadeName,
          _type(node.target.staticType),
          expression(node.target),
        ),
        for (final section in node.cascadeSections) _cascadeStep(section),
      ];
      return IrBlockValue(steps, const IrLocal(_cascadeName));
    } finally {
      _cascadeTarget = previous;
    }
  }

  IrStmt _cascadeStep(Expression section) {
    if (section is AssignmentExpression) {
      final target = section.leftHandSide;
      if (target is PropertyAccess && section.operator.lexeme == '=') {
        final written = section.writeElement;
        final value = expression(section.rightHandSide);
        if (written != null && !written.isSynthetic) {
          return IrSetter(
            const IrLocal(_cascadeName),
            target.propertyName.name,
            value,
          );
        }
        return IrAssignField(
          target.propertyName.name,
          value,
          target: const IrLocal(_cascadeName),
        );
      }
      throw Unsupported(
        'cascade step ${target.runtimeType}',
        section.toSource(),
      );
    }
    if (section is MethodInvocation) {
      return IrExprStmt(
        IrCall(
          const IrLocal(_cascadeName),
          section.methodName.name,
          _arguments(
            section.argumentList,
            section.methodName.element is ExecutableElement
                ? section.methodName.element as ExecutableElement
                : null,
            section,
          ),
        ),
      );
    }
    throw Unsupported(
      'cascade step ${section.runtimeType}',
      section.toSource(),
    );
  }

  /// The receiver the enclosing cascade bound; its sections leave their target
  /// implicit, so it has to be carried.
  Expression? _cascadeTarget;
  static const _cascadeName = 'cascaded';

  /// A closure literal, when it captures nothing this compiler cannot give it.
  ///
  /// A closure reaching `this` is refused for the reason given in
  /// `frontend_kernel.dart`: it outlives the call that made it and `this` is a
  /// borrow.
  IrExpr _closure(FunctionExpression node) {
    // Not a text search for `this`. Dart lets an instance member be named
    // without it -- `factor` inside a method *is* `this.factor` -- so
    // `toSource().contains('this')` let exactly the closures this refuses
    // through, and the Kernel front end (where `this` is explicit) refused
    // them, so the two disagreed. Resolution is the only way to ask.
    final finder = _InstanceUse();
    node.accept(finder);
    if (finder.found) {
      throw Unsupported('closure capturing `this`', node.toSource());
    }
    final params = <IrParam>[];
    for (final p in node.parameters?.parameters ?? const <FormalParameter>[]) {
      final inner = p is DefaultFormalParameter ? p.parameter : p;
      final name = inner.name?.lexeme;
      if (name == null) throw Unsupported('unnamed parameter', p.toSource());
      params.add(IrParam(name, _type(inner.declaredFragment?.element.type)));
    }
    return IrClosure(
      params,
      body(node.body),
      _type(node.declaredFragment?.element.returnType),
    );
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
          'named argument with no resolved callee',
          site.toSource(),
        );
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
        'named argument `${named.keys.first}` not in the callee',
        site.toSource(),
      );
    }
    return out;
  }

  /// The value an omitted optional parameter stands for.
  IrExpr _omitted(FormalParameterElement param, AstNode site) {
    final code = param.defaultValueCode;
    if (code != null) {
      final literal = _literalFromSource(code);
      if (literal != null) return literal;
      throw Unsupported('default `$code` is not a literal', site.toSource());
    }
    if (param.type.nullabilitySuffix.name == 'question') {
      return const IrLiteral('null', IrType('Null', nullable: true));
    }
    throw Unsupported(
      'omitted parameter `${param.name}` has no default',
      site.toSource(),
    );
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
          'top-level call `${node.methodName.name}`',
          node.toSource(),
        );
      }
    }
    final target = node.target;
    if (node.isNullAware && target != null) {
      return IrNullAware(
        expression(target),
        IrCall(const IrBound(), node.methodName.name, args),
      );
    }
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
      return IrReturn(
        node.expression == null ? null : expression(node.expression!),
      );
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
      final value = node.expression;
      if (value is AssignmentExpression) return _assignment(value);
      if (value is ThrowExpression)
        return IrThrow(expression(value.expression));
      return IrExprStmt(expression(value));
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
    return IrAssert(expression(condition), message: message?.toSource());
  }

  /// `x = value`, and only when `x` is a local.
  ///
  /// A compound assignment (`x += 1`) is expanded here, because analyzer keeps
  /// it as one node while Kernel has already rewritten it -- the two front ends
  /// have to arrive at the same IR.
  IrStmt _assignment(AssignmentExpression node) {
    final target = node.leftHandSide;
    final (name, onThis) = _assignTarget(node);
    final operator = node.operator.lexeme;
    final value = expression(node.rightHandSide);

    IrExpr combined(IrExpr current) {
      if (operator == '=') return value;
      if (operator.endsWith('=') && operator.length > 1) {
        return IrBinary(
          operator.substring(0, operator.length - 1),
          current,
          value,
        );
      }
      throw Unsupported('assignment operator `$operator`', node.toSource());
    }

    if (!onThis) return IrAssign(name, combined(IrLocal(name)));
    if (_isSetterTarget(node)) {
      // A setter is a call. Its "current value" for a compound assignment is
      // the matching getter, not the field -- there may be no field at all.
      final receiver =
          target is PropertyAccess && target.target is! ThisExpression
          ? expression(target.target!)
          : null;
      return IrSetter(
        receiver,
        name,
        combined(IrCall(receiver, name, const [])),
      );
    }
    return IrAssignField(name, combined(IrField(null, name)));
  }

  /// The name being assigned, and whether it is a field of `this`.
  ///
  /// The element is taken from `writeElement` on the assignment, not from the
  /// identifier: analyzer leaves the identifier's own element **null** on the
  /// left of an assignment, and reading it there gave "assignment to `Null`"
  /// for every field write in the corpus.
  ///
  /// In valid code `writeElement` is a local, a parameter, or a setter. The
  /// setter is the field's implicit one when it is really a field, and a real
  /// `set x(v)` otherwise -- `isSynthetic` again, as on the getter side.
  (String, bool) _assignTarget(AssignmentExpression node) {
    final target = node.leftHandSide;
    final written = node.writeElement;
    final name = switch (target) {
      SimpleIdentifier() => target.name,
      PropertyAccess() when target.target is ThisExpression =>
        target.propertyName.name,
      _ => throw Unsupported(
        'assignment to ${target.runtimeType}',
        node.toSource(),
      ),
    };
    if (written is LocalVariableElement || written is FormalParameterElement) {
      return (name, false);
    }
    if (written != null && written.isSynthetic) return (name, true);
    return (name, true);
  }

  /// Whether the assignment target is a real `set x(v)` rather than a field.
  bool _isSetterTarget(AssignmentExpression node) {
    final written = node.writeElement;
    if (written is LocalVariableElement || written is FormalParameterElement) {
      return false;
    }
    return written != null && !written.isSynthetic;
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
    final constants = <IrConstDecl>[];
    final refused = <String>[];
    for (final declaration in unit.declarations) {
      if (declaration is TopLevelVariableDeclaration) {
        for (final v in declaration.variables.variables) {
          if (!declaration.variables.isConst &&
              !declaration.variables.isFinal) {
            continue;
          }
          final init = v.initializer;
          if (init == null) continue;
          try {
            constants.add(
              IrConstDecl(
                v.name.lexeme,
                _type(declaration.variables.type?.type),
                expression(init),
              ),
            );
          } on Unsupported catch (error) {
            refused.add('top-level ${v.name.lexeme}: $error');
          }
        }
        continue;
      }
      if (declaration is EnumDeclaration) {
        // An enum is not a ClassDeclaration in analyzer's AST, so it was
        // skipped entirely here while every *reference* to one of its values
        // was refused separately -- 847 refusals whose declarations the front
        // end had never looked at.
        final (cls, problems) = lowerEnum(declaration);
        classes.add(cls);
        refused.addAll(problems.map((p) => '${cls.name}: $p'));
        continue;
      }
      if (declaration is! ClassDeclaration) continue;
      final (cls, problems) = lowerClass(declaration);
      classes.add(cls);
      refused.addAll(problems.map((p) => '${cls.name}: $p'));
    }
    return (IrLibrary(classes, constants: constants), refused);
  }

  /// Lowers a plain enum. An enhanced one -- with fields, a constructor or
  /// methods -- is refused: it is a Rust enum *plus* an impl, and emitting it
  /// as a plain one would drop its members without saying so. Across
  /// `package:flutter` 232 of 249 enums are plain.
  (IrClass, List<String>) lowerEnum(EnumDeclaration node) {
    final refused = <String>[];
    final declared = node.members.where((m) {
      if (m is MethodDeclaration) return true;
      if (m is FieldDeclaration) return !m.isStatic;
      if (m is ConstructorDeclaration) return true;
      return false;
    }).toList();
    if (declared.isNotEmpty) {
      refused.add(
        'unsupported enhanced enum: ${declared.length} declared '
        'member(s)',
      );
    }
    return (
      IrClass(
        node.name.lexeme,
        isEnum: true,
        values: declared.isEmpty
            ? [for (final c in node.constants) c.name.lexeme]
            : const [],
        doc: _doc(node),
      ),
      refused,
    );
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
        if (init == null)
          throw Unsupported('const without initialiser', v.toSource());
        cls.constants.add(
          IrConstDecl(v.name.lexeme, type, expression(init), doc: _doc(member)),
        );
      } else {
        final initial = v.initializer;
        cls.fields.add(
          IrFieldDecl(
            v.name.lexeme,
            type,
            isFinal: member.fields.isFinal,
            initial: initial == null ? null : expression(initial),
            doc: _doc(member),
          ),
        );
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
    String? superBase;
    var superArgs = const <IrExpr>[];
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
        final arguments = init.argumentList.arguments;
        if (arguments.isNotEmpty) {
          superBase = cls.superclass;
          if (superBase == null) {
            throw Unsupported(
              'super constructor call with no base',
              init.toSource(),
            );
          }
          superArgs = _arguments(init.argumentList, init.element, init);
        }
      } else {
        throw Unsupported('initialiser ${init.runtimeType}', init.toSource());
      }
    }
    // See `frontend_kernel.dart`: a constructor body is not lowered, and
    // dropping it silently emits a constructor that ignores its arguments.
    final constructorBody = member.body;
    if (constructorBody is BlockFunctionBody &&
        constructorBody.block.statements.isNotEmpty) {
      throw Unsupported('constructor with a body', member.toSource());
    }
    cls.constructors.add(
      IrConstructor(
        params,
        inits,
        isConst: member.constKeyword != null,
        name: name,
        asserts: asserts,
        superBase: superBase,
        superArgs: superArgs,
        doc: _doc(member),
      ),
    );
  }

  void _lowerMethod(IrClass cls, MethodDeclaration member) {
    // A private member used to be skipped here, and the ordering of that skip
    // against the abstract and setter checks mattered enough to be worth a
    // round of its own. Both are gone: private members are translated now, so
    // there is nothing for the ordering to decide.

    final params = <IrParam>[];
    for (final p
        in member.parameters?.parameters ?? const <FormalParameter>[]) {
      final inner = p is DefaultFormalParameter ? p.parameter : p;
      final name = inner.name?.lexeme;
      if (name == null) throw Unsupported('unnamed parameter', p.toSource());
      params.add(
        IrParam(
          name,
          _type(inner.declaredFragment?.element.type),
          named: p.isNamed,
        ),
      );
    }

    final isOperator = member.operatorKeyword != null;
    final finder = _ThrownTypes();
    member.body.accept(finder);
    if (finder.types.length > 1) {
      throw Unsupported(
        'method throwing ${finder.types.length} error types',
        member.name.lexeme,
      );
    }
    // An abstract member has no body to lower, so it goes on a separate list:
    // it is what the trait *requires*, not what the trait *provides*.
    final target = member.isAbstract ? cls.abstractMethods : cls.methods;
    target.add(
      IrMethod(
        member.name.lexeme,
        params,
        _type(member.returnType?.type),
        member.isAbstract ? const IrBlock([]) : body(member.body),
        isStatic: member.isStatic,
        isGetter: member.isGetter,
        isSetter: member.isSetter,
        operator: isOperator
            ? (params.isEmpty && member.name.lexeme == '-'
                  ? 'unary-'
                  : member.name.lexeme)
            : null,
        throws: finder.types.isEmpty ? null : finder.types.single,
        doc: _doc(member),
      ),
    );
  }
}

/// Whether an expression reaches an instance member of the enclosing class,
/// with or without writing `this`.
class _InstanceUse extends RecursiveAstVisitor<void> {
  bool found = false;

  void _check(Element? element) {
    if (element == null) return;
    if (element.enclosingElement is! InterfaceElement) return;
    final isStatic = switch (element) {
      FieldElement() => element.isStatic,
      PropertyAccessorElement() => element.isStatic,
      MethodElement() => element.isStatic,
      _ => true,
    };
    if (!isStatic) found = true;
  }

  @override
  void visitThisExpression(ThisExpression node) {
    found = true;
    super.visitThisExpression(node);
  }

  @override
  void visitSimpleIdentifier(SimpleIdentifier node) {
    _check(node.element);
    super.visitSimpleIdentifier(node);
  }
}

/// The error types a method body throws.
class _ThrownTypes extends RecursiveAstVisitor<void> {
  final types = <String>{};

  @override
  void visitThrowExpression(ThrowExpression node) {
    final type = node.expression.staticType;
    types.add(type?.element?.name ?? 'Object');
    super.visitThrowExpression(node);
  }
}
