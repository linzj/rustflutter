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
    if (type is RecordType) {
      if (type.namedFields.isNotEmpty) {
        throw Unsupported(
          'a record type with named fields',
          type.getDisplayString(),
        );
      }
      return IrType(
        'Record',
        nullable: nullable,
        arguments: [for (final f in type.positionalFields) _type(f.type)],
      );
    }
    final element = type.element;
    final name = element?.name ?? type.getDisplayString();
    return IrType(
      name ?? 'dynamic',
      nullable: nullable,
      arguments: type is InterfaceType
          ? [for (final a in type.typeArguments) _type(a)]
          : const [],
    );
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
        type: _type(node.staticType),
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
    if (node is AwaitExpression) {
      return IrAwait(expression(node.expression));
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
    if (node is RecordLiteral) {
      if (node.fields.any((f) => f is NamedExpression)) {
        throw Unsupported('a record with named fields', node.toSource());
      }
      return IrRecord([for (final f in node.fields) expression(f)]);
    }
    if (node is SetOrMapLiteral) {
      if (!node.isMap) throw Unsupported('a set literal', node.toSource());
      final args = node.typeArguments?.arguments;
      return IrMapLiteral(
        [
          for (final element in node.elements)
            if (element is MapLiteralEntry)
              (expression(element.key), expression(element.value))
            else
              throw Unsupported(
                'map element ${element.runtimeType}',
                node.toSource(),
              ),
        ],
        _type(args != null && args.length == 2 ? args[0].type : null),
        _type(args != null && args.length == 2 ? args[1].type : null),
      );
    }
    if (node is ListLiteral) {
      return IrListLiteral([
        for (final element in node.elements)
          if (element is Expression)
            expression(element)
          else
            throw Unsupported(
              'list element ${element.runtimeType}',
              node.toSource(),
            ),
      ], _type(node.typeArguments?.arguments.singleOrNull?.type));
    }
    if (node is IndexExpression) {
      final owner = node.target?.staticType?.element?.name;
      if (owner == 'Map') {
        return IrCall(expression(node.target!), 'get', [
          expression(node.index),
        ]);
      }
      return IrIndex(expression(node.target!), expression(node.index));
    }
    if (node is StringInterpolation) {
      return IrInterpolation([
        for (final element in node.elements)
          if (element is InterpolationString)
            IrLiteral(element.value, const IrType('String'))
          else
            expression((element as InterpolationExpression).expression),
      ]);
    }
    if (node is AssignmentExpression) return _assignmentValue(node);
    if (node is ThrowExpression) {
      // `a ?? throw StateError(..)`. Rust has no throw and does not need one:
      // `return Err(e)` has type `!`, which fits wherever a value was wanted.
      return IrThrowValue(expression(node.expression));
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
    // A field the enclosing closure copied in is a local now, not a field of
    // a `this` the closure does not hold. See `IrClosure.captures`.
    if (_captured.contains(node.name)) return IrLocal(node.name);
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
    // A value an enum variant carries, read from one of the enum's own
    // methods. Lowered as a field read like any other; the backend knows that
    // for an enum it is a getter over a `match`, and knowing it there means
    // knowing it once rather than in both front ends.
    if (element.enclosingElement is EnumElement &&
        element is GetterElement &&
        element.isSynthetic) {
      return IrField(null, node.name, onEnum: true);
    }
    // A method used as a value: `applyTwice(scaled, x)`. In Rust that is a
    // closure that calls it, which makes it the same question as any other
    // closure and gets the same answer -- in a borrowed position it may borrow
    // the receiver, anywhere else it would have to own it. The Kernel front
    // end reads the same thing off `InstanceTearOff`.
    if (element is MethodElement &&
        !element.isStatic &&
        element.enclosingElement is InterfaceElement) {
      if (!_borrowedArgument) {
        throw Unsupported('a method used as a value', node.toSource());
      }
      if (element.formalParameters.any((p) => p.isNamed) ||
          element.typeParameters.isNotEmpty) {
        throw Unsupported(
          'a method with named or generic parameters used as a value',
          node.toSource(),
        );
      }
      final params = [
        for (var i = 0; i < element.formalParameters.length; i++)
          IrParam(
            element.formalParameters[i].name ?? 'a$i',
            _type(element.formalParameters[i].type),
          ),
      ];
      return IrClosure(
        params,
        IrReturn(
          IrCall(null, node.name, [for (final p in params) IrLocal(p.name)]),
        ),
        _type(element.returnType),
      );
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

  /// The Rust tuple index for a positional record field, or null.
  ///
  /// Dart writes them `$1`, `$2`, counting from one; Rust counts from zero,
  /// and the subtraction is done here so the backend has one story.
  int? _recordField(String name) {
    if (name.length < 2 || name.codeUnitAt(0) != 0x24) return null;
    final position = int.tryParse(name.substring(1));
    return position == null || position < 1 ? null : position - 1;
  }

  IrExpr _prefixed(PrefixedIdentifier node) {
    if (_recordField(node.identifier.name) != null) {
      // See the PropertyAccess case: `s.$2` on a plain local parses as a
      // prefixed identifier rather than a property access, and reaching only
      // one of the two left the two front ends saying different things.
      return IrRecordField(
        expression(node.prefix),
        _recordField(node.identifier.name)!,
      );
    }
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
      // `Label.twice` is a *method* used as a value, not a static field read.
      // Read as a field it came out as `Label::TWICE`, naming a constant that
      // was never declared.
      if (node.identifier.element is MethodElement) {
        return IrFunctionRef(target.name ?? '?', node.identifier.name);
      }
      return IrStatic(target.name ?? '?', node.identifier.name);
    }
    final listOwner = node.identifier.element == null
        ? null
        : enclosingOf(node.identifier.element!)?.name;
    if (listOwner == 'List' || listOwner == 'Iterable') {
      // A getter in Dart is a method in Rust: `xs.length` is `xs.len()`.
      final rust = listMethodNames[node.identifier.name];
      if (rust == null) {
        throw Unsupported('`List.${node.identifier.name}`', node.toSource());
      }
      return IrCall(expression(node.prefix), rust, const []);
    }
    if (listOwner == 'Map') {
      final name = node.identifier.name;
      if (orderedMapMembers.contains(name)) {
        throw Unsupported(
          '`Map.$name`, which depends on insertion order',
          node.toSource(),
        );
      }
      final rust = mapMethodNames[name];
      if (rust == null) throw Unsupported('`Map.$name`', node.toSource());
      return IrCall(expression(node.prefix), rust, const []);
    }
    final accessor = node.identifier.element;
    if (accessor is PropertyAccessorElement && !accessor.isSynthetic) {
      return IrCall(expression(node.prefix), node.identifier.name, const []);
    }
    return IrField(
      expression(node.prefix),
      node.identifier.name,
      onEnum: accessor?.enclosingElement is EnumElement,
    );
  }

  IrExpr _property(PropertyAccess node) {
    if (_recordField(node.propertyName.name) != null) {
      // `r.$1` in Dart is `r.0` in Rust: Dart counts positional record fields
      // from one and Rust counts tuple fields from zero.
      //
      // Written here rather than in `expression`, where it was unreachable:
      // the dispatch sends every PropertyAccess to this method first, so a
      // check further down never ran.
      return IrRecordField(
        expression(node.realTarget),
        _recordField(node.propertyName.name)!,
      );
    }
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
    return IrField(
      expression(target),
      node.propertyName.name,
      onEnum: accessor?.enclosingElement is EnumElement,
    );
  }

  /// A getter or field read on an already-lowered receiver.
  IrExpr _memberOn(IrExpr receiver, SimpleIdentifier name) {
    final element = name.element;
    if (element is PropertyAccessorElement && !element.isSynthetic) {
      return IrCall(receiver, name.name, const []);
    }
    return IrField(
      receiver,
      name.name,
      onEnum: element?.enclosingElement is EnumElement,
    );
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
    // A closure that only reads `final` fields copies them in rather than
    // holding `this`. See `IrClosure.captures`; the Kernel front end draws the
    // same line from the resolved fields, and the closures fixture is what
    // holds the two to it.
    final finals = _finalFieldsRead(node);
    final copies = finals != null && !_borrowedArgument;
    if (finder.found &&
        !copies &&
        !(_borrowedArgument && _onlyReadsThis(node))) {
      throw Unsupported('closure capturing `this`', node.toSource());
    }
    final params = <IrParam>[];
    for (final p in node.parameters?.parameters ?? const <FormalParameter>[]) {
      final inner = p is DefaultFormalParameter ? p.parameter : p;
      final name = inner.name?.lexeme;
      if (name == null) throw Unsupported('unnamed parameter', p.toSource());
      params.add(IrParam(name, _type(inner.declaredFragment?.element.type)));
    }
    final was = _captured;
    if (copies) _captured = {for (final f in finals) f.name!};
    try {
      return IrClosure(
        params,
        body(node.body),
        _type(node.declaredFragment?.element.returnType),
        captures: copies
            ? [for (final f in finals) IrParam(f.name!, _type(f.type))]
            : const [],
      );
    } finally {
      _captured = was;
    }
  }

  /// The fields a closure reads on `this`, when every one is `final` and
  /// nothing else about `this` is touched. Null when it is not that shape.
  List<FieldElement>? _finalFieldsRead(FunctionExpression node) {
    final demand = _InstanceDemand();
    node.accept(demand);
    if (demand.demanding) return null;
    final finder = _FinalFieldReads();
    node.accept(finder);
    if (!finder.allFinal || finder.fields.isEmpty) return null;
    return finder.fields.values.toList();
  }

  /// Whether a closure only *reads* fields of `this`.
  ///
  /// Reading takes a shared borrow, and the method the closure sits in already
  /// holds one. Writing a field wants `&mut self` while `self` is borrowed for
  /// the call, and calling a method hands out the whole object; both stay
  /// refused. The Kernel front end's `_onlyReadsThis` draws the same line, and
  /// the fixtures are what keep them there.
  bool _onlyReadsThis(FunctionExpression node) {
    final use = _InstanceDemand();
    node.accept(use);
    return !use.demanding;
  }

  IrExpr _construct(InstanceCreationExpression node) {
    final type = _type(node.staticType);
    final name = node.constructorName.name?.name;
    return IrNew(
      type,
      _arguments(
        node.argumentList,
        node.constructorName.element,
        node,
        borrows: false,
      ),
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
    AstNode site, {
    bool borrows = true,
  }) {
    final was = _borrowedArgument;
    _borrowedArgument = borrows;
    try {
      return _argumentList(list, callee, site);
    } finally {
      _borrowedArgument = was;
    }
  }

  /// Whether a closure written here would land in a borrowed position.
  ///
  /// The backend emits a function-typed *parameter* as `impl Fn(..)`, so a
  /// closure passed to a call borrows and lives exactly as long as the call.
  /// A constructor argument is stored in the object being built and outlives
  /// it, so it stays refused. Kept in step with the Kernel front end, which
  /// draws the same line in the same place.
  bool _borrowedArgument = false;

  /// The fields the closure being lowered copies in. A read of one is a read
  /// of the local, not of `this`.
  Set<String> _captured = const {};

  /// `name/index` for every parameter its callee does more with than call.
  Set<String> _keptParameters = const {};

  /// Whether the callee keeps the argument at [index] rather than calling it.
  bool _keeps(ExecutableElement? callee, int index) =>
      callee == null || _keptParameters.contains('${callee.name}/$index');

  /// Lowers one argument with `_borrowedArgument` set for *that* parameter.
  IrExpr _argumentAt(Expression value, ExecutableElement? callee, int index) {
    final was = _borrowedArgument;
    final kept = _keeps(callee, index);
    if (kept) _borrowedArgument = false;
    try {
      final lowered = expression(value);
      // Owned where it is kept, so the argument is boxed to match.
      if (kept && lowered is IrClosure) {
        return IrClosure(
          lowered.params,
          lowered.body,
          lowered.returns,
          captures: lowered.captures,
          boxed: true,
        );
      }
      return lowered;
    } finally {
      _borrowedArgument = was;
    }
  }

  List<IrExpr> _argumentList(
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
    var at = 0;
    for (final param in callee.formalParameters) {
      final name = param.name;
      final index = at++;
      if (param.isNamed) {
        final supplied = name == null ? null : named.remove(name);
        if (supplied != null) {
          out.add(_argumentAt(supplied, callee, index));
          continue;
        }
      } else if (next < positional.length) {
        out.add(_argumentAt(positional[next++], callee, index));
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
    // A `List` or `Iterable` method: the name changes and nothing else does.
    // Shared with the Kernel front end through `listMethodNames`, so the two
    // cannot drift apart on a name.
    final owner = element == null ? null : enclosingOf(element)?.name;
    if (owner == 'List' || owner == 'Iterable') {
      final args = _arguments(node.argumentList, null, node);
      final step = iterStepNames[node.methodName.name];
      if (step != null && args.length == 1) {
        final source = expression(node.target!);
        return source is IrIterChain
            ? IrIterChain(source.source, [...source.steps, (step, args.single)])
            : IrIterChain(source, [(step, args.single)]);
      }
      final rust = listMethodNames[node.methodName.name];
      if (rust == null) {
        throw Unsupported('`List.${node.methodName.name}`', node.toSource());
      }
      return IrCall(
        node.target == null ? null : expression(node.target!),
        rust,
        args,
      );
    }
    if (owner == 'Map') {
      final name = node.methodName.name;
      if (orderedMapMembers.contains(name)) {
        throw Unsupported(
          '`Map.$name`, which depends on insertion order',
          node.toSource(),
        );
      }
      final rust = mapMethodNames[name];
      if (rust == null) throw Unsupported('`Map.$name`', node.toSource());
      return IrCall(
        node.target == null ? null : expression(node.target!),
        rust,
        _arguments(node.argumentList, null, node),
      );
    }
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
    if (node.methodName.name == 'identical' && args.length == 2) {
      return IrIdentical(args[0], args[1]);
    }
    // dart:math's `max`/`min` and Flutter's `clampDouble`. Rust has all three,
    // and `max` is one spelling for floats and integers alike.
    const arithmetic = {'max': 'max', 'min': 'min'};
    final rust = arithmetic[node.methodName.name];
    if (rust != null && args.length == 2) {
      return IrCall(args[0], rust, [args[1]]);
    }
    if (node.methodName.name == 'clampDouble' && args.length == 3) {
      return IrCall(args[0], 'clamp', [args[1], args[2]]);
    }
    // A local function is a local holding a closure, so calling it is calling
    // that value -- not a method on `this`, and not a top-level function.
    if (element is LocalFunctionElement) {
      return IrCallValue(IrLocal(node.methodName.name), args);
    }
    // A top-level function is not a method on `this`. Without this check
    // `clampDouble(a, b, c)` came out as `self.clamp_double(a, b, c)`, which is
    // both wrong and a place the two front ends disagreed -- the Kernel one
    // refuses a top-level call outright.
    if (element is TopLevelFunctionElement) {
      return IrStaticCall(
        null,
        node.methodName.name,
        _arguments(node.argumentList, element, node),
      );
    }
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
      // Which class the call lands in is the *resolved target's*, not the
      // `extends` clause's. With `class X extends A with B`, a `super.foo()`
      // that B declares belongs to B, and the clause says A. The Kernel front
      // end reads it off the target for the same reason, and the mixin fixture
      // is what holds the two together.
      //
      // Falling back to `Object` when nothing resolves: every Dart class
      // extends it, written or not, and the backend answers it.
      final target = node.methodName.element?.enclosingElement;
      final base = target is InterfaceElement
          ? target.name
          : (_superclass ?? 'Object');
      return IrSuperCall(base ?? 'Object', node.methodName.name, args);
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
    if (node is SwitchStatement) {
      final cases = <IrCase>[];
      IrStmt? otherwise;
      var values = <IrExpr>[];
      for (final member in node.members) {
        if (member is SwitchDefault) {
          otherwise = _caseBody(member.statements);
          continue;
        }
        // Dart 3 parses `case Corner.topLeft:` as a *pattern* case, so the
        // constant has to be taken out of the pattern. Anything richer than a
        // constant is a real pattern match and is refused: Rust has patterns
        // too, but they are not these patterns.
        final Expression value;
        if (member is SwitchCase) {
          value = member.expression;
        } else if (member is SwitchPatternCase) {
          final pattern = member.guardedPattern.pattern;
          if (member.guardedPattern.whenClause != null) {
            throw Unsupported(
              'switch case with a `when` clause',
              node.toSource(),
            );
          }
          if (pattern is! ConstantPattern) {
            throw Unsupported(
              'switch case matching ${pattern.runtimeType}',
              node.toSource(),
            );
          }
          value = pattern.expression;
        } else {
          throw Unsupported(
            'switch member ${member.runtimeType}',
            node.toSource(),
          );
        }
        values.add(expression(value));
        if (member.statements.isEmpty) {
          // `case A: case B: ..` -- one arm matching several values, which is
          // `A | B =>`. Kernel gives it as one case with two expressions, so
          // both front ends arrive at the same arm.
          continue;
        }
        cases.add(IrCase(values, _caseBody(member.statements)));
        values = <IrExpr>[];
      }
      if (values.isNotEmpty) {
        throw Unsupported('switch case with no body', node.toSource());
      }
      return IrSwitch(expression(node.expression), cases, otherwise);
    }
    if (node is FunctionDeclarationStatement) {
      // A named function written inside a body: a closure bound to a local,
      // which is what Dart's is.
      final function = node.functionDeclaration;
      return IrLocalFunction(
        function.name.lexeme,
        _closure(function.functionExpression) as IrClosure,
      );
    }
    if (node is WhileStatement) {
      final previous = _breakLeavesSwitch;
      _breakLeavesSwitch = false;
      try {
        return IrWhile(expression(node.condition), statement(node.body));
      } finally {
        _breakLeavesSwitch = previous;
      }
    }
    if (node is BreakStatement) {
      if (node.label != null) {
        throw Unsupported('labelled break', node.toSource());
      }
      if (_breakLeavesSwitch) {
        // Not the `break` at the end of a case -- that one is dropped before
        // the body is lowered. This is a `break` in the middle, which means
        // leaving the switch early, and a Rust match arm cannot. Refused
        // rather than emitted: a bare `break` in an arm does not compile, and
        // one member failing to compile takes the whole file with it.
        throw Unsupported(
          'break out of a switch from inside a case',
          node.toSource(),
        );
      }
      // Labelled when the loop's body is a labelled block, because Rust will
      // not let a bare `break` cross one.
      return IrBreak(_loopLabel);
    }
    if (node is ContinueStatement) {
      if (node.label != null) {
        throw Unsupported('labelled continue', node.toSource());
      }
      final label = _continueLabel;
      return label == null ? const IrContinue() : IrBreak(label);
    }
    if (node is ForStatement) {
      // The same three parts as Kernel's, but they arrive inside one `parts`
      // node here. `for (x in xs)` is a different part kind entirely on this
      // side -- the analyzer keeps the source shape, where the CFE has already
      // lowered it to an iterator loop -- so it is refused, and the two front
      // ends only meet on the loops both of them see the same way.
      final parts = node.forLoopParts;
      if (parts is ForEachPartsWithDeclaration) {
        return IrForIn(
          parts.loopVariable.name.lexeme,
          expression(parts.iterable),
          statement(node.body),
        );
      }
      if (parts is! ForPartsWithDeclarations) {
        throw Unsupported(
          'for loop parts ${parts.runtimeType}',
          node.toSource(),
        );
      }
      final condition = parts.condition;
      // See the Kernel front end: Dart's `continue` in a `for` runs the
      // updates and Rust's does not, so with updates present the body becomes
      // a labelled block the `continue` breaks out of -- landing on the
      // updates, which is where Dart would have been.
      final (loopBody, label) = _forBody(node.body, parts.updaters.isNotEmpty);
      return IrBlock([
        for (final v in parts.variables.variables)
          IrLocalDecl(
            v.name.lexeme,
            _declaredType(parts.variables, v),
            v.initializer == null ? null : expression(v.initializer!),
          ),
        IrWhile(
          condition == null
              ? const IrLiteral('true', IrType('bool'))
              : expression(condition),
          IrBlock([
            loopBody,
            // `i = i + 1` is an assignment, and an assignment is a statement
            // on both sides of this compiler -- lowered as an expression it is
            // refused, which is what the fixture caught.
            for (final update in parts.updaters)
              update is AssignmentExpression
                  ? _assignment(update)
                  : IrExprStmt(expression(update)),
          ]),
          label: label,
        ),
      ]);
    }
    if (node is VariableDeclarationStatement) {
      final declared = node.variables.variables;
      if (declared.length != 1) {
        throw Unsupported('multiple declarators', node.toSource());
      }
      final v = declared.single;
      return IrLocalDecl(
        v.name.lexeme,
        _declaredType(node.variables, v),
        v.initializer == null ? null : expression(v.initializer!),
      );
    }
    if (node is TryStatement) {
      final finallyBlock = node.finallyBlock;
      if (finallyBlock != null) {
        // Kernel gives `try/catch/finally` as a TryFinally wrapping a TryCatch,
        // so this side builds the same two nodes rather than one node with
        // three parts -- otherwise the two front ends would disagree about the
        // shape while agreeing about the meaning.
        return IrTryFinally(
          node.catchClauses.isEmpty
              ? statement(node.body)
              : _catches(node, node.catchClauses),
          statement(finallyBlock),
        );
      }
      return _catches(node, node.catchClauses);
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

  /// The type of a local, written or inferred.
  ///
  /// `var out = ''` has no written type, and this used to leave the annotation
  /// off and let Rust infer it. Kernel always knows the type, so the two front
  /// ends said different things about the same line -- invisible until `for`
  /// started translating and brought a `var` into a fixture both of them
  /// reached. Resolution knows it here too; asking is better than leaving it,
  /// because Rust cannot always infer what Dart could.
  IrType? _declaredType(VariableDeclarationList list, VariableDeclaration v) {
    final written = list.type;
    if (written != null) return _type(written.type);
    return _type(v.declaredFragment?.element.type);
  }

  /// A case body, without the `break` Dart puts at the end of it.
  ///
  /// That `break` means "leave the switch", which a Rust match arm does by
  /// ending. One anywhere *but* the end would be leaving early, which an arm
  /// cannot do, so it is left in place to be refused.
  IrStmt _caseBody(List<Statement> statements) {
    final body =
        statements.isNotEmpty &&
            statements.last is BreakStatement &&
            (statements.last as BreakStatement).label == null
        ? statements.sublist(0, statements.length - 1)
        : statements;
    final previous = _breakLeavesSwitch;
    _breakLeavesSwitch = true;
    try {
      return IrBlock([for (final s in body) statement(s)]);
    } finally {
      _breakLeavesSwitch = previous;
    }
  }

  /// Whether a `break` reached now would be leaving a switch rather than a
  /// loop. A loop written inside a case takes its own `break` back.
  var _breakLeavesSwitch = false;

  /// The label a `continue` in the loop being lowered has to break out of.
  ///
  /// Null inside a `while`, where `continue` means what Rust's does.
  String? _continueLabel;

  /// The label on the loop being lowered, when it has one.
  String? _loopLabel;
  var _nextLabel = 0;

  /// Whether a `for` body needs to become a labelled block.
  ///
  /// Only when the loop has updates *and* something in it continues: without a
  /// `continue` the label would be a label nothing uses, and the two front ends
  /// have to arrive at the same text.
  bool _continues(Statement body) {
    final finder = _ContinueFinder();
    body.accept(finder);
    return finder.found;
  }

  (IrStmt, String?) _forBody(Statement body, bool hasUpdates) {
    final wasInSwitch = _breakLeavesSwitch;
    _breakLeavesSwitch = false;
    try {
      return _forBodyInner(body, hasUpdates);
    } finally {
      _breakLeavesSwitch = wasInSwitch;
    }
  }

  (IrStmt, String?) _forBodyInner(Statement body, bool hasUpdates) {
    final wraps = hasUpdates && _continues(body);
    // Both labels are allocated only when something needs them, and the loop's
    // before the body's -- which is the order the CFE allocates in, so the two
    // front ends arrive at the same numbers as well as the same shape.
    final loopLabel = wraps && _breaks(body) ? '__l${_nextLabel++}' : null;
    final bodyLabel = wraps ? '__l${_nextLabel++}' : null;
    final previousContinue = _continueLabel;
    final previousLoop = _loopLabel;
    _continueLabel = bodyLabel;
    _loopLabel = loopLabel;
    try {
      final lowered = statement(body);
      return (wraps ? IrLabeled(bodyLabel!, lowered) : lowered, loopLabel);
    } finally {
      _continueLabel = previousContinue;
      _loopLabel = previousLoop;
    }
  }

  bool _breaks(Statement body) {
    final finder = _ContinueFinder(wanted: 'break');
    body.accept(finder);
    return finder.found;
  }

  /// The catch half of a `try`, without its `finally`.
  ///
  /// Takes the clauses separately so a `try/catch/finally` can hand over just
  /// its catch part without a synthetic AST node being built to hold it.
  IrStmt _catches(TryStatement node, List<CatchClause> clauses) {
    if (clauses.length != 1) {
      throw Unsupported(
        'try with ${clauses.length} catch clauses',
        node.toSource(),
      );
    }
    final clause = clauses.single;
    final stack = clause.stackTraceParameter;
    if (stack != null && clause.body.toSource().contains(stack.name.lexeme)) {
      throw Unsupported('catch reading its stack trace', node.toSource());
    }
    final guard = clause.exceptionType?.type;
    final guardName = guard?.element?.name;
    return IrTryCatch(
      statement(node.body),
      clause.exceptionParameter?.name.lexeme ?? 'error',
      statement(clause.body),
      errorType: guardName == 'Object' ? null : guardName,
      stack: stack?.name.lexeme,
    );
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
  /// `a.b = v` where the value is wanted, matching the Kernel front end's rule:
  /// a field on `this`, and nothing else.
  IrExpr _assignmentValue(AssignmentExpression node) {
    final left = node.leftHandSide;
    if (node.operator.lexeme != '=') {
      throw Unsupported(
        'compound assignment used for its value',
        node.toSource(),
      );
    }
    if (left is SimpleIdentifier && left.element is LocalVariableElement) {
      return IrAssignValue(left.name, expression(node.rightHandSide));
    }
    if (left is PropertyAccess && left.target is ThisExpression) {
      final element = left.propertyName.element;
      if (element is! FieldElement && element is! PropertyAccessorElement) {
        throw Unsupported('assignment used for its value', node.toSource());
      }
      return IrSetValue(
        null,
        left.propertyName.name,
        expression(node.rightHandSide),
      );
    }
    throw Unsupported('assignment used for its value', node.toSource());
  }

  IrStmt _assignment(AssignmentExpression node) {
    final target = node.leftHandSide;
    if (target is IndexExpression) {
      if (node.operator.lexeme != '=') {
        throw Unsupported('compound assignment to an index', node.toSource());
      }
      if (target.target?.staticType?.element?.name == 'Map') {
        return IrExprStmt(
          IrCall(expression(target.target!), 'insert', [
            expression(target.index),
            expression(node.rightHandSide),
          ]),
        );
      }
      return IrIndexSet(
        expression(target.target!),
        expression(target.index),
        expression(node.rightHandSide),
      );
    }
    // `tint.opacity = v` where `tint` is a field of `this`. Handled here and
    // not through `_assignTarget`, which returns a name and loses the
    // receiver -- so the write came out as `self.opacity = v`, naming a field
    // of the wrong object and compiling if one happened to exist.
    if (target is PrefixedIdentifier && _isFieldOfThis(target.prefix)) {
      final receiver = IrField(null, target.prefix.name);
      final name = target.identifier.name;
      final value = expression(node.rightHandSide);
      if (node.operator.lexeme != '=') {
        throw Unsupported(
          'compound assignment through a field',
          node.toSource(),
        );
      }
      return IrAssignField(name, value, target: receiver);
    }
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

  /// Whether an identifier names a field of `this` -- so that a write through
  /// it is a write through `self`.
  bool _isFieldOfThis(SimpleIdentifier prefix) {
    final element = prefix.element;
    return element is PropertyAccessorElement || element is FieldElement;
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
    // Which parameters their callees *keep*, before anything is lowered: a
    // closure may borrow only if the callee is finished with it when it
    // returns. The Kernel front end reads this off the callee's body; here the
    // bodies are all in one unit, so they are collected once. See
    // `_ParameterEscapes` there and `bin/census_escapes.dart` for the count.
    final keeps = _KeptParameters();
    unit.accept(keeps);
    _keptParameters = keeps.kept;
    final classes = <IrClass>[];
    final constants = <IrConstDecl>[];
    final functions = <IrMethod>[];
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
      if (declaration is FunctionDeclaration) {
        try {
          functions.add(_lowerTopLevel(declaration));
        } on Unsupported catch (error) {
          refused.add('top-level ${declaration.name.lexeme}: $error');
        }
        continue;
      }
      if (declaration is! ClassDeclaration) continue;
      final (cls, problems) = lowerClass(declaration);
      classes.add(cls);
      refused.addAll(problems.map((p) => '${cls.name}: $p'));
    }
    return (
      IrLibrary(classes, constants: constants, functions: functions),
      refused,
    );
  }

  /// A top-level function, as a method with no receiver.
  IrMethod _lowerTopLevel(FunctionDeclaration node) {
    if (node.isGetter || node.isSetter) {
      throw Unsupported('a top-level accessor', node.name.lexeme);
    }
    final function = node.functionExpression;
    final params = <IrParam>[];
    for (final p in function.parameters?.parameters ?? const []) {
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
    return IrMethod(
      node.name.lexeme,
      params,
      _type(node.returnType?.type),
      body(function.body),
      typeParameters: [
        for (final p in function.typeParameters?.typeParameters ?? const [])
          p.name.lexeme,
      ],
      isStatic: true,
      doc: _doc(node),
    );
  }

  /// Lowers an enum. An enhanced one -- with methods -- is a Rust enum plus an
  /// impl, and that loses nothing; refusing it was right only while the
  /// alternative was emitting a plain one and dropping the members.
  ///
  /// Per-variant **state** is still out of reach: a Dart enum can give each
  /// value its own final fields, and a Rust enum would have to give every
  /// variant a payload to say the same thing. 5 of the 284 enums here do.
  (IrClass, List<String>) lowerEnum(EnumDeclaration node) {
    final refused = <String>[];
    // The names of the fields each value carries, in declaration order, and
    // what each value passed for them. `none(0)` gives the variant a `value`
    // of `0` -- a constant *of* the variant, so the Rust is a `match` in a
    // getter rather than a payload on the enum. The Kernel front end reads the
    // same thing off the evaluated constants; the enum fixture holds the two
    // to the same answer.
    final carried = [
      for (final member in node.members)
        if (member is FieldDeclaration && !member.isStatic)
          for (final v in member.fields.variables) v.name.lexeme,
    ];
    final valueFields = <String, Map<String, String>>{};
    var stateful = carried.isNotEmpty;
    if (stateful) {
      for (final constant in node.constants) {
        final args = constant.arguments?.argumentList.arguments ?? const [];
        if (args.length != carried.length) break;
        final own = <String, String>{};
        for (var i = 0; i < args.length; i++) {
          final literal = _enumLiteral(args[i]);
          if (literal == null) break;
          own[carried[i]] = literal;
        }
        if (own.length != carried.length) break;
        valueFields[constant.name.lexeme] = own;
      }
      // All of them or none: a getter that covers some variants is not a
      // getter.
      stateful = valueFields.length != node.constants.length;
    }
    if (stateful) {
      refused.add('unsupported an enum whose values carry fields');
    }
    final cls = IrClass(
      node.name.lexeme,
      isEnum: true,
      values: stateful
          ? const []
          : [for (final c in node.constants) c.name.lexeme],
      valueFields: stateful ? const {} : valueFields,
      doc: _doc(node),
    );
    if (!stateful) {
      for (final member in node.members) {
        if (member is! MethodDeclaration) continue;
        try {
          _lowerMethod(cls, member);
        } on Unsupported catch (error) {
          refused.add('${node.name.lexeme}.${member.name.lexeme}: $error');
        }
      }
    }
    return (cls, refused);
  }

  /// A literal an enum value passed to its constructor, as Rust.
  ///
  /// Only the four shapes the Kernel side admits, spelled the same way, so a
  /// fixture cannot pass on one front end and fail on the other.
  String? _enumLiteral(Expression argument) {
    if (argument is IntegerLiteral) return '${argument.value}';
    if (argument is DoubleLiteral) return '${argument.value}';
    if (argument is BooleanLiteral) return '${argument.value}';
    if (argument is SimpleStringLiteral) {
      final text = argument.value
          .replaceAll(r'\', r'\\')
          .replaceAll('"', r'\"');
      return '"$text".to_string()';
    }
    if (argument is PrefixExpression && argument.operator.lexeme == '-') {
      final inner = _enumLiteral(argument.operand);
      return inner == null ? null : '-$inner';
    }
    return null;
  }

  /// Lowers one class, collecting what it could not translate rather than
  /// stopping at the first refusal: a report of eleven unsupported members is
  /// worth more than a report of the first one.
  (IrClass, List<String>) lowerClass(ClassDeclaration node) {
    final element = node.declaredFragment?.element;
    final cls = IrClass(
      node.name.lexeme,
      typeParameters: [
        for (final p in node.typeParameters?.typeParameters ?? const [])
          p.name.lexeme,
      ],
      superclass: node.extendsClause?.superclass.name.lexeme,
      mixins: [
        for (final t in node.withClause?.mixinTypes ?? const []) _type(t.type),
      ],
      superclassArguments: [
        for (final t
            in node.extendsClause?.superclass.typeArguments?.arguments ??
                const [])
          _type(t.type),
      ],
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
        if (element is! FieldElement) {
          throw Unsupported('static ${element.runtimeType}', v.toSource());
        }
        // The initialiser is lowered from source rather than from the evaluated
        // constant. Both are available; the source keeps `Alignment(-1, -1)`
        // recognisable, and the evaluated value is what the driver prints to
        // check the two agree.
        final init = v.initializer;
        if (init == null)
          throw Unsupported('static without initialiser', v.toSource());
        cls.constants.add(
          IrConstDecl(
            v.name.lexeme,
            type,
            expression(init),
            doc: _doc(member),
            // A `static final` is computed once on first use, which is what
            // Rust's `LazyLock` is.
            isLazy: !element.isConst,
          ),
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
    if (member.factoryKeyword != null) {
      // A factory is an associated function returning Self, which is what
      // Dart's is: `Tinted.faint()` and `Tinted::faint()` are the same call.
      // A factory that returns a *subclass* or a cached instance needs the
      // hierarchy and is refused where its body refuses.
      cls.methods.add(
        IrMethod(
          name ?? 'new',
          params,
          IrType(cls.name),
          this.body(member.body),
          isStatic: true,
          doc: _doc(member),
        ),
      );
      return;
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
    // See `frontend_kernel.dart`: the body runs against the value being built,
    // which is a local here because Rust has no constructor phase. Dropping it
    // silently would emit a constructor that ignores its arguments, which is
    // why it was refused before there was anywhere to put it.
    final constructorBody = member.body;
    final IrStmt? body =
        constructorBody is BlockFunctionBody &&
            constructorBody.block.statements.isNotEmpty
        ? statement(constructorBody.block)
        : null;
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
        body: body,
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
          // Owned where the callee keeps it: see `IrParam.kept`.
          kept: _keptParameters.contains(
            '${member.name.lexeme}/${params.length}',
          ),
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
        typeParameters: [
          for (final p in member.typeParameters?.typeParameters ?? const [])
            p.name.lexeme,
        ],
        isStatic: member.isStatic,
        isGetter: member.isGetter,
        isSetter: member.isSetter,
        // Only plain `async`; a generator (`async*`, `sync*`) is not one.
        isAsync: member.body.isAsynchronous && !member.body.isGenerator,
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

/// Which parameters their callees keep, by `name/index`.
///
/// The one use a borrowed closure survives is being called; anything else --
/// stored in a field, added to a list, handed on -- outlives the call. Only
/// the declarations in this unit are seen, and a callee that is not here
/// counts as keeping: guessing the other way guesses that a borrow outlives
/// its borrower.
class _KeptParameters extends RecursiveAstVisitor<void> {
  final kept = <String>{};

  void _look(String? name, FormalParameterList? params, FunctionBody? body) {
    if (name == null || params == null || body == null) return;
    for (var i = 0; i < params.parameters.length; i++) {
      final p = params.parameters[i];
      final inner = p is DefaultFormalParameter ? p.parameter : p;
      final declared = inner.declaredFragment?.element;
      if (declared == null) continue;
      final walk = _ParameterEscapes(declared);
      body.accept(walk);
      if (walk.escapes) kept.add('$name/$i');
    }
  }

  @override
  void visitMethodDeclaration(MethodDeclaration node) {
    _look(node.name.lexeme, node.parameters, node.body);
    super.visitMethodDeclaration(node);
  }

  @override
  void visitFunctionDeclaration(FunctionDeclaration node) {
    _look(
      node.name.lexeme,
      node.functionExpression.parameters,
      node.functionExpression.body,
    );
    super.visitFunctionDeclaration(node);
  }
}

/// Every use of a parameter that is not "call it right here".
class _ParameterEscapes extends RecursiveAstVisitor<void> {
  _ParameterEscapes(this.param);

  final Element param;
  bool escapes = false;

  @override
  void visitFunctionExpressionInvocation(FunctionExpressionInvocation node) {
    final target = node.function;
    if (target is SimpleIdentifier && target.element == param) {
      node.argumentList.accept(this);
      return;
    }
    super.visitFunctionExpressionInvocation(node);
  }

  @override
  void visitMethodInvocation(MethodInvocation node) {
    // `f(..)` on a parameter holding a function is a MethodInvocation whose
    // target is null and whose name resolves to the parameter.
    if (node.target == null && node.methodName.element == param) {
      node.argumentList.accept(this);
      return;
    }
    super.visitMethodInvocation(node);
  }

  @override
  void visitSimpleIdentifier(SimpleIdentifier node) {
    if (node.element == param) escapes = true;
  }
}

/// The `final` fields a closure reads on `this`, and whether they all are.
class _FinalFieldReads extends RecursiveAstVisitor<void> {
  final fields = <String, FieldElement>{};
  bool allFinal = true;

  void _look(Element? element) {
    if (element == null) return;
    if (element.enclosingElement is! InterfaceElement) return;
    // A synthetic getter *is* the field; a written one is a computed getter
    // and reads whatever it likes.
    final field = switch (element) {
      GetterElement(isSynthetic: true) => element.variable,
      FieldElement() => element,
      _ => null,
    };
    if (field is FieldElement && !field.isStatic) {
      if (field.isFinal) {
        fields[field.name!] = field;
      } else {
        allFinal = false;
      }
    }
  }

  @override
  void visitSimpleIdentifier(SimpleIdentifier node) {
    _look(node.element);
    super.visitSimpleIdentifier(node);
  }

  @override
  void visitPropertyAccess(PropertyAccess node) {
    if (node.target is ThisExpression) _look(node.propertyName.element);
    super.visitPropertyAccess(node);
  }
}

/// Whether a closure asks more of `this` than a shared borrow.
///
/// The analyzer's half of `_ThisUse` in the Kernel front end. `this` is
/// usually implicit here, so the question is asked of the resolved element:
/// an instance field read is allowed, an instance field written or an instance
/// method named -- called or torn off -- is not.
class _InstanceDemand extends RecursiveAstVisitor<void> {
  bool demanding = false;

  bool _isInstance(Element? element) {
    if (element == null) return false;
    if (element.enclosingElement is! InterfaceElement) return false;
    return switch (element) {
      FieldElement() => !element.isStatic,
      PropertyAccessorElement() => !element.isStatic,
      MethodElement() => !element.isStatic,
      _ => false,
    };
  }

  @override
  void visitThisExpression(ThisExpression node) => demanding = true;

  @override
  void visitMethodInvocation(MethodInvocation node) {
    if (node.target == null && _isInstance(node.methodName.element)) {
      demanding = true;
    }
    super.visitMethodInvocation(node);
  }

  @override
  void visitAssignmentExpression(AssignmentExpression node) {
    final target = node.leftHandSide;
    if (target is SimpleIdentifier && _isInstance(target.element)) {
      demanding = true;
    }
    if (target is PropertyAccess && target.target is ThisExpression) {
      demanding = true;
    }
    super.visitAssignmentExpression(node);
  }

  @override
  void visitSimpleIdentifier(SimpleIdentifier node) {
    // A bare name that resolves to an instance *method* is a tear-off, which
    // needs the object itself and not a field of it.
    if (node.element is MethodElement && _isInstance(node.element)) {
      demanding = true;
    }
    super.visitSimpleIdentifier(node);
  }

  @override
  void visitSuperExpression(SuperExpression node) => demanding = true;
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

/// The analyzer's half of the return-inside-try check. Nested functions are
/// skipped: a `return` in a closure belongs to that closure.
class _EarlyExit extends RecursiveAstVisitor<void> {
  bool found = false;

  @override
  void visitFunctionExpression(FunctionExpression node) {}

  @override
  void visitReturnStatement(ReturnStatement node) {
    found = true;
  }
}

/// Looks for a `continue` belonging to the loop being lowered -- not to one
/// written inside it, and not to a closure's.
class _ContinueFinder extends RecursiveAstVisitor<void> {
  _ContinueFinder({this.wanted = 'continue'});

  final String wanted;
  bool found = false;

  @override
  void visitFunctionExpression(FunctionExpression node) {}

  @override
  void visitForStatement(ForStatement node) {}

  @override
  void visitWhileStatement(WhileStatement node) {}

  @override
  void visitDoStatement(DoStatement node) {}

  @override
  void visitContinueStatement(ContinueStatement node) {
    if (wanted == 'continue') found = true;
  }

  @override
  void visitBreakStatement(BreakStatement node) {
    if (wanted == 'break') found = true;
  }
}
