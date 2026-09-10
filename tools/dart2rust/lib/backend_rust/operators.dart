part of '../backend_rust.dart';

// Operators, `??`, options and literals.
augment class RustBackend {
  /// An erased twin's result back to what the call declared (see the
  /// prelude's `CastErased`). A projected `T?` -- this declaration's own
  /// parameter, nullable -- has no `CastErased` of its own: the value
  /// comes back as the `Option<T>` and goes out through `from_option`,
  /// as any projected value does (`find<T>()` returning `T?`, ws496).
  /// A class's method by name, this class's or one of its ancestors'.
  IrMethod? _methodOf(String? className, String name) {
    var c = className == null ? null : library[className];
    while (c != null) {
      final own = [
        ...c.methods,
        ...c.abstractMethods,
      ].where((m) => m.name == name && !m.isStatic).firstOrNull;
      if (own != null) return own;
      c = c.superclass == null ? null : library[c.superclass!];
    }
    return null;
  }

  String _erasedCast(IrType resultType, String call, {IrMethod? method}) {
    // A twin whose return does not mention the method's own parameters
    // (`getElementForInheritedWidgetOfExactType<T>` returns an
    // `InheritedElement?`) hands back the declared type already: no cast
    // (`CastErased<Option<Rc<dyn InheritedElement>>>` asked of itself,
    // ws503).
    if (method != null &&
        type(_substituteType(method.returnType, _erasure(method))) ==
            type(method.returnType)) {
      return call;
    }
    if (resultType.projected && resultType.nullable) {
      final inner = type(
        IrType(resultType.name, arguments: resultType.arguments),
      );
      return '<$inner as DartNullable>::from_option('
          'dart_cast_erased::<Option<$inner>, _>($call))';
    }
    return 'dart_cast_erased::<${type(resultType)}, _>($call)';
  }

  /// `a ?? b`, in the one of four spellings Rust needs.
  ///
  /// Two questions decide it, and both come from the front end because the IR
  /// carries no expression types:
  ///
  /// * **Is the result still nullable?** `a ?? b` is non-null exactly when `b`
  ///   is. `unwrap_or_else` produces a value, `or_else` produces an Option, and
  ///   using the wrong one does not type-check -- which is how nested `??`
  ///   found this, since `a ?? b ?? c` has a nullable `a ?? b` inside it.
  /// * **May the right side be evaluated eagerly?** Dart's `??` is
  ///   short-circuit and Rust's `unwrap_or`/`or` are not. Only a literal is
  ///   safe; 77% of upstream's right-hand sides are calls, constructors or
  ///   throws.
  String _ifNull(IrIfNull node) {
    // A `match` on a place moves out of it, and the place lives on
    // (`final child = inactive ?? create(); if (inactive != null) ..` in
    // `Element.inflateWidget`, ws494): a local or a field of `this` is
    // read by clone.
    final operand = node.left;
    // The scrutinee bound first: a `match` keeps its scrutinee's
    // temporaries -- the `Ref` of a `.borrow().clone()` -- alive through
    // every arm, and the `None` arm of `_instance ??= X()` on a static
    // cell wrote through `borrow_mut()` into it ("already borrowed",
    // run516). A `let` drops them at its own end.
    final read = operand is IrLocal
        ? '${expr(operand)}.clone()'
        : expr(operand);
    final left = '{ let __scrutinee = $read; __scrutinee }';
    if (node.right is IrThrowValue) {
      // `a ?? throw e`. The closure forms are wrong here for the reason a try
      // body could not hold a `?`: the `return Err(e)` inside `unwrap_or_else`
      // would return from the *closure*. A match has no closure to escape
      // from, and the arm that throws simply diverges.
      return 'match $left { '
          'Some(__value) => __value, '
          'None => ${expr(node.right)} }';
    }
    final right = expr(node.right);
    // The lazy side as a `match`, not an `or_else(|| ..)`: a closure is its
    // own function, and an `.await` inside one -- `a ?? await b()` -- is
    // "await outside async". `match` keeps the laziness and stays in the
    // enclosing function. 13 `E0728`s.
    if (node.nullableResult) {
      return node.eager
          ? '$left.or($right)'
          : 'match $left { Some(__value) => Some(__value), None => $right }';
    }
    if (!node.eager) {
      return 'match $left { Some(__value) => __value, None => $right }';
    }
    return node.eager
        ? '$left.unwrap_or($right)'
        : '$left.unwrap_or_else(|| $right)';
  }

  /// Dart's binary operators in Rust's spelling.
  ///
  /// Most are the same token and pass straight through. The ones that are not
  /// are the reason this is a function and not string interpolation:
  ///
  /// * `~/` is truncating division and has no Rust operator at all. On floats
  ///   it is `(a / b).trunc()`; the `.toDouble()` Dart then needs is dropped
  ///   in `_call`, because the result is already an `f32`.
  /// * `??` takes the left unless it is null.
  ///
  /// An operator not listed and not passed through would be silently wrong, so
  /// anything unrecognised stops.
  String _binary(String op, IrExpr left, IrExpr right, [IrType? type]) {
    // A comparison against a line Dart's AOT compiler proved dead: the dead
    // side is spelled at the live side's type.
    //
    // `unreachable!(..)` is a `!`, and a `!` with nothing to constrain it
    // falls back to `()` -- then `A == B` asks for `(): PartialEq<i64>`,
    // which nobody implements ("can't compare `()` with `i64`",
    // `_buildDayPicker`'s `widget.minimumDate!.month == selectedMonth`,
    // where TFA proved `minimumDate` is never non-null in this program).
    // Binding it is enough, because a `!` coerces to anything at a `let`.
    //
    // Only a comparison, and that is not a hedge: `&&` and `||` take each
    // side as its own `bool`, so nothing has to unify and the fallback is
    // harmless. A comparison needs `A: PartialEq<B>`, so an unconstrained
    // `A` is the whole problem. Three sites, two stubs -- counted, with
    // every other diverging operand in the program left alone.
    const comparisons = {'==', '!=', '<', '>', '<=', '>='};
    if (comparisons.contains(op)) {
      final deadLeft = _neverReturns(left);
      final deadRight = _neverReturns(right);
      if (deadLeft != deadRight) {
        final live = (deadLeft ? right : left).rustType;
        if (live != null) {
          final dead = expr(deadLeft ? left : right);
          final bound = '{ let __never: ${this.type(live)} = $dead; __never }';
          return deadLeft
              ? '($bound $op ${expr(right)})'
              : '(${expr(left)} $op $bound)';
        }
      }
    }
    if (op == '+' && type?.name == 'String') {
      // `String + String` is not Rust. `format!` is, it needs no borrow worked
      // out at either end, and it is what Dart's `+` on two strings means.
      return 'format!("{}{}", ${expr(left)}, ${expr(right)})';
    }
    const passthrough = {
      '+',
      '-',
      '*',
      '/',
      '%',
      '==',
      '!=',
      '<',
      '>',
      '<=',
      '>=',
      '&&',
      '||',
      '&',
      '|',
      '^',
      '<<',
      '>>',
    };
    if (op == '~/') return '((${expr(left)} / ${expr(right)}).trunc())';
    if (op == '??') {
      // Dart's `??` is short-circuit: the right side is evaluated only when the
      // left is null. Rust's `unwrap_or` evaluates it **always**, so it is right
      // only for a value that has no effects and costs nothing -- and this used
      // `unwrap_or` for everything from round two until the corpus was counted.
      //
      // Of 6764 `??` in package:flutter only 23% have a literal or constant on
      // the right. The rest are calls, constructors, and in six places a
      // `throw` -- where eager evaluation does not give a wrong answer, it
      // throws unconditionally.
      //
      // A literal keeps the shorter form because it reads better and is
      // provably safe; everything else defers.
      if (right is IrLiteral) {
        return '${expr(left)}.unwrap_or(${expr(right)})';
      }
      return '${expr(left)}.unwrap_or_else(|| ${expr(right)})';
    }
    // Dart's `>>>` is the logical shift on the 64-bit pattern; Rust's `>>`
    // on `i64` is arithmetic, and on `u64` it is this (`_TrieNode.
    // _trieIndex`, `_bitCount`: the `PersistentHashMap` every
    // `InheritedElement` mounts through, run537).
    // Dart's shifts on an `int`, by count: the prelude's, which give 0 (or
    // the sign) past 63 where Rust's operators panic (`_trieIndex` at bit
    // index 65, ws557). Only on an `int` left operand: the byte and mask
    // arithmetic on other widths keeps the operator.
    final leftInt =
        left.rustType?.name == 'int' ||
        (left is IrLiteral && left.type.name == 'int');
    if (op == '>>>' || ((op == '<<' || op == '>>') && leftInt)) {
      final helper = switch (op) {
        '<<' => 'dart_shl',
        '>>' => 'dart_shr',
        _ => 'dart_ushr',
      };
      return '$helper((${expr(left)}) as i64, (${expr(right)}) as i64)';
    }
    if (!passthrough.contains(op)) {
      throw Unsupported('binary operator `$op`', '${expr(left)} $op ...');
    }
    // An operator on an open class's handle: `Rc<dyn Size> * f64` has no
    // `impl std::ops::Mul` to land on (the orphan rule: `Rc` is not
    // fundamental), so it is the trait's method, which fails like any
    // method (`Size.lerp`, ws473).
    final leftName = left.rustType?.name;
    final mapping = operatorTraits[op];
    if (mapping != null &&
        leftName != null &&
        library[leftName] != null &&
        library.isAbstract(leftName)) {
      return '${expr(left)}.op_${mapping.$2}(${expr(right)})$_propagate';
    }
    // ..and on a counted class's handle: the `impl std::ops::Mul` is
    // `for Struct`, so the *left* operand is the value the handle holds,
    // cloned (`Rc<Matrix4> * Rc<Matrix4>`, ws511). The right is not: the
    // impl's `Rhs` is the operator's parameter as it was declared, and a
    // counted class named in a parameter is its handle -- `Mul<Rc<Matrix3>>
    // for Matrix3`, `Mul<Rc<dyn Object>>` where the parameter is `dynamic`.
    // Dereferencing it too handed `Vector3` to a `Rc<Vector3>` slot (17 at
    // ws747, across Vector3, _Vector, OffsetPair, AttributedString).
    if (mapping != null) {
      final name = left.rustType?.name;
      final counted = name != null && (library[name]?.counted ?? false);
      if (counted && !left.rustType!.nullable) {
        return '((*${expr(left)}).clone() $op ${expr(right)})';
      }
    }
    // `==` on a type parameter's values (`T`, `T?`) is Dart's `==`, the
    // prelude's `DartEq`, which every parameter carries; `PartialEq` is
    // not asked of one (`selected == value` on a `T?` in
    // `CupertinoSegmentedControl`, 7 at ws460).
    // ..and on any object that is not a primitive: Dart's `==` is the
    // class's `operator ==`, which is `DartEq` here, taken by reference
    // (`==` on two `Rc<dyn Size>` moved its operand, E0382, 53 at ws464).
    if ((op == '==' || op == '!=') &&
        (_ownsParameter(left.rustType) ||
            _ownsParameter(right.rustType) ||
            (_objectLike(left.rustType) && _objectLike(right.rustType)))) {
      // `DartEq` compares two of the *left's* type: the right operand
      // was shared into `Object` for Dart's `operator ==(Object)`, and is
      // shared into the left's trait instead (`&Rc<dyn Object>` where
      // `&Rc<dyn Color>` was wanted, 12 at ws467).
      final leftType = left.rustType;
      final bare = right is IrUpcast && right.type.name == 'Object'
          ? right.value
          : right;
      // ..and where this module's world cannot classify the value (a
      // struct of another library), the `Object` sharing stands: the
      // prelude's `dart_object` takes the trait the comparison wants.
      // Two handles of one trait at different instantiations (`Route<T>`
      // against the navigator's `Route<dynamic>`, ws505): Dart's `==` on
      // them is identity, and only a thin pointer can compare the two.
      final rightType = right.rustType;
      if (leftType != null &&
          rightType != null &&
          leftType.name == rightType.name &&
          !leftType.nullable &&
          !rightType.nullable &&
          library.isAbstract(leftType.name) &&
          leftType.arguments.isNotEmpty &&
          leftType.arguments.toString() != rightType.arguments.toString()) {
        final same = 'dart_identical_any(&${expr(left)}, &${expr(right)})';
        return op == '==' ? same : '(!$same)';
      }
      // Two handles of different traits: the one *below* goes up into the
      // other's type -- never the other way, which is a cast that fails
      // (`next?.route != entry.lastAnnouncedNextRoute`, a `Route?` against
      // a `_RoutePlaceholder?` above it, run636); unrelated ones compare
      // as the objects they are.
      final bareType = bare.rustType;
      if (leftType != null && bareType != null) {
        final ln = stripNull(leftType).name;
        final rn = stripNull(bareType).name;
        if (ln != rn && library.isAbstract(ln) && library.isAbstract(rn)) {
          if (_world.isBelow(ln, rn) && !_world.isBelow(rn, ln)) {
            final lifted = coerceInto(left, bareType, _world, inClosure: true);
            final eq = '${expr(lifted)}.dart_eq(&${expr(bare)})';
            return op == '==' ? eq : '(!$eq)';
          }
          if (!_world.isBelow(rn, ln)) {
            final eq =
                'dart_option_object(${_asOption(left)}).dart_eq(&dart_option_object(${_asOption(bare)}))';
            return op == '==' ? eq : '(!$eq)';
          }
        }
      }
      final coerced = leftType != null && bare.rustType != null
          ? coerceInto(bare, leftType, _world, inClosure: true)
          : bare;
      final other = identical(coerced, bare) ? right : coerced;
      final eq = '${expr(left)}.dart_eq(&${expr(other)})';
      return op == '==' ? eq : '(!$eq)';
    }
    return '(${expr(left)} $op ${expr(right)})';
  }

  /// A value as an `Option` for `dart_option_object`: itself when its
  /// recorded type is nullable, `Some(..)` otherwise.
  String _asOption(IrExpr e) {
    final t = e.rustType;
    return t != null && isNullable(t) && !t.projected
        ? expr(e)
        : 'Some(${expr(e)})';
  }

  /// The type a `<T as DartNullable>` projection names: the parameter
  /// itself when it is one in scope, else the concrete type it was
  /// substituted with, spelled (`<Rc<dyn Object> as DartNullable>`, not
  /// `<Object as ..>`: E0782 26 and `dynamic` 25 at ws465).
  String _nullableOf(String parameter) {
    // ..bound to one instantiation inside a wider impl (`_selfBinding`).
    final bound = _selfBinding[parameter];
    if (bound != null) return type(bound);
    if (cls.typeParameters.contains(parameter) ||
        _methodTypeParams.contains(parameter)) {
      return parameter;
    }
    return type(IrType(parameter));
  }

  /// `::<i64>` for the class's own parameters inside a wider impl for one
  /// instantiation (`_selfBinding`), so that `ConstantTween::lerp(self,
  /// t)` names `ConstantTween::<f64>` and rustc does not infer the class's
  /// `T` from the trait's return type instead (ws627); empty otherwise.
  String _selfTurbofish() {
    if (_selfBinding.isEmpty) return '';
    final args = [
      for (final p in cls.typeParameters)
        if (_selfBinding[p] != null) type(_selfBinding[p]!),
    ];
    if (args.length != cls.typeParameters.length) return '';
    return '::<${args.join(', ')}>';
  }

  /// A type in the class's own terms, as the wider impl being written
  /// binds them (`_selfBinding`), for the coercion rule to compare with
  /// the trait's side; the type itself outside one.
  IrType _selfBound(IrType t) =>
      _selfBinding.isEmpty ? t : _substituteType(t, _selfBinding);

  /// Whether a type is a translated class's, a trait's, or a collection's
  /// -- anything `==` compares by `DartEq` rather than by value.
  bool _objectLike(IrType? t) {
    if (t == null || t.isFunction) return false;
    final name = t.name;
    // A `dynamic` (an `Object?`) compares by `DartEq` too: the raw `==`
    // on two `Rc<dyn Object>` moved its right operand (E0382, a pattern
    // switch on `data['platformBrightness']`, run502), and the prelude's
    // `DartEq for dyn Object` is the value comparison either way.
    if (const {
      'int',
      'double',
      'num',
      'bool',
      'String',
      'void',
      '()',
      'Null',
      'Type',
      'Option',
    }.contains(name)) {
      return false;
    }
    if (name == 'dynamic' || name == 'Object') return true;
    final c = library[name];
    if (c != null && c.isEnum) return false;
    return c != null || const {'List', 'Map', 'Set', 'Iterable'}.contains(name);
  }

  /// Whether a type is a type parameter of the class or method being
  /// printed, or its nullable form.
  bool _ownsParameter(IrType? t) =>
      t != null &&
      t.arguments.isEmpty &&
      !t.isFunction &&
      (cls.typeParameters.contains(t.name) ||
          _methodTypeParams.contains(t.name));

  /// A Dart string's contents, safe to sit inside a Rust `"..."`.
  ///
  /// The backslash has to be doubled *before* the quote is escaped, or the
  /// backslash this step just added would be doubled by the next one. Only
  /// these two characters need it: Rust and Dart agree on the rest.
  /// A Dart string as a Rust literal.
  ///
  /// The backslash and the quote were escaped from the start. The control
  /// characters were not, and a carriage return written raw into a Rust
  /// literal is a hard error -- `bare CR not allowed in string` -- 108 times
  /// across upstream, which mostly writes them inside `\r\n`.
  String _escape(String text) => text
      .replaceAll('\\', '\\\\')
      .replaceAll('"', '\\"')
      .replaceAll('\r', '\\r')
      .replaceAll('\n', '\\n')
      .replaceAll('\t', '\\t')
      .replaceAll('\u0000', '\\0')
      // Text-direction controls (the l10n files have them) are rejected raw
      // by rustc's `text_direction_codepoint_in_literal`; written as escapes
      // they are the same string. 23 literals.
      .replaceAllMapped(
        RegExp('[\u200E\u200F\u202A-\u202E\u2066-\u2069]'),
        (m) => '\\u{${m[0]!.codeUnitAt(0).toRadixString(16)}}',
      );

  String _literal(String value, IrType t) {
    if (t.name == 'double') {
      // Rust needs the point: `1` is an integer literal even in an f32 context.
      return value.contains('.') || value.contains('e') ? value : '$value.0';
    }
    // Escaped for the same reason the assert message is: a Dart string holding
    // a quote or a backslash would otherwise end the Rust literal early or
    // start an escape that was never in the source.
    if (t.name == 'String') return '"${_escape(value)}".to_string()';
    if (t.name == 'Null') return 'None';
    return value;
  }

  /// The free function that holds a base class's own body for `name`.
  ///
  /// Rust has no `super`. Once an impl overrides a trait's default method the
  /// default is unreachable -- `Trait::name(self)` dispatches back to the
  /// override and the program hangs. So every concrete method on an abstract
  /// class is emitted twice: once as a free generic function holding the body,
  /// and once as the trait default, which calls it. `super.name(..)` then names
  /// the function, which is the one thing that cannot dispatch anywhere else.
  /// A getter and a setter share a Dart name and must not share a Rust one.
  ///
  /// `RenderBox` has `Size get size` and `set size(Size)`, and both produced
  /// `render_box_super_size` -- the same collision round 62 found in the trait
  /// impls, one level over in the free functions that hold the bodies.
  static String superFn(
    String base,
    String name, {
    bool isSetter = false,
  }) => _rustIdentifier(
    '${snakeRaw(base)}_super_${isSetter ? 'set_' : ''}'
    '${RegExp(r'^[A-Za-z_][A-Za-z0-9_]*$').hasMatch(name) ? snakeRaw(name) : _operatorName(name)}',
  );

  /// A static call, checked against the IR when it lands in this library.
  ///
  /// `Alignment._stringify(x, y)` was emitted for a method the front end had
  /// refused, so the output named a function nobody wrote. That is round one's
  /// bug in a new shape: it was masked then by refusing every private reference,
  /// and removing that blunt rule brought it back. The precise rule is the same
  /// one `_superCall` uses -- if the callee is in this file, it has to be in the
  /// IR.
  /// Whether a top-level name is one of the library's mutable variables.
  bool _isMutableTopLevel(String name) =>
      library.constants.any((c) => c.name == name && c.isMutable) ||
      // Another module's: `numberFormatSymbols` read from `NumberFormat`
      // was a bare `NUMBER_FORMAT_SYMBOLS.get(..)` against its `LazyLock`.
      (library.constantsElsewhere[name]?.isMutable ?? false);

  /// `List.generate(n, f)` and friends, which are Dart's list constructors
  /// wearing a static's clothes. Rust builds a `Vec` from an iterator.
  static const _listStatics = {
    'generate',
    'filled',
    'from',
    'of',
    'unmodifiable',
  };

  /// A value spelled projected (`<T as DartNullable>::Or`, a read out of
  /// a `Vec<T?>` or a generic accessor) where an `Option` operation --
  /// `!`, `== null`, `?.`, `??`, `==` -- wants the `Option<T>` a body works
  /// with: the prelude's conversion first.
  /// A type the prelude's `FromDynamic` converts: what a `dynamic` holds
  /// as itself, the scalars, and collections of those.
  bool _dynamicRepresentable(IrType t) {
    if (t.isFunction) return false;
    if (const {
      'Object',
      'dynamic',
      'String',
      'int',
      'double',
      'bool',
    }.contains(t.name)) {
      return true;
    }
    return (t.name == 'List' || t.name == 'Map') &&
        t.arguments.isNotEmpty &&
        t.arguments.every(_dynamicRepresentable);
  }

  /// `.flatten()` after a map lookup whose value type is itself nullable.
  String _flattenedValue(IrExpr? map) {
    final t = map?.rustType;
    if (t == null || t.arguments.length != 2) return '';
    return t.arguments[1].nullable ? '.flatten()' : '';
  }

  /// A closure's return type, spelled where inference has nothing to go
  /// on: a body ending in `Ok(None)` -- a `Null`-returning closure handed
  /// to the prelude's `then` -- left `Option<_>` open (`_initKeyboard`,
  /// run477). Elsewhere `_`, as before.
  String _closureReturnSpelled(IrType returns) {
    if (returns.isFunction || returns.name == 'raw') return '_';
    if (returns.name == 'Null' || returns.nullable) return type(returns);
    // A trait, spelled: then the `Ok(..)` around the body is a coercion
    // site, and a concrete handle unsizes into the trait object there.
    // Left `_`, `List<Widget>.generate(n, (i) => _VisibilityScope(..))`
    // collected a `Vec<Rc<_VisibilityScope>>` where `Vec<Rc<dyn Widget>>`
    // went -- the cast has nowhere to be written (12 at ws747).
    if (library.isAbstract(returns.name) && !_mentionsUnknown(returns)) {
      return type(returns);
    }
    return '_';
  }

  /// The captured cell locals that hold a `late` field (see `_cellLocals`).
  Set<String> _lateCellLocals = const {};

  /// `Some(v)`, with a closure inside it unsized to the function type it
  /// is typed as. A struct literal's field spells the slot
  /// (`Option<Rc<dyn Fn(..)>>`) and Rust still does not unsize a closure
  /// through the `Some` on the way in: the `Rc<{closure}>` stayed one
  /// where a `FormField<T>`'s erased validator went (ws856). Only where
  /// the type is spelled -- a closure whose own type this is -- since
  /// spelling it everywhere named type parameters out of scope (ws551).
  String _some(IrExpr value) {
    final held = value is IrClosure && !value.boxed
        ? 'std::rc::Rc::new(${expr(value)})'
        : expr(value);
    final t = value.rustType;
    if (t != null && t.isFunction && _closureLike(value)) {
      return 'Some({ let __f: ${type(t)} = $held; __f })';
    }
    return 'Some($held)';
  }

  /// A mapped element's body: the closure around it is one the prelude
  /// calls, and its return is a plain value. A failing call inside
  /// unwraps, as a chain step's does (`_stepClosure`); left propagating,
  /// the `?` had no `Result` to come out of -- "the `?` operator can only
  /// be used in a closure that returns `Result`" (3 in `Navigator` at
  /// ws871).
  String _mappedBody(IrExpr body) {
    final saved = _failure;
    _failure = null;
    final text = expr(body);
    _failure = saved;
    return text;
  }

  /// A closure, possibly behind the wrappers coerce puts on one (a
  /// `Some`, an upcast, a clone).
  bool _closureLike(IrExpr e) => switch (e) {
    IrClosure() => true,
    IrUpcast(:final value) => _closureLike(value),
    IrSome(:final value) => _closureLike(value),
    // The adapter `coerce` makes of a bound function value (`{ let __f =
    // ..; Rc::new(move |..| ..) }`): its map's return is spelled, or the
    // `Rc<{closure}>` never unsized (a conditional tear-off into
    // `VoidCallback?`, ws551).
    IrBlockValue(:final value) => _closureLike(value),
    IrCall(:final target, :final name, :final args) =>
      (name == 'clone' || name == '!rc') &&
          args.isEmpty &&
          target != null &&
          _closureLike(target),
    _ => false,
  };

  /// A type that cannot be spelled as a return (`_`, a placeholder, a
  /// method's own parameter nothing declares here).
  bool _mentionsUnknown(IrType t) {
    if (t.name == '_' || t.name == 'raw' || t.name.isEmpty) return true;
    if (t.isFunction) {
      return t.parameters!.any(_mentionsUnknown) ||
          (t.returns != null && _mentionsUnknown(t.returns!));
    }
    return t.arguments.any(_mentionsUnknown);
  }

  /// Whether the null-aware body being printed binds its value by value
  /// (a scalar receiver) rather than by reference.
  bool _boundByValue = false;

  /// Whether a null-aware body hands the binding itself back: a `?..`
  /// cascade's block, whose last expression is the bound name.
  static bool _endsAtBound(IrExpr body) => switch (body) {
    IrBound() => true,
    IrBlockValue(:final value) => _endsAtBound(value),
    _ => false,
  };

  /// That body with the binding cloned where it is produced.
  String _clonedBound(IrExpr body) => switch (body) {
    IrBound() => '$_boundName.clone()',
    IrBlockValue(:final statements, :final value) => () {
      final saved = _out.length;
      final savedIndent = _indent;
      _indent = 0;
      for (final s in statements) {
        stmt(s);
      }
      final written = _out.sublist(saved).join(' ');
      _out.removeRange(saved, _out.length);
      _indent = savedIndent;
      return '{ $written ${_clonedBound(value)} }';
    }(),
    _ => expr(body),
  };
}
