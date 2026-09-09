part of '../frontend_kernel.dart';

// Static calls, and filling a callee’s parameters.
augment class KernelFrontend {
  IrExpr _staticInvocation(StaticInvocation node) {
    // TFA spells a cast it has proven, or one it cannot check, as
    // `unsafeCast<Clock?>(Zone.current[_clockKey])`. It is the `as` it
    // replaced, and lowers as one -- without this the operand stood in for
    // the whole and an `Rc<dyn Object>` landed in an `Option<Clock>`.
    if (node.target.name.text == 'unsafeCast' &&
        node.target.enclosingLibrary.importUri.toString() == 'dart:_internal' &&
        node.arguments.positional.length == 1 &&
        node.arguments.types.length == 1) {
      // ..and into the cast's type: `unsafeCast<double?>(#1{Size}.width)`
      // hands a `double` to a `double?` (`Some(..)`), which the CFE's `#0`
      // above it is declared as.
      final operand = node.arguments.positional.single;
      final to = node.arguments.types.single;
      final lowered = expression(AsExpression(operand, to));
      // Only that shape: a non-null `T` into a `T?`. Widening every
      // `unsafeCast` doubled `Option`s and unwrapped `dynamic`s (+17, ws159).
      final from = _staticType(operand);
      if (from is InterfaceType &&
          to is InterfaceType &&
          from.classNode == to.classNode &&
          from.nullability != Nullability.nullable &&
          to.nullability == Nullability.nullable) {
        // ..reading the value back first: what is in hand may be the
        // bound an erased slot handed out, and `Some(x)` around an
        // `Rc<dyn Object>` is no `Option<Rc<dyn Cursor>>`
        // (`mouseCursor?.resolve(states)` in
        // `ToggleableStateMixin.buildToggleable`, ws707).
        IrExpr inner = lowered;
        try {
          inner = coerce(
            lowered,
            _type(to.withDeclaredNullability(Nullability.nonNullable)),
          );
        } on Unsupported {
          inner = lowered;
        }
        return IrSome(inner)..rustType = _type(to);
      }
      return lowered;
    }
    final target = node.target;
    final declaration = _preludeDeclaration(target).function;
    final positional = node.arguments.positional;
    // Two of dart:math's, and one of Flutter's own. Rust has all three, and
    // `max` is the same spelling for floats and integers because `f32::max` is
    // inherent and `Ord::max` covers the rest. 372 `max` and 184 `clampDouble`.
    const arithmetic = {'max': 'max', 'min': 'min'};
    final rust = arithmetic[target.name.text];
    if (rust != null && positional.length == 2) {
      // `max(0, x)` with an `int` and a `double`: the `int` is cast, as
      // the operators cast theirs (`0.max(f64)` was 3 `found integer`s).
      String? cls(Expression e) {
        final t = _staticType(e);
        return t is InterfaceType ? t.classNode.name : null;
      }

      var a = expression(positional[0]);
      var b = expression(positional[1]);
      if (cls(positional[0]) == 'int' && cls(positional[1]) == 'double') {
        a = _toF64(a);
      } else if (cls(positional[0]) == 'double' &&
          cls(positional[1]) == 'int') {
        b = _toF64(b);
      }
      // On a `T extends num`: the prelude's numeric protocol (`DartNum`),
      // whose names do not collide with `Ord`'s on a known number
      // (`AnimationMin<T extends num>.value`, run676).
      final first = _staticType(positional[0]);
      if (first is TypeParameterType && !_erasedParameter(first.parameter)) {
        return IrCall(a, 'dart_$rust', [b])..rustType = _type(first);
      }
      return IrCall(a, rust, [b]);
    }
    // The CFE lowers `<int>[3, 11, 29]` to `_GrowableList._literal3(..)`, so a
    // list literal never reaches this compiler as a ListLiteral. Restored
    // rather than transliterated, for the same reason `??` and cascades are:
    // the analyzer front end sees the literal, and the two have to agree.
    final owner = target.enclosingClass?.name;
    // `Uint8List.view(buffer, [offset, length])`: a `Vec<u8>` cannot carry
    // an associated function; the prelude's free one.
    if (owner == 'Uint8List' &&
        target.name.text == 'view' &&
        positional.isNotEmpty) {
      return IrStaticCall(
        null,
        'uint8_list_view',
        _arguments(node.arguments, target.function),
      );
    }
    // `Uint8List.sublistView(data, [start, end])` / `ByteData.sublistView`:
    // a copy of the bytes here (a `TypedData` is its bytes: a `ByteData` or
    // a `Uint8List`), the prelude's free functions (`StandardMessageCodec`,
    // on every platform message; ws493).
    if ((owner == 'Uint8List' || owner == 'ByteData') &&
        target.name.text == 'sublistView' &&
        positional.isNotEmpty) {
      return IrStaticCall(
        null,
        owner == 'Uint8List'
            ? 'uint8_list_sublist_view'
            : 'byte_data_sublist_view',
        _arguments(node.arguments, target.function),
      );
    }
    // `int.parse(s)` / `double.tryParse(s)`: the prelude's four functions.
    // `intl`'s field parsers and 30-odd other sites.
    if ((owner == 'int' || owner == 'double') &&
        (target.name.text == 'parse' || target.name.text == 'tryParse') &&
        positional.length == 1) {
      final named = node.arguments.named;
      final head = target.name.text == 'parse' ? 'parse' : 'try_parse';
      if (named.isEmpty) {
        return IrStaticCall(null, '${head}_$owner', [
          expression(positional[0]),
        ]);
      }
      // ..and `int.parse(s, radix: r)`, which is the same four functions
      // with the base said out loud. `double` has no radix
      // (`DefaultMaterialLocalizations.parseCompactDate`, refused since it
      // was written).
      if (owner == 'int' && named.length == 1 && named.single.name == 'radix') {
        return IrStaticCall(null, '${head}_int_radix', [
          expression(positional[0]),
          coerce(
            expression(named.single.value),
            const IrType('int', nullable: true),
          ),
        ]);
      }
    }
    // `_List<T?>(n)` -- `List.filled(n, null)` after the CFE -- is a list of
    // `n` nulls, which for a nullable element is exactly what it says: the
    // prelude's `vec_of_nones`. A non-nullable element has nothing to fill
    // with and stays refused in the backend. `_makeArray` in
    // `persistent_hash_map.dart`, and everything hashing through it.
    if (owner == '_List' &&
        target.name.text.isEmpty &&
        positional.length == 1 &&
        node.arguments.types.length == 1 &&
        node.arguments.types.single.nullability == Nullability.nullable) {
      // With the element type spelled: for a projected `E?` (a generic
      // class's `List<E?>`) the prelude fills with `<E as DartNullable>::Or`
      // nulls, which nothing could infer (`HeapPriorityQueue._queue`, run436).
      final element = node.arguments.types.single;
      // ..and a `dynamic` element (`List<Object?>`, which is one) holds
      // its nulls as `Null` objects, not as `None`s (ws499).
      final elementIr = _type(element);
      if (elementIr.name == 'dynamic' && !elementIr.nullable) {
        return IrStaticCall(null, 'vec_of_nulls', [expression(positional[0])])
          ..rustType = IrType('List', arguments: [elementIr]);
      }
      return IrStaticCall(
        null,
        'vec_of_nones',
        [expression(positional[0])],
        typeArguments: [
          _type(element.withDeclaredNullability(Nullability.nonNullable)),
        ],
      );
    }
    // `_GrowableList(0)` -- `List.empty(growable: true)` and `<T>[]` after
    // the CFE -- is an empty list. With a length it would be `n` nulls,
    // which for a non-nullable element has nothing to fill with; that one
    // still stops in the backend.
    if (owner == '_GrowableList' &&
        target.name.text.isEmpty &&
        positional.length == 1 &&
        positional.single is IntLiteral &&
        (positional.single as IntLiteral).value == 0) {
      return IrListLiteral(
        const [],
        _type(node.arguments.types.singleOrNull ?? const DynamicType()),
      );
    }
    if ((owner == '_GrowableList' || owner == '_List') &&
        target.name.text.startsWith('_literal')) {
      // Each element widens into the element type, and a local named as an
      // element is cloned (`[left, right]` moved `left`).
      final element = node.arguments.types.singleOrNull;
      // ..and into an `Object?` element (`Object.hashAll([isChecked, ..])`
      // over enums and structs) each is shared, as an argument would be.
      return IrListLiteral([
        for (final e in positional) listElement(e, element),
      ], _type(element ?? const DynamicType()));
    }
    // The rest of `dart:math`'s functions are methods on `f64` in Rust,
    // spelled almost the same. `log` was refused as a top-level nothing
    // declared, and took `ClampingScrollSimulation._kDecelerationRate` and
    // everything reading it with it.
    const unary = {
      'log': 'ln',
      'exp': 'exp',
      'sqrt': 'sqrt',
      'sin': 'sin',
      'cos': 'cos',
      'tan': 'tan',
      'asin': 'asin',
      'acos': 'acos',
      'atan': 'atan',
    };
    // `dart:math` takes `num`s; Rust's are methods of `f64`, so an `int`
    // argument (`log(10)`, `pow(10, n)`) is cast first.
    IrExpr asDouble(Expression e) {
      final t = _staticType(e);
      final lowered = expression(e);
      // A literal has no static type without a context (`log(10)` in a
      // static's initialiser), and is an `int` by its spelling.
      return e is IntLiteral ||
              (t is InterfaceType && t.classNode.name == 'int')
          ? _toF64(lowered)
          : lowered;
    }

    if ('${target.enclosingLibrary.importUri}' == 'dart:math') {
      final method = unary[target.name.text];
      if (method != null && positional.length == 1) {
        return IrCall(asDouble(positional[0]), method, const []);
      }
      if (target.name.text == 'atan2' && positional.length == 2) {
        return IrCall(asDouble(positional[0]), 'atan2', [
          asDouble(positional[1]),
        ]);
      }
    }
    if (target.name.text == 'pow' && positional.length == 2) {
      return IrCall(asDouble(positional[0]), 'powf', [asDouble(positional[1])]);
    }
    if (target.name.text == 'clampDouble' && positional.length == 3) {
      return IrCall(expression(positional[0]), 'clamp', [
        expression(positional[1]),
        expression(positional[2]),
      ]);
    }
    if (target.name.text == 'unsafeCast' && positional.length == 1) {
      // The CFE's own cast, inserted where it has already proved the type. It
      // does nothing at runtime in Dart; here a cast from a trait object to
      // the struct it proved -- `unsafeCast<_NativePath>(path)` in front of
      // every native taking one -- is the downcast through `Any`.
      final from = _staticType(positional.single);
      final to = node.arguments.types.singleOrNull;
      final fromTraitObject =
          from is DynamicType ||
          (from is InterfaceType &&
              from.nullability != Nullability.nullable &&
              (_abstractLike(from.classNode) ||
                  from.classNode.name == 'Object'));
      if (fromTraitObject &&
          to is InterfaceType &&
          to.nullability != Nullability.nullable &&
          !_abstractLike(to.classNode) &&
          to.classNode.name != 'Object' &&
          (from is! InterfaceType || from.classNode != to.classNode)) {
        // The Rust name: `double` is an `f64` (`arg is double` after TFA).
        final toIr = _type(to);
        return IrCall(
          IrDowncast(
            expression(positional.single),
            rustScalar(toIr.name),
            arguments: toIr.arguments,
          ),
          'clone',
          const [],
        );
      }
      return expression(positional.single);
    }
    // `LinkedHashMap(equals: .., hashCode: ..)` as a factory: the prelude's
    // `Map`, the custom equality dropped (see `_construct`).
    if (const {
          'HashMap',
          'LinkedHashMap',
          'LinkedHashSet',
          'HashSet',
        }.contains(owner) &&
        target.name.text.isEmpty &&
        target.kind == ProcedureKind.Factory) {
      // ..with its type arguments: `HashSet<T>()` into an `Object` slot
      // has nothing else to say what `Set::new()` holds (run560).
      return IrNew(
        IrType(
          owner!.contains('Set') ? 'Set' : 'Map',
          arguments: [for (final t in node.arguments.types) _type(t)],
        ),
        const [],
      );
    }
    // `String.fromCharCodes(codes)`: a free function of the prelude's, since
    // Rust's `String` takes no inherent additions.
    if (owner == 'String' &&
        target.name.text == 'fromCharCodes' &&
        positional.length >= 1) {
      // `Uint8List` is a `Vec<u8>` and the prelude's takes a `Vec<i64>`:
      // the widening every other `List<int>` slot gets
      // (`_widensNarrowElements`), which a hand-written prelude call has
      // to ask for itself (`Digest._hexEncode` builds its char codes in a
      // `Uint8List`, ws945).
      var codes = expression(positional[0]);
      final held = _staticType(positional[0]);
      final narrow = held is InterfaceType ? _narrowElement(held) : null;
      if (narrow != null && narrow != 'f32' && narrow != 'f64') {
        codes = IrCall(codes, '!widen', const [])
          ..rustType = IrType('List', arguments: [const IrType('int')]);
      }
      return IrStaticCall(null, 'string_from_char_codes', [codes]);
    }
    // ..and `String.fromCharCode(code)`, one rune (`Icon.build` spells the
    // glyph of an `IconData.codePoint`, run695).
    if (owner == 'String' &&
        target.name.text == 'fromCharCode' &&
        positional.length == 1) {
      return IrStaticCall(null, 'string_from_char_code', [
        expression(positional[0]),
      ]);
    }
    // `scheduleMicrotask(f)`: the prelude's `_schedule_microtask` takes the
    // `Rc<dyn Fn()>` a translated closure is; the public-named one is the
    // prelude's own `Box<dyn FnOnce()>` entry.
    // A `dart:collection` extension getter on an iterable (`xs.lastOrNull`,
    // lowered by the CFE to `IterableExtensions|get#lastOrNull<T>(xs)`):
    // the prelude's method on the receiver, so that the receiver is
    // borrowed as any list method's is (`FocusScopeNode.focusedChild`,
    // run650). The table is the mapping.
    final coreExtension =
        _coreExtensionMethods[target.enclosingLibrary.importUri
            .toString()]?[target.name.text];
    if (coreExtension != null && owner == null && positional.isNotEmpty) {
      final args = _arguments(node.arguments, target.function);
      final element = node.arguments.types.isNotEmpty
          ? _type(node.arguments.types.first)
          : const IrType('dynamic');
      return IrCall(args.first, coreExtension, args.sublist(1))
        ..rustType = IrType(
          element.name,
          nullable: true,
          arguments: element.arguments,
        );
    }
    // `future.onError<E>(handle, test: ..)` -- `dart:async`'s extension on
    // `Future`, which the CFE lowers to `FutureExtensions|onError(future,
    // handle, test: ..)`. It is `catchError` with the error type as part
    // of the test, so at an `Object` `E` the prelude's `catch_error` is
    // the whole of it (`AssetImage.obtainKey`, run725). A narrower `E`
    // needs its `is` in the test and is refused rather than dropped.
    if (target.name.text == 'FutureExtensions|onError' &&
        target.enclosingLibrary.importUri.toString() == 'dart:async' &&
        owner == null &&
        positional.length >= 2) {
      final e = node.arguments.types.length > 1
          ? node.arguments.types[1]
          : null;
      final everyError =
          e is DynamicType ||
          (e is InterfaceType &&
              e.classNode.name == 'Object' &&
              e.classNode.enclosingLibrary.importUri.toString() == 'dart:core');
      if (!everyError) {
        throw Unsupported(
          '`Future.onError` with an error type of its own',
          _sample(node),
        );
      }
      // With the extension's own parameters put in: the handler returns
      // `FutureOr<T>`, and lowered against the declaration it spelled a
      // `T` nothing here declares.
      final args = _arguments(
        node.arguments,
        target.function,
        true,
        _instantiated(node),
      );
      final value = node.arguments.types.isNotEmpty
          ? _type(node.arguments.types.first)
          : const IrType('dynamic');
      return IrCall(args.first, 'catch_error', args.sublist(1))
        ..rustType = IrType('Future', arguments: [value]);
    }
    final coreFunction =
        _coreTopLevel[target.enclosingLibrary.importUri
            .toString()]?[target.name.text];
    if (coreFunction != null && owner == null) {
      final (fn, slots) = coreFunction;
      final args = _arguments(node.arguments, target.function);
      // A prelude callee's slots are not widened into by `_widened` (its
      // generics take the value as it is); the table's are spelled here.
      if (slots != null) {
        for (var i = 0; i < args.length && i < slots.length; i++) {
          args[i] = coerce(args[i], slots[i]);
        }
      }
      return IrStaticCall(null, fn, args);
    }
    if (target.name.text == 'identical' && positional.length == 2) {
      // `identical(x, null)` is `x == null`: the null test the value's
      // representation answers (`IrIsNull`), not a comparison against a
      // `None` -- an `Object?` list element is an `Rc<dyn Object>` holding
      // the `Null` object (`_CompressedNode.put`'s `identical(keyOrNull,
      // null)`, run538).
      if (_isNull(positional[1])) return IrIsNull(expression(positional[0]));
      if (_isNull(positional[0])) return IrIsNull(expression(positional[1]));
      return IrIdentical(expression(positional[0]), expression(positional[1]));
    }
    if (owner == null) {
      // A top-level function, this library's or another's. Which of those it
      // is no longer decides anything here: whether the callee exists in the
      // output is a whole-crate question, and the crate is not known until
      // every library has been lowered, so the backend asks it instead. The
      // analyzer front end never made the distinction, so this is also one
      // fewer place the two of them could differ.
      // The same cleaning `_lowerTopLevel` gives the declaration: an
      // extension member's `Ext|get#name` has to be one identifier at both
      // ends, and the crate-wide "does the callee exist" check compares them.
      return IrStaticCall(
        null,
        _topLevelName(target.name.text),
        _withGenericArgs(
          declaration,
          node.arguments,
          () => _arguments(
            node.arguments,
            declaration,
            true,
            _instantiated(node, declaration),
          ),
        ),
        fails: _fails(target),
        diverges: _diverges(target),
        asyncFn: _asyncMember(target),
        typeArguments: _keptTypeArguments(declaration, node.arguments),
        module: _topLevelModule(target),
      );
    }
    // `Future<T>.value()` with no value: no argument, rather than the
    // omitted optional filled in as `None` -- the prelude's `future_none`
    // makes the `null` of `T` (`()` for `void`), which a `None` is not
    // once the slot is the projected `T?` (+4 at ws570).
    if (owner == 'Future' &&
        target.name.text == 'value' &&
        node.arguments.positional.isEmpty) {
      // Spelled even though the callee is the prelude's -- `_keptTypeArguments`
      // gives a prelude callee none -- because `future_none`'s `T` has
      // nothing else to infer it from: `Future<void>.value()` in an `async`
      // body left `!` to the never-type fallback (5 at ws757).
      List<IrType> spelled() {
        try {
          return [for (final t in node.arguments.types) _typeNested(t)];
        } on Unsupported {
          return const [];
        }
      }

      return IrStaticCall(owner, 'value', const [], typeArguments: spelled());
    }
    return IrStaticCall(
      owner,
      // An unnamed factory -- `factory Vector3(x, y, z)` -- has no name in
      // Kernel at all. The backend spells an empty static name `new`, for a
      // prelude class as much as a translated one, and `_lowerProcedure`
      // declares the factory under that name.
      target.name.text,
      // With the call's type arguments: `WidgetStateProperty.resolveWith<
      // Color?>((states) { .. })` expects the closure to return `Color?`,
      // and the declared `T` said nothing (63 `Option<Color>` <- `Color`).
      _withGenericArgs(
        declaration,
        node.arguments,
        () => _arguments(
          node.arguments,
          declaration,
          true,
          _instantiated(node, declaration),
        ),
      ),
      fails: _fails(target),
      diverges: _diverges(target),
      asyncFn: _asyncMember(target),
      // The prelude's `Future` constructors are generic functions with
      // nothing but the type argument to say what `T` is when no value
      // is handed in (`Future<void>.delayed(Duration.zero)` fell back to
      // the never type, the thenvoid fixture): the class's arguments,
      // spelled.
      typeArguments: owner == 'Future'
          ? _recordedTypes(node.arguments.types)
          : _keptTypeArguments(declaration, node.arguments),
      module: owner == null ? _topLevelModule(target) : null,
    );
  }

  /// `types` spelled, or none when one cannot be.
  List<IrType> _recordedTypes(List<DartType> types) {
    try {
      return [for (final t in types) _type(t)];
    } on Unsupported {
      return const [];
    }
  }

  /// dart:core members the prelude implements as a *sibling* declares
  /// them. Dart's `List<E>.from(Iterable elements)` takes anything and
  /// casts element by element; the prelude copies, as `List.of(Iterable<
  /// E>)` does, and so its slot is `of`'s: coerced into the declared
  /// `Iterable<dynamic>`, the argument was upcast on the way in and nothing
  /// cast it back (`Vec<Hct> <= Vec<Rc<dyn Object>>`, 11 at ws424).
  static const preludeSiblings = {
    'List.from': 'of',
    'Set.from': 'of',
    'Map.from': 'of',
    'HashSet.from': 'of',
    'LinkedHashSet.from': 'of',
    'HashMap.from': 'of',
    'LinkedHashMap.from': 'of',
    // `List.unmodifiable(Iterable)` / `Map.unmodifiable(Map)`: copies,
    // as `of`. `Set.removeAll(Iterable<Object?>)` and its siblings take
    // the set's own elements here, as `addAll(Iterable<E>)` does (ws580).
    'List.unmodifiable': 'of',
    'Map.unmodifiable': 'of',
    'Set.removeAll': 'addAll',
    'Set.retainAll': 'addAll',
    'Set.containsAll': 'addAll',
  };

  Procedure _preludeDeclaration(Procedure target) {
    final owner = target.enclosingClass;
    if (owner == null || _translatedClass(owner)) return target;
    final sibling = preludeSiblings['${owner.name}.${target.name.text}'];
    if (sibling == null) return target;
    for (final p in owner.procedures) {
      // ..of the same kind: a factory's sibling is a factory, an instance
      // method's (`Set.removeAll` -> `addAll`) an instance method.
      if (p.isStatic == target.isStatic &&
          p.name.text == sibling &&
          p.function.positionalParameters.length ==
              target.function.positionalParameters.length) {
        return p;
      }
    }
    return target;
  }

  /// A translated generic callee's type arguments for the type parameters
  /// it keeps (an erased one is its bound and has no slot), spelled as a
  /// turbofish; nothing for a prelude callee, whose Rust signature is its
  /// own, or when one cannot be spelled.
  ///
  /// As type arguments (`_typeNested`), like a class's (`_erasedArguments`):
  /// a `T?` put in for the callee's `R` is the slot `<T as DartNullable>::
  /// Or`, so `makeBox<T?>(..)` returns the same `Box<<T as DartNullable>::
  /// Or>` the local declaring it is spelled with, and the closure it takes
  /// -- whose parameter is that same `T?` -- has the callee's parameter
  /// type. Spelled `Option<T>` the two disagreed (`showMenu<T?>(..).then`
  /// in `_PopupMenuButtonState.showButtonMenu`, ws690).
  List<IrType> _keptTypeArguments(FunctionNode fn, Arguments arguments) {
    final parameters = fn.typeParameters;
    if (parameters.isEmpty || arguments.types.length != parameters.length) {
      return const [];
    }
    if (!_calleeTranslated(fn, null)) return const [];
    try {
      return [
        for (var i = 0; i < parameters.length; i++)
          if (!_erasedParameter(parameters[i])) _typeNested(arguments.types[i]),
      ];
    } on Unsupported {
      return const [];
    }
  }

  /// Arguments in the callee's declaration order.
  ///
  /// Kernel has already split them into positional and named, and a named one
  /// that was omitted is simply absent -- so the callee's own parameter list is
  /// still what decides the order, exactly as in the analyzer front end.
  List<IrExpr> _arguments(
    Arguments node, [
    FunctionNode? callee,
    bool borrows = true,
    FunctionType? instantiated,
    List<DartType>? positionalTypes,
    Map<String, DartType>? namedTypes,
    List<IrType?>? positionalSlots,
  ]) {
    final was = _borrowedArgument;
    _borrowedArgument = borrows;
    try {
      final out = _argumentList(
        node,
        callee,
        instantiated,
        positionalTypes,
        namedTypes,
        positionalSlots,
      );
      // The hidden `Type` arguments a generic method takes (`_typeValues`):
      // each observed type argument as a value -- a `Type::of("X")`, or the
      // enclosing method's own hidden parameter when the argument is its
      // type parameter (`_findModels<T>` calling `getElement..<T>`).
      final target = callee?.parent;
      if (target is Procedure) {
        for (final i in _typeValues(target)) {
          out.add(
            _typeLiteral(
              i < node.types.length ? node.types[i] : const DynamicType(),
            ),
          );
        }
      }
      return out;
    } finally {
      _borrowedArgument = was;
    }
  }

  /// One argument, lowered knowing what the callee does with it.
  ///
  /// A closure may borrow only if the callee is *finished with it* when it
  /// returns. Round 59 asked the weaker question -- "is this an argument" --
  /// and `bin/census_escapes.dart` measured what that costs: of 1234 closures
  /// handed to a call, 394 are kept by the callee. `addListener`,
  /// `scheduleMicrotask`, `Timer`, `WidgetStateProperty.resolveWith`: storing
  /// one needs `'static`, and a borrow cannot give it. Those go back to being
  /// refused, which is the truth about them until objects are counted.
  /// `lower()`, with the slot's owner known (`_slotTranslated`).
  /// Whether the slot being widened into is a callee's parameter (an
  /// edge, spelled projected for a bare `T?`) rather than a body's own.
  bool _argumentEdge = false;

  T _asArgument<T>(T Function() widen) {
    final was = _argumentEdge;
    _argumentEdge = true;
    try {
      return widen();
    } finally {
      _argumentEdge = was;
    }
  }

  IrExpr _forCallee(
    FunctionNode? callee,
    DartType? declared,
    IrExpr lowered,
    IrExpr Function(IrExpr lowered) widen,
  ) {
    final was = _slotTranslated;
    final wasPrelude = _slotPrelude;
    _slotTranslated = _calleeTranslated(callee, declared);
    _slotPrelude = !_translatedCallee(callee) && callee != null;
    try {
      return widen(lowered);
    } finally {
      _slotTranslated = was;
      _slotPrelude = wasPrelude;
    }
  }

  /// Whether the slot being filled is a prelude callee's. Its function
  /// parameters are its own Rust signatures (`first_where_or` takes the
  /// element, not Dart's `T?`), and a closure handed to one is not adapted
  /// by the declared Dart type (132 new at ws419).
  var _slotPrelude = false;

  /// The prelude members that write into a list argument, by class and
  /// name: the positional indices handed out as `&mut` (see `IrMutRef`).
  static const _outBufferArguments = <String, Map<String, Set<int>>>{
    'RandomAccessFile': {
      'readInto': {0},
      'readIntoSync': {0},
    },
  };

  IrExpr _argument(
    Expression value,
    FunctionNode? callee,
    int index, [
    FunctionType? instantiated,
    IrType? slotIr,
    DartType? declaredOverride,
  ]) {
    final param = callee != null && index < callee.positionalParameters.length
        ? callee.positionalParameters[index]
        : null;
    // The *instantiated* parameter type when the call site has one: a
    // `List<Shadow>.add(E)` takes a `Shadow`, and the `E` alone could
    // widen nothing (`Shadow <= Option<Shadow>` after TFA dropped a `!`).
    // ..but the *declared* one when it is a function type naming an erased
    // parameter: the instantiated `bool Function(ScrollNotification)` is
    // not what the slot holds, `bool Function(T)` erased is.
    // A super call fills the mixin's declared slots, with this class's
    // arguments put in (`declaredOverride`, see the super-call lowering).
    final declaredType = declaredOverride ?? param?.type;
    // ..or any declared type naming one: `Entry<S>(v)` with `Entry.T`
    // erased holds an `Rc<dyn Object>`, not the `S` the call put in (the
    // outparam fixture).
    final paramType =
        (declaredOverride == null
            ? _landingSlot(callee: callee, index: index)
            : null) ??
        (declaredType != null &&
                (_mentionsErased(declaredType) || _atBound(declaredType))
            ? declaredType
            : instantiated != null &&
                  index < instantiated.positionalParameters.length
            ? instantiated.positionalParameters[index]
            : declaredType);
    // `DART2RUST_TRACE_ARG=<callee name>`: the slot each argument lands
    // in, to stderr.
    final calleeMember = callee?.parent;
    // A prelude call that *fills* its argument takes the place as `&mut`
    // (`IrMutRef`), never a copy: the table names them.
    if (calleeMember is Member) {
      final outs =
          _outBufferArguments[calleeMember.enclosingClass?.name]?[calleeMember
              .name
              .text];
      if (outs != null && outs.contains(index)) {
        return IrMutRef(expression(value));
      }
      // ..and a translated callee that fills the slot (`_fillsParameter`)
      // takes the caller's place the same way.
      // ..converted into the slot's own type first where it differs
      // (`List<ContainerLayer>` lent to a `List<ContainerLayer?>`,
      // `FollowerLayer._pathsToCommonAncestor`): a lent temporary, filled
      // and dropped -- what the copy did before -- rather than a type
      // error.
      if (calleeMember is Procedure && _fillsParameter(calleeMember, index)) {
        final place = expression(value);
        IrType? slotType;
        try {
          slotType = paramType == null ? null : _type(paramType);
        } on Unsupported {
          slotType = null;
        }
        // The place itself when the types agree: `_widened` clones a
        // local on its way into a slot, and the fill went into the clone.
        if (slotType == null ||
            place.rustType == null ||
            '${place.rustType}' == '$slotType') {
          return IrMutRef(place);
        }
        return IrMutRef(
          _widened(
            value,
            paramType,
            place,
            slotIr:
                slotIr ??
                _landingSlotIr(callee: callee, index: index) ??
                _topBound(declaredType, paramType) ??
                _genericSlotIr(callee, declaredType) ??
                _constructedSlotIr(callee, declaredType),
          ),
        );
      }
    }
    final tracedArg = Platform.environment['DART2RUST_TRACE_ARG'];
    if (calleeMember is Member &&
        (tracedArg == '*' ||
            tracedArg ==
                (calleeMember.enclosingClass?.name ??
                    calleeMember.name.text))) {
      stderr.writeln(
        'TRACE_ARG ${calleeMember.enclosingClass?.name}.${calleeMember.name.text}[$index] in=${_member?.enclosingClass?.name}.${_member?.name.text} '
        'declared=$declaredType instantiated=${instantiated?.positionalParameters} '
        'param=$paramType slotIr=$slotIr landing=${_landingSlotIr(callee: callee, index: index)} generic=${_genericSlotIr(callee, declaredType)}',
      );
    }
    final argument = _numLiteral(
      value,
      paramType,
      callee,
      _forCallee(
        callee,
        declaredType,
        _withBorrowing(
          param,
          callee,
          () => _withExpectedReturn(
            _instantiatedSlot(callee, paramType),
            value,
            () => expression(value),
          ),
        ),
        (lowered) => _asArgument(
          () => _widened(
            value,
            paramType,
            lowered,
            // A `T?` slot with `T` bound to a top type is the `Option<Rc<dyn
            // Object>>` the callee holds (`_topBound`; `DiagnosticsProperty<
            // Object?>(value: ..)`, ws499).
            slotIr:
                slotIr ??
                _landingSlotIr(callee: callee, index: index) ??
                _topBound(declaredType, paramType) ??
                _genericSlotIr(callee, declaredType) ??
                _constructedSlotIr(callee, declaredType),
          ),
        ),
      ),
    );
    if (calleeMember is Member &&
        (tracedArg == '*' ||
            tracedArg ==
                (calleeMember.enclosingClass?.name ??
                    calleeMember.name.text))) {
      stderr.writeln(
        'TRACE_ARG_OUT ${calleeMember.enclosingClass?.name}.${calleeMember.name.text}[$index] '
        '${argument.runtimeType} type=${argument.rustType}',
      );
    }
    // Into a projected slot of a translated callee: the spelled `T?`.
    return _translatedCallee(callee)
        ? _acrossBinding(
            argument,
            declaredType,
            _argumentBinding(callee, declaredType),
            toOption: false,
          )
        : argument;
  }

  /// A closure literal handed to a function-typed parameter returns what
  /// the *parameter's* type says: `String? Function(String)` taking
  /// `(l) => "default"` returns `Some("default")`. The closure's own
  /// return type is what it wrote, not what it is for (5 in intl).
  /// A function-typed slot with the callee's own instantiation put in.
  ///
  /// A generic class's constructor parameter still names the class's `T`
  /// (`OpenContainerBuilder<T>` is `Widget Function(BuildContext, void
  /// Function([T?]))`), and a closure written for it declared a parameter
  /// no name here stands for -- 4 `cannot find type T` at ws761, each one
  /// a `build` that then did not compile. The erased parameters stay as
  /// they are: those slots hold the bound, not what the call put in.
  DartType? _instantiatedSlot(FunctionNode? callee, DartType? param) {
    if (param is! FunctionType || callee == null) return param;
    final Map<TypeParameter, DartType> kept;
    if (identical(callee, _constructedCallee) && _constructedArgs.isNotEmpty) {
      kept = _constructedArgs;
    } else if (identical(callee, _genericCallee) && _genericArgs.isNotEmpty) {
      kept = _genericArgs;
    } else {
      return param;
    }
    final map = {
      for (final e in kept.entries)
        if (!_erasedParameter(e.key)) e.key: e.value,
    };
    if (map.isEmpty) return param;
    try {
      return Substitution.fromMap(map).substituteType(param);
    } on Object {
      return param;
    }
  }

  IrExpr _withExpectedReturn(
    DartType? param,
    Expression value,
    IrExpr Function() lower,
  ) {
    // Through the cast the CFE wraps a closure argument in (`(chunk) =>
    // ..` as `void Function(List<int>)?` for `listen`): the closure under
    // it is what the expected type is for.
    var closure = value;
    while (closure is AsExpression) closure = closure.operand;
    if (param is! FunctionType || closure is! FunctionExpression)
      return lower();
    final was = _expectedReturn;
    final wasFunction = _expectedFunction;
    _expectedReturn = param.returnType;
    _expectedFunction = param;
    try {
      return lower();
    } finally {
      _expectedReturn = was;
      _expectedFunction = wasFunction;
    }
  }

  /// The function type the next lowered closure is expected to have: its
  /// parameters stand in for a closure's own `dynamic` ones. `(locale) =>
  /// ..` in a `List<String Function(String)>` literal is inferred with a
  /// `dynamic` parameter by the CFE, and the `Rc<dyn Fn(String) -> String>`
  /// the list holds does not take an `Rc<dyn Object>`.
  FunctionType? _expectedFunction;

  /// The return type the next lowered body should widen into, if a
  /// parameter's function type says so.
  DartType? _expectedReturn;

  /// An int literal into a parameter a *translated* callee declares `num`
  /// (an `f64` here) is cast. Not a `dart:` callee's: `int.+(num other)` is
  /// declared that way and its `num` is not an `f64` (ws54).
  /// `e as f64`, once: a value already an `f64` -- `coerce` cast it on
  /// the way into the operand slot -- is left alone (`((1 as f64) as f64)`,
  /// 430 at ws356).
  static IrExpr _toF64(IrExpr e) {
    // Each arm of a conditional, not the whole of it: `n > 3 ? 30.0 * s : 0`
    // is a `double` in Dart, and `if .. { 30.0 * s } else { 0 } as f64`
    // is two types Rust will not unify before the cast is reached
    // (`ExpandingBottomSheet._mobileWidthFor` and two more, ws867).
    if (e is IrConditional) {
      final then = _toF64(e.then);
      final otherwise = _toF64(e.otherwise);
      if (identical(then, e.then) && identical(otherwise, e.otherwise)) {
        return e;
      }
      return IrConditional(e.condition, then, otherwise)
        ..rustType = const IrType('double');
    }
    if (e.rustType?.name == 'double') return e;
    if (e is IrCast && e.rust == 'f64') return e;
    // Inside the `Some` the widening already put on: the cast belongs to
    // the value, not to the `Option` (`(Some(0) as f64)`, ws779).
    if (e is IrSome) {
      return IrSome(_toF64(e.value))
        ..rustType = const IrType('double', nullable: true);
    }
    // An integer *literal* is written as a float rather than cast: an
    // unsuffixed literal under `as f64` is an `i32` to Rust, and
    // `1000000000000000000 as f64` does not fit one (`NumberFormat.
    // _numberOfIntegerDigits`, 4 at ws777).
    if (e is IrLiteral && e.type.name == 'int') {
      // Suffixed: two unsuffixed float literals make an *ambiguous*
      // `{float}`, and a method on that does not resolve
      // (`(1000000.0 / 60.0).round()`, E0689 at ws779).
      return IrLiteral('${e.value}.0_f64', const IrType('double'))
        ..rustType = const IrType('double');
    }
    return IrCast(e, 'f64')..rustType = const IrType('double');
  }

  IrExpr _numLiteral(
    Expression value,
    DartType? param,
    FunctionNode? callee,
    IrExpr lowered,
  ) {
    if (param is! InterfaceType) return lowered;
    final slot = param.classNode.name;
    if (slot != 'num' && slot != 'double') return lowered;
    // Already an `f64` -- `coerce` cast it (`((1 as f64) as f64)`, ws356).
    if (lowered.rustType?.name == 'double') return lowered;
    // An integer *literal* where a `double` goes is a double, whoever
    // declares the slot: that is Dart's rule about the literal, not about
    // the callee (`lerpDouble(split, 1, transformed)` in `Split.transform`,
    // ws763). A `num` slot keeps the callee test below, where an `int` is
    // still an `int` unless the callee's `num` is this output's `f64`.
    if (slot == 'double') {
      // A literal, whichever way it arrives: type flow analysis turns one
      // into a `ConstantExpression`, and testing only for `IntLiteral`
      // missed every `lerpDouble(a, 0, t)` in the shape code (9 at ws779).
      final literal =
          value is IntLiteral ||
          (value is ConstantExpression && value.constant is IntConstant);
      if (!literal) return lowered;
      return _toF64(lowered)..rustType = const IrType('double');
    }
    // A literal, or a value whose static type is `int` (a translated
    // callee's `num` is an `f64`, so either is cast).
    final given = _staticType(value);
    final isInt =
        value is IntLiteral ||
        (given is InterfaceType &&
            given.classNode.name == 'int' &&
            given.nullability != Nullability.nullable);
    if (!isInt) return lowered;
    final member = callee?.parent;
    if (member is! Member) return lowered;
    // A *translated* callee's `num` is an `f64` here, whichever library
    // declares it: `lerpDouble(a, 0, t)` lives in `dart:ui` and its body is
    // translated, so its `num?` slots really are `Option<f64>` and an `int`
    // there does not fit. Only a callee the prelude answers keeps its `num`
    // as it was (9 at ws779).
    if (!_translatedCallee(callee)) return lowered;
    return _toF64(lowered)..rustType = const IrType('double');
  }

  /// Whether a declared type is a type parameter this compiler spells at
  /// its bound (`_spelledAsBound`): `T extends Iterable<E>` is written
  /// `Rc<dyn DartIterable<E>>` in the callee's Rust signature whatever the
  /// call site put in for `T`, so the *declared* type is the slot and the
  /// instantiation is not. Said here beside the erased case, which is the
  /// same shape of answer -- "the declaration knows better than the
  /// substitution" (`collection`'s `_UnorderedEquality<E, T extends
  /// Iterable<E>>` took a `Set<E>` where its own signature says the
  /// handle; the iterablebound fixture).
  bool _atBound(DartType t) {
    if (t is! TypeParameterType) return false;
    // Only an `Iterable` bound. `_spelledAsBound` also answers for
    // `String`, `int`, `double`, `bool` and `List`, and for those the
    // instantiation and the spelling agree in kind, so taking the
    // declaration there changes slots that were right -- the render tree
    // came back as two nodes the round that did (run922).
    final bound = t.parameter.bound;
    return bound is InterfaceType &&
        bound.classNode.name == 'Iterable' &&
        bound.classNode.enclosingLibrary.importUri.scheme == 'dart';
  }

  IrExpr _namedArgument(
    Expression value,
    Object param, [
    DartType? declaredOverride,
  ]) {
    final callee = _calleeOf(param);
    final declared =
        declaredOverride ?? (param is FunctionParameter ? param.type : null);
    final type =
        (declaredOverride == null
            ? _landingSlot(
                callee: callee,
                name: param is FunctionParameter ? param.parameterName : null,
              )
            : null) ??
        declared;
    final tracedNamed = Platform.environment['DART2RUST_TRACE_NAMED'];
    final argument = _numLiteral(
      value,
      type,
      callee,
      _forCallee(
        callee,
        declared,
        _withBorrowing(
          param,
          callee,
          () => _withExpectedReturn(
            _instantiatedSlot(callee, type),
            value,
            () => expression(value),
          ),
        ),
        (lowered) {
          if (tracedNamed != null &&
              param is FunctionParameter &&
              param.parameterName == tracedNamed) {
            stderr.writeln(
              'TRACE_NAMED ${param.parameterName} declared=$declared type=$type '
              'lowered=${lowered.runtimeType} rust=${lowered.rustType} '
              'slotIr=${_constructedSlotIr(callee, declared)} '
              'translated=${_calleeTranslated(callee, declared)}',
            );
          }
          return _widened(
            value,
            type,
            lowered,
            slotIr:
                _landingSlotIr(
                  callee: callee,
                  name: param is FunctionParameter ? param.parameterName : null,
                ) ??
                _topBound(declared, _argumentBinding(callee, declared)) ??
                _genericSlotIr(callee, declared) ??
                _constructedSlotIr(callee, declared),
          );
        },
      ),
    );
    return _translatedCallee(callee)
        ? _acrossBinding(
            argument,
            declared,
            _argumentBinding(callee, declared),
            toOption: false,
          )
        : argument;
  }
}
