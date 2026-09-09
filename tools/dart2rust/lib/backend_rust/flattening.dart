part of '../backend_rust.dart';

augment class RustBackend {
  // -- Flattening the hierarchy -----------------------------------------------

  /// This class's fields, with its bases' in front of them.
  ///
  /// Rust has no inheritance, so a subclass's struct has to carry what the base
  /// declared. Round five turned an abstract class into a trait and reported its
  /// fields as untranslated; this is that bill coming due, because 80% of the
  /// 1888 `super(...)` calls in package:flutter have an abstract base.
  ///
  /// Base first, in declaration order, so the layout reads the way upstream's
  /// class hierarchy does.
  List<IrFieldDecl> _allFields(
    IrClass of, [
    Map<String, IrType> bound = const {},
  ]) {
    final own = [
      for (final f in of.fields)
        IrFieldDecl.substituted(
          f,
          (t) => _substituteType(t, bound),
          initial: bound.isEmpty
              ? null
              : (e) => _substitute(e, const {}, bound),
        ),
      // A mixin's fields arrive with the class's own: the front end lowers
      // the anonymous application classes' members into it (ws112). Adding
      // them here as well declared `PointerEvent`'s fields twice (845
      // struct errors the round the gesture crates became reachable).
    ];
    final base = library[of.superclass];
    if (base == null) return own;
    // The base's type parameters, bound to what this class passed it.
    // `ErrorDescription extends DiagnosticsProperty<String>` inherits a
    // `T? _value`, and copying it in unsubstituted left a field of type `T` in
    // a struct with no `T` -- 32 `cannot find type T`, and every one of them a
    // field this compiler had claimed to translate.
    final passed = of.superclassArguments;
    // `class _DialogRoute extends PopupRoute` -- no arguments written -- is
    // `PopupRoute<dynamic>` in Dart. Leaving the base's `T` unbound copied
    // `Option<T>` fields into a struct with no `T`: 25 `cannot find type T`.
    final next = <String, IrType>{
      if (passed.length == base.typeParameters.length)
        for (var i = 0; i < passed.length; i++)
          base.typeParameters[i]: _substituteType(passed[i], bound)
      else if (passed.isEmpty)
        for (final p in base.typeParameters) p: const IrType('dynamic'),
    };
    // A subclass may redeclare a field the base already has -- Dart lets it
    // shadow -- and one struct cannot hold two `_color`s (13 `E0124`s). The
    // nearer declaration is the one the class's own code names, so it wins.
    final inherited = _allFields(base, next);
    final ownNames = {for (final f in own) f.name};
    return [
      for (final f in inherited)
        if (!ownNames.contains(f.name)) f,
      ...own,
    ];
  }

  /// `T` -> whatever `T` was bound to, inside a type and its arguments.
  IrType _substituteType(IrType t, Map<String, IrType> bound) {
    // A function type keeps its parameters and result beside `arguments`,
    // not in it: `FormFieldBuilder<T>` copied into `TextFormField` kept its
    // `T` -- 25 `cannot find type T`, every one inside a `dyn Fn(..)`.
    final params = t.parameters;
    if (params != null) {
      return IrType.function(
        [for (final p in params) _substituteType(p, bound)],
        _substituteType(t.returns!, bound),
        nullable: t.nullable,
      );
    }
    if (t.arguments.isEmpty) {
      final to = bound[t.name];
      if (to == null) return t;
      // `T?` with `T` bound to `dynamic` is `dynamic`: Dart's `dynamic`
      // already admits null, and spelling the `?` again made the impl say
      // `Option<Rc<dyn Object>>` where the trait, bound the same way, said
      // `Rc<dyn Object>` -- `decodeMessage` on every codec, 16 `E0053`s.
      // ..but a *projected* `T?` stays projected over `dynamic` too: the
      // trait's side normalises `<Rc<dyn Object> as DartNullable>::Or` to
      // `Option<Rc<dyn Object>>`, and the impl has to say the same.
      if (to.name == 'dynamic' && !t.projected) return to;
      // The `?` belongs to the *use*, not to what is put in its place:
      // `ChildType? _child` with `ChildType` bound to `RenderBox` is a
      // `RenderBox?`, and dropping the question mark made the accessor return
      // a `Box<dyn RenderBox>` where the trait it implements wants an
      // `Option<Box<dyn RenderBox>>` -- 575 `E0053`s.
      // `T?` where `T` is *already* nullable collapses, as it does in Dart:
      // `bool??` is `bool?`. Rust does not collapse -- with
      // `RestorableValue<bool?>` the trait's `Option<T>` is
      // `Option<Option<bool>>`, and its two `None`s are distinguishable in a
      // way Dart cannot express -- so 14 members come out with a type the
      // trait will not accept.
      //
      // Refusing them instead was tried and measured *worse*: an accessor's
      // refusal takes the whole `impl` with it, and the slice went 273 errors
      // to 278. Saying it properly means the IR carrying nullability as
      // something richer than a flag, which it does not. Until then these 14
      // stay visible and explained rather than turned into a cascade.
      if (!t.nullable) return to;
      // ..and now it does collapse, as Dart does: a signature's `T?` is
      // `<T as DartNullable>::Or` (`IrType.projected`), which *is* `X?`
      // for `T = X?`. Put in for another parameter it stays projected.
      // A projected slot stays projected over what is put in: an impl's
      // signature has to spell the trait's `<X as DartNullable>::Or`.
      // ..unless what is put in is nullable itself: then the projection
      // *is* that `Option`, and rustc normalises the trait's side to it.
      if (to.nullable) return to;
      if (t.projected) {
        return IrType(
          to.name,
          nullable: true,
          arguments: to.arguments,
          projected: true,
        );
      }
      return IrType(to.name, nullable: true, arguments: to.arguments);
    }
    return IrType(
      t.name,
      nullable: t.nullable,
      arguments: [for (final a in t.arguments) _substituteType(a, bound)],
    );
  }

  /// The field initialisers a `super(...)` stands for.
  ///
  /// The base's own constructor is *inlined*: its parameters are replaced by
  /// the arguments the super call passed, and its field initialisers become
  /// this constructor's. That is what flattening means once it reaches storage,
  /// and it recurses, since a base may call `super` in turn -- the chains go six
  /// deep in places.
  /// A thrown value, boxed into the function's error type when that is
  /// `Object` and the value is a string -- the tree shaker's "code removed
  /// by TFA" throws, in statement and in expression position alike.
  /// A throw, spelled for where it is. Inside a failing method it is the
  /// `Err` the signature promised. Anywhere else -- a method a trait
  /// declares (whose signature is the trait's, one for every class), a
  /// static, a top-level function, a closure -- there is no `Result` to
  /// carry it, and it is a panic: Dart's exception, uncaught on this side.
  /// Not a quiet wrong answer; a loud one, at the site. 50 of ws32's 361
  /// errors were `Result`s meeting signatures that never said so.
  String _thrown(IrExpr value) => _failure == null
      ? 'panic!("uncaught Dart exception: {:?}", ${expr(value)})'
      : 'return Err(${_boxedThrow(value)})';

  /// The error type a method's signature carries, if any: see `_thrown`.
  String? _failureOf(IrMethod method) => _resultModel ? _error : null;

  String _boxedThrow(IrExpr value) {
    final thrown = expr(value);
    // ..and any constructed error: a `FormatException` thrown where the
    // signature says `Rc<dyn Object>` is boxed into it too.
    // ..and a prelude exception made by its constructor function
    // (`Exception::new(..)` is a static call, not an `IrNew`; 29 at ws325).
    const preludeExceptions = _preludeClasses;
    final boxed =
        (_failure == 'Object' || _failure == 'std::rc::Rc<dyn Object>') &&
        ((value is IrLiteral && value.type.name == 'String') ||
            value is IrNew ||
            value is IrConstInstance ||
            (value is IrStaticCall &&
                value.owner != null &&
                preludeExceptions.contains(value.owner)));
    // ..spelled as the error type: inside an `async` block the `Err` has
    // no signature to infer from, and `Rc<Exception>` was the block's
    // whole error type (`initServiceExtensions`, ws479).
    final asError =
        _failure == 'Object' || _failure == 'std::rc::Rc<dyn Object>';
    if (boxed) {
      // Through `dart_boxed`, not a bare `Rc::new`: the handle is the same
      // one either way, and the registration behind it is what lets the
      // error *print*. Without it every uncaught exception read `Instance
      // of 'StateError'` -- six of the gallery's `RenderErrorBox`es and
      // nothing to say why (run745).
      return asError ? 'dart_boxed($thrown)' : 'std::rc::Rc::new($thrown)';
    }
    final valueType = value.rustType;
    if (asError &&
        valueType != null &&
        valueType.name != 'Object' &&
        valueType.name != 'dynamic' &&
        !valueType.nullable &&
        !valueType.isFunction &&
        (library[valueType.name] != null ||
            _preludeClasses.contains(valueType.name))) {
      // ..behind a handle where the class is spelled by value: an `Rc`
      // around the struct, since only a handle unsizes to `dyn Object`.
      final held = library[valueType.name];
      final counted = held?.counted ?? false;
      final abstract = library.isAbstract(valueType.name);
      final handle = counted || abstract
          ? _handleOf(value)
          : 'dart_object(${_handleOf(value)})';
      return '($handle as std::rc::Rc<dyn Object>)';
    }
    return thrown;
  }

  /// Free functions the prelude provides, which the front end calls by name
  /// and no library declares: the crate-wide "was it translated" check has
  /// to know them, or `vec_of_nones(..)` reads as a call to nothing.
  static const _preludeFunctions = {
    'dart_null_object',
    'dart_function_object',
    'dart_call_function',
    'dart_function_same',
    'dart_from_dynamic',
    'dart_double_str',
    'dart_object_str',
    'dart_type_of',
    'dart_shl',
    'dart_identical',
    'dart_boxed',
    'dart_option_object',
    'future_or_value',
    'future_or_future',
    'dart_shr',
    'dart_ushr',
    'vec_of_nulls',
    // `math.max`/`math.min` as values (`_mathValueNames`).
    'dart_max_of',
    'dart_min_of',
    'dart_native',
    'dart_native_as',
    'future_ready',
    'dart_cast_erased',
    'dart_is_kind',
    'dart_is_type',
    'dart_as_own',
    'uint8_list_sublist_view',
    'byte_data_sublist_view',
    // By their Dart names, as the call names them (`postEvent`, not the
    // `post_event` it is spelled as).
    'exit',
    'dart_null_as',
    'dart_null_check_failed',
    'postEvent',
    'registerExtension',
    'EnumName_get_name',
    // `package:collection`'s `.indexed`, and `dart:async`'s `unawaited`.
    'IterableExtensions_get_indexed',
    'unawaited',
    // `dart:convert`'s `jsonEncode`, beside the `jsonDecode` next to it.
    'jsonEncode',
    // `dart:io`'s `stdout`/`stdin`, which are getters the CFE lowers to a
    // call.
    'stdout',
    'stdin',
    'never',
    'new_object',
    'string_from_char_codes',
    'string_from_char_code',
    'vec_of_nones',
    'dart_iter',
    'dart_iterator_map',
    'post_event',
    '_print',
    '_print_debug',
    '_schedule_microtask',
    'dart_print',
    'json_decode',
    'object_hash_all',
    '_invoke1_with_return',
    '_get_callback_handle',
    '_get_callback_from_handle',
    'object_hash',
    'dart_str',
    'log',
    'parse_int',
    'try_parse_int',
    // ..and the two that take Dart's radix (`int.parse(s, radix: r)`).
    'parse_int_radix',
    'try_parse_int_radix',
    'parse_double',
    'try_parse_double',
    'schedule_microtask',
    'uint8_list_view',
    // `dart:ffi`'s `_abi()`, which every `#sizeOf`/`#offsetOf` the CFE
    // writes is indexed by.
    '_abi',
  };

  /// The prelude's classes that `is` can ask about and a `throw` boxes.
  /// The prelude's generic classes, by their Dart name and the Rust one
  /// `runtimeType` reports: `is` asks that name rather than downcasting to
  /// the single instantiation a value happened to be boxed as. The blanket
  /// `runtime_type` spells the *struct*, so `Future` is `DartFuture`.
  static const _preludeGenerics = {
    'Future': 'DartFuture',
    'Completer': 'Completer',
    'StreamSubscription': 'StreamSubscription',
    'Converter': 'Converter',
    'Expando': 'Expando',
  };

  /// `const X()` of a prelude class, where the prelude has the value it
  /// names. Only for a constant with no fields: a constant that carries
  /// some is a different object, and the shapes have to agree.
  static const _preludeConstInstances = {'Stream': 'Stream::empty()'};

  static const _preludeClasses = {
    // The prelude's plain value classes: `x is DateTime` in a date
    // picker's `_buildDayItem`, `x is ByteData` in the message codecs.
    'DateTime',
    'ByteData',
    'Exception',
    'FormatException',
    'StateError',
    'ArgumentError',
    'RangeError',
    'UnsupportedError',
    'UnimplementedError',
    'ConcurrentModificationError',
    'TypeError',
    'AssertionError',
    'Error',
  };

  static const _typedLists = {
    'Float32List',
    'Float64List',
    'Int8List',
    'Int16List',
    'Int32List',
    'Int64List',
    'Uint8List',
    'Uint16List',
    'Uint32List',
    'Uint64List',
    'Uint8ClampedList',
  };

  /// The statements a `super(...)` chain runs before its fields are set --
  /// the temporaries the CFE binds in a base's initialiser list -- with the
  /// base's parameters replaced by what this constructor passed, exactly as
  /// `_inheritedInits` does for the field initialisers. Without them a base
  /// field init named a `__t0` this constructor never bound.
  /// What a class puts in for its base's type parameters, from the
  /// `superclassArguments` of the class the chain started at, composed
  /// down the chain.
  Map<String, IrType> _baseTypes(IrClass from, Map<String, IrType> types) {
    final baseName = from.superclass;
    final base = baseName == null ? null : library[baseName];
    if (base == null) return const {};
    return {
      for (
        var i = 0;
        i < base.typeParameters.length && i < from.superclassArguments.length;
        i++
      )
        base.typeParameters[i]: _substituteType(
          from.superclassArguments[i],
          types,
        ),
    };
  }

  /// The bodies a constructor runs on the way in: its base's chain, from
  /// the deepest base down to the direct one, each with the constructor it
  /// reaches and the arguments the `super(..)` passes. A generic base is
  /// left out: its body names the base's `T`, which this constructor
  /// cannot (it stays what it was: not run).
  List<(IrClass, IrConstructor, List<IrExpr>)> _inheritedBodies(
    IrConstructor ctor, [
    IrClass? from,
  ]) {
    final baseName = ctor.superBase;
    if (baseName == null) return const [];
    final base = library[baseName];
    if (base == null) return const [];
    final baseCtors = base.constructors
        .where((c) => c.name == ctor.superName)
        .toList();
    if (baseCtors.length != 1) return const [];
    final baseCtor = baseCtors.single;
    if (baseCtor.params.length != ctor.superArgs.length) return const [];
    // A *generic* base's own body names the base's `T`, which this
    // constructor cannot -- but a bodiless one names nothing, and the walk
    // goes on through it to the bases above. `RenderObject()`'s body sets
    // `late bool _needsCompositing`, and the bodiless
    // `_RenderPhysicalModelBase<T>` in between stopped the walk: every
    // `RenderPhysicalModel` read that `late` unset (run733).
    if (base.typeParameters.isNotEmpty && baseCtor.body != null) {
      // ..unless what this class puts in for them is those same names: the
      // struct beside an open class's trait carries the class's parameters
      // unchanged (`SeqImpl<T>` over `Seq<T>`), so the base's body already
      // spells only what can be named here. Without this `TweenSequence`'s
      // constructor -- the one that fills `_intervals` -- never ran, and the
      // first frame threw "could not find an interval for 0.0" (run763).
      final passed = _baseTypes(from ?? cls, const {});
      final sameNames = base.typeParameters.every((p) {
        final put = passed[p];
        return put != null &&
            put.name == p &&
            put.arguments.isEmpty &&
            !put.nullable;
      });
      if (!sameNames) return const [];
    }
    // A bodiless base too: its parameters are what the next base's
    // arguments name (`RenderProxyBoxWithHitTestBehavior({child}) :
    // super(child)`, whose `child` was never bound, ws523).
    return [
      ..._inheritedBodies(baseCtor, base),
      (base, baseCtor, ctor.superArgs),
    ];
  }

  List<IrStmt> _inheritedPre(
    IrConstructor ctor, [
    IrClass? from,
    Map<String, IrType> types = const {},
  ]) {
    final baseName = ctor.superBase;
    if (baseName == null) return const [];
    final base = library[baseName];
    if (base == null) return const [];
    final here = from ?? cls;
    final binding = _baseTypes(here, types);
    final baseCtors = base.constructors
        .where((c) => c.name == ctor.superName)
        .toList();
    if (baseCtors.length != 1) return const [];
    final baseCtor = baseCtors.single;
    if (baseCtor.params.length != ctor.superArgs.length) return const [];
    final substitution = <String, IrExpr>{
      for (var i = 0; i < baseCtor.params.length; i++)
        baseCtor.params[i].name: ctor.superArgs[i],
      ..._baseTempRenames(base, baseCtor),
    };
    return [
      ..._inheritedPre(baseCtor, base, binding),
      for (final s in baseCtor.pre)
        if (s is IrLocalDecl)
          IrLocalDecl(
            _baseTempName(base, s.name),
            // A parameter's binding is typed by the parameter: `let
            // configuration = None` inferred nothing (E0282, ws461).
            s.type ??
                baseCtor.params
                    .where((p) => p.name == s.name)
                    .map((p) => _substituteType(p.type, binding))
                    .firstOrNull,
            s.init == null ? null : _substitute(s.init!, substitution, binding),
          )
        else
          s,
    ];
  }

  /// A base constructor's temporaries, renamed for the constructor they are
  /// inlined into. Each library numbers its own `__tN`, so a base in another
  /// library and the subclass both have a `__t0` -- and the subclass passes
  /// its `__t0` as the super argument the base's `__t0` is computed from:
  /// `let __t0 = __t0.to_int()`, 9 times.
  static String _baseTempName(IrClass base, String name) =>
      '${snakeRaw(base.name)}_$name';

  static Map<String, IrExpr> _baseTempRenames(
    IrClass base,
    IrConstructor ctor,
  ) => {
    for (final s in ctor.pre)
      if (s is IrLocalDecl) s.name: IrLocal(_baseTempName(base, s.name)),
  };

  Map<String, IrExpr> _inheritedInits(
    IrConstructor ctor, [
    IrClass? from,
    Map<String, IrType> types = const {},
  ]) {
    final baseName = ctor.superBase;
    if (baseName == null) return const {};
    final base = library[baseName];
    final binding = _baseTypes(from ?? cls, types);
    if (base == null) {
      throw Unsupported(
        'super constructor call into `$baseName`, which is not in this file',
        'super(...)',
      );
    }
    final baseCtors = base.constructors
        .where((c) => c.name == ctor.superName)
        .toList();
    if (baseCtors.length != 1) {
      throw Unsupported(
        'super constructor call into `$baseName`, which has '
            '${baseCtors.length} unnamed constructors',
        'super(...)',
      );
    }
    final baseCtor = baseCtors.single;
    if (baseCtor.params.length != ctor.superArgs.length) {
      throw Unsupported(
        'super(...) passes ${ctor.superArgs.length} arguments to a '
            'constructor taking ${baseCtor.params.length}',
        'super(...)',
      );
    }
    var substitution = <String, IrExpr>{
      for (var i = 0; i < baseCtor.params.length; i++)
        baseCtor.params[i].name: ctor.superArgs[i],
      ..._baseTempRenames(base, baseCtor),
    };
    // Through the base's redirects: `_SemanticsBase()` is `this.
    // fromProperties(.., properties: SemanticsProperties(..))`, and the
    // field initialisers are the target's, in terms of its parameters,
    // which the redirect's arguments fill (`Semantics.new`, 119 callers,
    // "field never initialised: properties").
    var reached = baseCtor;
    while (reached.redirectTo != null) {
      final targetName = reached.redirectTo!.isEmpty
          ? null
          : reached.redirectTo;
      final targets = base.constructors
          .where((c) => c.name == targetName)
          .toList();
      if (targets.length != 1 ||
          targets.single.params.length != reached.redirectArgs.length) {
        throw Unsupported(
          'super constructor call into `$baseName`, whose constructor '
              'redirects to one this compiler cannot follow',
          'super(...)',
        );
      }
      final target = targets.single;
      final through = substitution;
      substitution = {
        for (var i = 0; i < target.params.length; i++)
          target.params[i].name: _substitute(reached.redirectArgs[i], through),
        ..._baseTempRenames(base, target),
      };
      reached = target;
    }
    final reachedSubstitution = substitution;
    return {
      // The base's own inherited initialisers first, so a chain resolves from
      // the top down and a nearer class can override nothing -- Dart does not
      // let it, and neither does this.
      ..._inheritedInits(reached, base, binding).map(
        (k, v) => MapEntry(k, _substitute(v, reachedSubstitution, binding)),
      ),
      ...reached.fieldInits.map(
        (k, v) => MapEntry(k, _substitute(v, reachedSubstitution, binding)),
      ),
    };
  }

  /// Replaces references to a constructor's parameters with the expressions a
  /// `super(...)` passed for them.
  IrExpr _substitute(
    IrExpr e,
    Map<String, IrExpr> by, [
    Map<String, IrType> types = const {},
  ]) {
    IrExpr go(IrExpr node) => _substitute(node, by, types);
    return switch (e) {
      IrLocal(:final name) => by[name] ?? e,
      IrField(:final target, :final name, :final onEnum, :final owner) =>
        IrField(
          target == null ? null : go(target),
          name,
          onEnum: onEnum,
          owner: owner,
        ),
      IrBinary(:final op, :final left, :final right, :final type) => IrBinary(
        op,
        go(left),
        go(right),
        type: type,
      ),
      IrUnary(:final op, :final operand) => IrUnary(op, go(operand)),
      IrNullCheck(:final operand) => IrNullCheck(go(operand)),
      IrCastTo(:final target, :final type) => IrCastTo(
        go(target),
        _substituteType(type, types),
      ),
      IrSuperDispatch(
        :final receiver,
        :final base,
        :final name,
        :final args,
        :final typeArguments,
        :final classArity,
        :final castTo,
      ) =>
        IrSuperDispatch(
          go(receiver),
          base,
          name,
          args.map(go).toList(),
          typeArguments,
          classArity,
          castTo: castTo,
        ),
      IrDowncast(:final target, :final type, :final arguments) => IrDowncast(
        go(target),
        type,
        arguments: [for (final a in arguments) _substituteType(a, types)],
      ),
      IrDynamicDispatch(:final receiver, :final arms) => IrDynamicDispatch(
        go(receiver),
        [for (final (t, b) in arms) (t, go(b))],
      ),
      IrSome(:final value) => IrSome(go(value)),
      // An edge conversion inlined from a base (`_inheritedInits`): the
      // base's `T` is this class's own parameter, renamed, or a concrete
      // type -- and for one of those the conversion is the identity.
      // ..or this class's own projected `T?` (`EnumBox<T> extends
      // Box<T?>`): the slot stays `<T as DartNullable>::Or`, and so does
      // the crossing -- dropping it put a bare `None` into it (ws688).
      IrNullableOf(:final value, :final parameter, :final toOption) =>
        switch (types[parameter]) {
          null => IrNullableOf(go(value), parameter, toOption: toOption),
          final to
              when to.arguments.isEmpty &&
                  (!to.nullable || to.projected) &&
                  cls.typeParameters.contains(to.name) =>
            IrNullableOf(go(value), to.name, toOption: toOption),
          _ => go(value),
        },
      IrCast(:final value, :final rust) => IrCast(go(value), rust),
      IrIsNull(:final operand) => IrIsNull(go(operand)),
      IrIfNull(
        :final left,
        :final right,
        :final nullableResult,
        :final eager,
      ) =>
        IrIfNull(
          go(left),
          go(right),
          nullableResult: nullableResult,
          eager: eager,
        ),
      IrNullAware(:final receiver, :final body, :final flatten) => IrNullAware(
        go(receiver),
        go(body),
        flatten: flatten,
      ),
      IrCall(
        :final target,
        :final name,
        :final args,
        :final qualifier,
        :final receiverClass,
        :final fails,
        :final diverges,
        :final asyncFn,
        :final asyncTarget,
        :final typeArguments,
      ) =>
        IrCall(
          target == null ? null : go(target),
          name,
          args.map(go).toList(),
          qualifier: qualifier,
          receiverClass: receiverClass,
          fails: fails,
          diverges: diverges,
          asyncFn: asyncFn,
          asyncTarget: asyncTarget,
          typeArguments: typeArguments,
        ),
      IrStaticCall(
        :final owner,
        :final name,
        :final args,
        :final fails,
        :final diverges,
        :final asyncFn,
        :final typeArguments,
        :final module,
      ) =>
        IrStaticCall(
          owner,
          name,
          args.map(go).toList(),
          fails: fails,
          diverges: diverges,
          asyncFn: asyncFn,
          typeArguments: typeArguments,
          module: module,
        ),
      IrNew(:final type, :final args, :final constructor) => IrNew(
        type,
        args.map(go).toList(),
        constructor: constructor,
      ),
      IrConditional(:final condition, :final then, :final otherwise) =>
        IrConditional(go(condition), go(then), go(otherwise)),
      IrSuperCall(
        :final base,
        :final name,
        :final args,
        :final isSetter,
        :final baseArguments,
        :final typeArguments,
      ) =>
        IrSuperCall(
          base,
          name,
          args.map(go).toList(),
          isSetter: isSetter,
          baseArguments: baseArguments,
          typeArguments: typeArguments,
        ),
      IrAwait(:final operand) => IrAwait(go(operand)),
      IrMutRef(:final place) => IrMutRef(go(place)),
      IrUpcast(:final value, :final type, :final handle, :final explicit) =>
        IrUpcast(go(value), type, handle: handle, explicit: explicit),
      IrMapElements(:final collection, :final kind, :final body) =>
        IrMapElements(go(collection), kind, go(body)),
      IrIs(:final expr, :final type, :final negated) => IrIs(
        go(expr),
        type,
        negated: negated,
      ),
      IrCallValue(:final target, :final args) => IrCallValue(
        go(target),
        args.map(go).toList(),
      ),
      // The statements too, all of them: a base constructor's `errorPalette
      // ?? TonalPalette.of(..)` is `let __t = error_palette; ..`, and the
      // parameter it names is the subclass's super argument (9). A `switch`
      // among them names one as well -- `_scrollCacheExtent = switch
      // (cacheExtentStyle) {..}` in `RenderViewportBase`, whose
      // `cacheExtentStyle` `RenderShrinkWrappingViewport` does not forward
      // and left unbound (run696).
      IrBlockValue(:final statements, :final value) => IrBlockValue([
        for (final s in statements) _substituteStmt(s, by, types),
      ], go(value)),
      IrConstInstance(:final type, :final fields) => IrConstInstance(type, {
        for (final entry in fields.entries) entry.key: go(entry.value),
      }),
      IrIdentical(:final left, :final right) => IrIdentical(
        go(left),
        go(right),
      ),
      IrThrowValue(:final value) => IrThrowValue(go(value)),
      IrInterpolation(:final parts) => IrInterpolation(parts.map(go).toList()),
      IrIndex(:final target, :final index) => IrIndex(go(target), go(index)),
      IrIterChain(:final source, :final steps) => IrIterChain(go(source), [
        for (final step in steps) (step.$1, go(step.$2)),
      ]),
      IrListLiteral(:final elements, :final element) => IrListLiteral(
        elements.map(go).toList(),
        element,
      ),
      IrRecord(:final fields) => IrRecord(fields.map(go).toList()),
      IrRecordField(:final record, :final index) => IrRecordField(
        go(record),
        index,
      ),
      IrMapLiteral(:final entries, :final key, :final value) => IrMapLiteral(
        [for (final entry in entries) (go(entry.$1), go(entry.$2))],
        key,
        value,
      ),
      IrFunctionRef() => e,
      IrAssignValue(:final name, :final value) => IrAssignValue(
        name,
        go(value),
      ),
      IrSetValue(:final target, :final name, :final value) => IrSetValue(
        target == null ? null : go(target),
        name,
        go(value),
      ),
      IrClosure() ||
      IrLiteral() ||
      IrStatic() ||
      IrTopLevel() ||
      IrThis() ||
      IrBound() => e,
    };
  }

  /// `_substitute` through a statement: a base constructor's initialiser is
  /// inlined into the subclass's, and every parameter it names -- wherever
  /// in it -- is the argument the `super(..)` passed. A statement left
  /// unwalked kept the base's own parameter name, which is not in scope
  /// when the subclass does not forward it.
  IrStmt _substituteStmt(
    IrStmt s,
    Map<String, IrExpr> by, [
    Map<String, IrType> types = const {},
  ]) {
    IrExpr go(IrExpr e) => _substitute(e, by, types);
    IrStmt at(IrStmt inner) => _substituteStmt(inner, by, types);
    return switch (s) {
      IrReturn(:final value) => IrReturn(value == null ? null : go(value)),
      IrLocalDecl(:final name, :final type, :final init, :final cell) =>
        IrLocalDecl(name, type, init == null ? null : go(init), cell: cell),
      IrIf(:final condition, :final then, :final otherwise) => IrIf(
        go(condition),
        at(then),
        otherwise == null ? null : at(otherwise),
      ),
      IrBlock(:final statements) => IrBlock([
        for (final inner in statements) at(inner),
      ]),
      IrExprStmt(:final expr) => IrExprStmt(go(expr)),
      IrAssign(:final name, :final value) => IrAssign(name, go(value)),
      IrAssignStatic(:final owner, :final name, :final value) => IrAssignStatic(
        owner,
        name,
        go(value),
      ),
      IrAssignTopLevel(:final name, :final value) => IrAssignTopLevel(
        name,
        go(value),
      ),
      IrAssignField(:final name, :final value, :final target, :final owner) =>
        IrAssignField(
          name,
          go(value),
          target: target == null ? null : go(target),
          owner: owner,
        ),
      IrSetter(
        :final target,
        :final name,
        :final value,
        :final qualifier,
        :final receiverClass,
      ) =>
        IrSetter(
          target == null ? null : go(target),
          name,
          go(value),
          qualifier: qualifier,
          receiverClass: receiverClass,
        ),
      IrLabeled(:final label, :final body) => IrLabeled(label, at(body)),
      IrForIn(:final name, :final iterable, :final body) => IrForIn(
        name,
        go(iterable),
        at(body),
      ),
      IrIndexSet(:final target, :final index, :final value) => IrIndexSet(
        go(target),
        go(index),
        go(value),
      ),
      IrSwitch(:final value, :final cases, :final otherwise) => IrSwitch(
        go(value),
        [
          for (final c in cases)
            IrCase([for (final v in c.values) go(v)], at(c.body)),
        ],
        otherwise == null ? null : at(otherwise),
      ),
      IrWhile(:final condition, :final body, :final label) => IrWhile(
        go(condition),
        at(body),
        label: label,
      ),
      IrTryFinally(:final body, :final finalizer) => IrTryFinally(
        at(body),
        at(finalizer),
      ),
      IrTryCatch(
        :final body,
        :final error,
        :final handler,
        :final errorType,
        :final stack,
      ) =>
        IrTryCatch(
          at(body),
          error,
          at(handler),
          errorType: errorType,
          stack: stack,
        ),
      IrThrow(:final value) => IrThrow(go(value)),
      IrAssert(:final condition, :final literalMessage, :final message) =>
        IrAssert(
          go(condition),
          literalMessage: literalMessage,
          message: message,
        ),
      // A local function's body is a closure, which `_substitute` leaves as
      // it is: a closure's captures are its own.
      IrLocalFunction() || IrBreak() || IrContinue() => s,
    };
  }
}
