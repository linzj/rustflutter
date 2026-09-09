part of '../backend_rust.dart';

augment class RustBackend {
  // -- Failure in the return value --------------------------------------------

  /// Methods of this class whose Rust signature returns `Result`.
  ///
  /// Seeded with the ones that throw, then closed over calls, the same shape as
  /// `_mutating`. Measured before it was built: across `package:flutter` 717
  /// members throw directly and 5906 -- 20% of all members -- return `Result`
  /// once that has spread. Not "almost everything", which is what made the
  /// decision affordable.
  ///
  /// It stops at the class boundary here. A call into another class would carry
  /// the failure further, and 20% is the whole-program figure; what this
  /// computes is the part visible in one file. The rest waits for the compiler
  /// to see more than a file at a time, which is the same wall the stubs are at.
  late final Map<String, String> _failing = _computeFailing();

  /// Whether a method that throws carries a `Result` in its signature.
  ///
  /// Off since 2026-09-04. The propagation was never modular: a `Result`
  /// on a method is visible to callers on `this` (which add `?`) and to
  /// nobody else -- a getter read through another object, a trait's
  /// declaration, a closure, a static -- and every one of those was a
  /// type error at the caller (ws59: `Rc<Image> <= Result<Rc<Image>,
  /// StateError>` on `_image`, 15 such in dart:ui alone). Without it a
  /// `throw` is a panic (`_thrown`), except inside a `try` body, where the
  /// flow closure still turns it into the `Err` the handler catches. What
  /// is lost: an exception thrown *by a callee* inside a `try` panics
  /// instead of being caught. That is a loud loss, at the site, and the
  /// runtime will say so; the quiet one was a signature nobody could see.
  /// Every function returns `Result<T, _error>` (STATUS, 决定 2026-09-04,
  /// 修正): a Dart exception is an object, and one type for them all is
  /// what lets `?` propagate through every call form alike.
  static const _resultModel = true;
  static const _error = 'std::rc::Rc<dyn Object>';

  Map<String, String> _computeFailing() {
    // The uniform model needs no per-class fixed point: every method fails.
    if (_resultModel || !_resultModel) return const {};
    final failing = <String, String>{};
    final calls = <String, Set<String>>{};
    for (final method in cls.methods) {
      final key = _rustName(method);
      if (method.throws != null) failing[key] = method.throws!;
      final found = _WalkSelf();
      found.statement(method.body);
      calls[key] = found.selfCalls;
    }
    var changed = true;
    while (changed) {
      changed = false;
      for (final entry in calls.entries) {
        if (failing.containsKey(entry.key)) continue;
        for (final callee in entry.value) {
          // `_WalkSelf` records the Dart name; the keys are Rust names.
          // Compared raw, `setFromTranslationRotation` never matched
          // `set_from_translation_rotation`, and neither contagion --
          // this one nor `_computeMutating`'s -- ever crossed a camelCase
          // call. The 16 E0596s that survived every receiver rule were this.
          final error = failing[snake(callee)];
          if (error != null) {
            failing[entry.key] = error;
            changed = true;
            break;
          }
        }
      }
      // A method that throws its own type and calls one failing with
      // another cannot carry both in one `Result`: it carries `Object`, the
      // type every Dart throw already has (5 "couldn't convert the error").
      for (final entry in calls.entries) {
        final own = failing[entry.key];
        if (own == null || own == 'Object') continue;
        for (final callee in entry.value) {
          final other = failing[snake(callee)];
          if (other != null && other != own) {
            failing[entry.key] = 'Object';
            changed = true;
            break;
          }
        }
      }
    }
    return failing;
  }

  /// Whether a statement returns from the method it is written in.
  ///
  /// Not from a closure written inside it -- `IrClosure` is not descended into,
  /// for the same reason the front ends' version skips nested functions.
  bool _returnsEarly(IrStmt s) {
    var found = false;
    void walk(IrStmt s) {
      if (found) return;
      switch (s) {
        case IrReturn():
          found = true;
        case IrBlock(:final statements):
          statements.forEach(walk);
        case IrIf(:final then, :final otherwise):
          walk(then);
          if (otherwise != null) walk(otherwise);
        case IrTryCatch(:final body, :final handler):
          walk(body);
          walk(handler);
        case IrTryFinally(:final body, :final finalizer):
          walk(body);
          walk(finalizer);
        case IrWhile(:final body):
          walk(body);
        case IrForIn(:final body):
          walk(body);
        case IrLabeled(:final body):
          walk(body);
        case IrSwitch(:final cases, :final otherwise):
          for (final one in cases) {
            walk(one.body);
          }
          if (otherwise != null) walk(otherwise);
        default:
      }
    }

    walk(s);
    return found;
  }

  /// Whether every path through a statement leaves the method.
  ///
  /// Deliberately conservative: it says yes only where it can see that it must
  /// be so. Saying yes wrongly would emit an `unreachable!()` that is reached,
  /// which is a panic at runtime; saying no wrongly costs nothing but a `{}`
  /// arm the compiler then complains about, which is loud and cheap.
  bool _alwaysReturns(IrStmt s) => switch (s) {
    IrReturn() => true,
    IrThrow() => true,
    IrBlock(:final statements) => statements.any(_alwaysReturns),
    IrIf(:final then, :final otherwise) =>
      otherwise != null && _alwaysReturns(then) && _alwaysReturns(otherwise),
    IrSwitch(:final cases, :final otherwise) =>
      otherwise != null &&
          _alwaysReturns(otherwise) &&
          cases.every((c) => _alwaysReturns(c.body)),
    IrTryCatch(:final body, :final handler) =>
      _alwaysReturns(body) && _alwaysReturns(handler),
    IrTryFinally(:final body, :final finalizer) =>
      _alwaysReturns(body) || _alwaysReturns(finalizer),
    // A labelled block can be left by its `break`, so it does not count as
    // always returning even when its body would.
    IrLabeled() => false,
    _ => false,
  };

  /// The error type a statement can produce, taken from the failing methods of
  /// this class that it calls.
  String? _errorIn(IrStmt body) {
    final found = _WalkSelf();
    found.statement(body);
    for (final name in found.selfCalls) {
      if (_traitDeclares(name)) continue;
      final error = _failing[snake(name)];
      if (error != null) return error;
    }
    return null;
  }

  /// The Rust return type of the method currently being emitted, as written in
  /// its signature -- `Result<..>` and all. A `return` inside a try body has to
  /// carry a value of exactly this type out of the closure.
  String? _rustReturns;

  /// Set while emitting a try body that contains a `return`.
  ///
  /// Inside one, `return x` cannot be a Rust `return`: it would return from the
  /// closure, and the method would carry on. It becomes `Ok(Some(x))` instead,
  /// which the `match` outside turns back into a real return.
  bool _inFlowClosure = false;

  /// The error type of the method currently being emitted, if it can fail.
  String? _failure;

  /// A method's return type, wrapped when it can fail.
  String _returnType(IrMethod method) {
    final error = _failureOf(method);
    // A Rust `async fn` returning `T` already is a future, so the `Future<T>`
    // Dart declared is the wrapper, not the value: `Future<void> f() async`
    // is `async fn f()`, and writing the wrapper as well would make it a
    // future of a future.
    final declared = method.isAsync
        ? _awaited(method.returnType)
        : method.returnType;
    final value = method.isSetter
        ? '()'
        : type(declared) == 'std::convert::Infallible'
        ? '!'
        : type(declared);
    // The error type goes through `type()` like any other: an abstract one --
    // `Object` is the commonest, since a `throw` with no declared type lands
    // there -- is a trait, and a trait is not a type. `Box<dyn Object>` is.
    if (error == null) return value;
    // `Result<!, E>` is unstable; `Infallible` says the same.
    final inner = value == '!' ? 'std::convert::Infallible' : value;
    return 'Result<$inner, ${type(IrType(error))}>';
  }

  /// A boxed future in return position may borrow the receiver: `+ '_`.
  /// A trait's async method is `fn f(&self) -> Pin<Box<dyn Future<..> +
  /// '_>>`, the manual spelling of `async fn` in a trait, and the default
  /// that boxes `super_fn(self, ..)` needs exactly that lifetime.
  /// A return type as a signature spells it: `!` for `Never` (the general
  /// spelling `Infallible` is for value positions), and a boxed future's
  /// receiver lifetime.
  /// `Result<T, E>` around a rendered return type.
  /// The `DartFuture<T>` an `async` function returns: `T` the awaited
  /// type, `()` for a `void` one.
  String _futureOf(IrMethod method) =>
      'DartFuture<${type(_awaited(method.returnType))}>';

  /// An `async` function is emitted twice: its body as a private `async
  /// fn` (`name__body`, the lazy borrowing future Rust makes), and under
  /// its own name a plain function that clones the receiver's handle,
  /// moves the arguments, and spawns the body on the scheduler --
  /// Dart's future: eager, shared, `'static`, a value (`DartFuture`).
  /// `receiver` is the handle binding (`let __self = ..;`) and the
  /// argument that reaches the body, or null for a function with none.
  void _emitAsyncWrapper(
    IrMethod method,
    String signature,
    String bodyName, {
    (String, String)? receiver,
    String turbofish = '',
  }) {
    _line('$signature {');
    _indent++;
    if (receiver != null) _line(receiver.$1);
    final args = [
      if (receiver != null) receiver.$2,
      ...method.params.map((p) => snake(p.name)),
    ].join(', ');
    _line(
      'DartFuture::spawn_named("${cls.name}.${method.name}", std::boxed::Box::pin(async move { $bodyName$turbofish($args).await }))',
    );
    _indent--;
    _line('}');
  }

  static String _wrapped(String rendered) => !_resultModel
      ? rendered
      // `Result<!, E>`: the never type is unstable as a type argument, and
      // `Infallible` is the stable spelling of a value that cannot be.
      : 'Result<${rendered == '!' ? 'std::convert::Infallible' : rendered}, $_error>';

  static String _spelledReturn(String rendered) =>
      rendered == 'std::convert::Infallible' ? '!' : _lifetimed(rendered);

  static String _lifetimed(String rendered) {
    const prefix =
        'std::pin::Pin<std::boxed::Box<dyn std::future::Future<Output = ';
    if (!rendered.startsWith(prefix) || !rendered.endsWith('>>'))
      return rendered;
    return "${rendered.substring(0, rendered.length - 2)} + '_>>";
  }

  /// `Future<T>` -> `T`; anything else unchanged.
  /// `Future<T>` or `FutureOr<T>` -> `T` (Dart's `flatten`: an `async`
  /// function declared `FutureOr<void>` spawns a `Future<void>`, run607).
  static IrType _awaited(IrType t) =>
      (t.name == 'Future' || t.name == 'FutureOr') && t.arguments.length == 1
      ? t.arguments.single
      : t;

  String _param(IrParam p, {bool owned = true}) => p.mutRef
      // A parameter the callee fills: the caller's place, lent
      // (`IrParam.mutRef`).
      ? '${_reassigned.contains(p.name) ? "mut " : ""}${snake(p.name)}: &mut ${type(p.type, owned: true)}'
      : '${_reassigned.contains(p.name) ? "mut " : ""}'
            // A *function-typed* parameter the callee keeps is owned however it was
            // reached: a list of listeners cannot hold a borrow. Only function
            // types: "keeps it" is measured as "does more than call it", and for an
            // ordinary parameter that includes merely comparing it -- which made
            // `identical(this, other)` take its argument by value and stop being a
            // question about references at all.
            '${snake(p.name)}: '
            '${type(p.type, owned: owned || (p.kept && p.type.isFunction))}';

  String _params(IrMethod method) => [
    // A trait method is `&mut self` when any implementer writes a field in
    // it; see `_receiverOf`. This is the trait's own declaration.
    // Not the trait's own default body writing a field: that write goes
    // through the setter the trait declares (`set_x(&self, ..)`), since the
    // implementers of a writing trait are counted.
    if (!method.isStatic) _sharedMutation(method) ? '&mut self' : '&self',
    // A parameter is borrowed, not owned: passing a `Box<dyn Trait>` in
    // would move it, and upstream's callers do not give theirs away.
    ...method.params.map((p) => _param(p, owned: false)),
  ].join(', ');

  String _emitStruct() {
    _line('// Generated by tools/dart2rust from upstream `${cls.name}`.');
    _line('//');
    _line('// Translated, not ported: this is the compiler\'s output, not a');
    _line('// hand-written re-expression. See tools/dart2rust/README.md.');
    _line('');
    _doc(cls.doc);
    // `Copy` only when every field is. A `String` field is not, and deriving
    // it anyway does not compile -- which is loud, but the derive is this
    // compiler's own line and it should not write one it knows is wrong.
    // Asked of the *emitted* type: a shared field is an `Rc<Cell<..>>`, which
    // is not `Copy` however copyable the value inside it is. Asking the Dart
    // type instead derived `Copy` for a struct that cannot have it.
    final copyable =
        !cls.counted && _allFields(cls).every((f) => _isCopy(_fieldType(f)));
    // `Debug` and `PartialEq` cannot be derived over a function-typed field
    // (a `dyn Fn` is neither), and a struct holding one got 15 `E0369`s and
    // 14 `E0277`s for the derive alone. Left off there: a `==` on such a
    // class is then an error at the use, which says what it is.
    // Nested too: a `Vec<Option<Rc<dyn Fn()>>>` field cannot be printed.
    final printable = _allFields(cls).every(
      (f) =>
          !f.type.isFunction &&
          !_fieldType(f).contains('dyn Fn') &&
          // A boxed future prints and compares no better than a closure.
          !_fieldType(f).contains('dyn std::future::Future'),
    );
    // A trait-object field compares by identity, and a derived `PartialEq`
    // cannot say so: `self.f == other.f` on an `Rc<dyn Object>` moved the
    // right-hand side (E0507, rustc 1.98 -- reproduced on four lines). The
    // `impl` is written out below instead, field by field, with the
    // prelude's `dart_eq` on those.
    // ..and a counted class's handle: `Rc<DynamicColor>` compares by
    // identity too, which is what Dart says of two references.
    // The field *is* a handle -- `Rc<..>`, or an `Option`/`Vec` of one --
    // not a struct that merely holds one somewhere in its type arguments
    // (`MapEquality<K, V>` compares by value).
    final handle = RegExp(r'^(Option<|Vec<)*std::rc::Rc<');
    // A closure field too: `PointerData._onRespond` is an `Rc<dyn Fn>`,
    // which `DartEq` compares by address as Dart compares closures. Left
    // out, the struct had no `PartialEq` at all and nothing generic over
    // it could be called (`_invoke1<PointerDataPacket>`).
    // ..and a field that *holds* function values anywhere in its type: a
    // `Map<String, WidgetBuilder>` (`WidgetsApp.routes`) has no
    // `PartialEq` to derive from, because `Rc<dyn Fn>` has none, and Dart
    // compares the closures inside it by identity -- which is exactly what
    // `DartEq` says of them (3 at ws793).
    final byIdentity = _allFields(cls)
        .where(
          (f) =>
              f.type.isFunction ||
              handle.hasMatch(_fieldType(f)) ||
              _fieldType(f).contains('dyn Fn'),
        )
        .toList();
    // ..and every field's own class comparable, recursively: a
    // `VecDeque<_StoredMessage>` of a struct holding a closure derives
    // nothing (`==` cannot be applied, 3).
    // ..and not over a projected `T?` field: `<T as DartNullable>::Or:
    // PartialEq` is a where clause a derive cannot write.
    final comparable =
        printable &&
        byIdentity.isEmpty &&
        _allFields(cls).every((f) => !f.type.projected) &&
        _allFields(cls)
            .every((f) => _comparableType(_fieldType(f), {cls.name}));
    // A counted class without an `operator ==` of its own is Dart's
    // `Object.==`: identity. Field by field it walked the object graph --
    // `FocusManager` holds its root scope, which holds its manager -- and
    // overflowed the stack on the first `==` between two (run641).
    final identityEq =
        cls.counted && !cls.methods.any((m) => m.operator == '==');
    // A boxed future is not `Clone`, and a struct holding one (an
    // `AssetBundle`'s caches) cannot derive it; its handle, the `Rc` every
    // counted class is passed by, still is. A value class holding one
    // would have to be cloned by value somewhere and is left to say so.
    final cloneable = _cloneable(cls);
    // A struct with a projected `T?` field writes its `Clone` out below:
    // the derive cannot say `<T as DartNullable>::Or: Clone`, and a bound
    // on the struct's own parameters would have to be repeated by every
    // declaration naming it (`_FutureBuilderState<T>` holding an
    // `AsyncSnapshot<T>`, ws403).
    final projecting = _allFields(cls).any((f) => f.type.projected);
    // ..and every *generic* struct's, for the same reason one step removed:
    // a derive over a field holding a projecting struct (`Option<
    // DropdownMenuItem<T>>`) needs that struct's `Clone`, whose bound is
    // `<T as DartNullable>::Or: Clone` -- said on the impl, where nothing
    // has to repeat it.
    final writesClone =
        cloneable && (projecting || cls.typeParameters.isNotEmpty);
    // A derived `Debug` on `ValueKey<T>` holds only for `T: Debug`, and
    // the `Key` trait it implements has `Debug` above it for every `T:
    // Clone + DartNullable<Or: Clone> + 'static` (18 E0277s the moment the type parameters lost
    // their `Debug` bound). A generic class prints as its class instead;
    // `PartialEq` can still be derived, that impl carries its own `T:
    // PartialEq` and no trait asks for it unconditionally.
    final derivesDebug = printable && cls.typeParameters.isEmpty;
    final derives = [
      if (cloneable && !writesClone) 'Clone',
      if (copyable && !writesClone) 'Copy',
      if (derivesDebug) 'Debug',
      if (comparable && !identityEq) 'PartialEq',
    ];
    if (derives.isNotEmpty) _line('#[derive(${derives.join(', ')})]');
    // `'static` on the struct: an `Rc<dyn Equality<Option<E>>>` field needs
    // its `E` to outlive the trait object (8 E0310s in `collection`).
    _line(
      '${_vis(cls.name)}struct ${cls.name}${_generics(cls, static: true)} {',
    );
    _indent++;
    for (final field in _allFields(cls)) {
      _doc(field.doc);
      _line('${_vis(field.name)}${snake(field.name)}: ${_fieldType(field)},');
    }
    // A counted object knows its own handle (`DartSelf`): a trait body's
    // `this` is `self.dart_self_<trait>()`, 117 `&__Self` where an
    // `Rc<dyn X>` was wanted at ws271.
    // `pub`: a constant instance of the class is spelled as a struct
    // literal wherever it is used (`dart_rc(Struct {..})`), other modules
    // included (E0451 in `SemanticsService`, ws432).
    if (cls.counted) _line('pub __self: DartSelf<Self>,');
    // A Dart class can name a type parameter it never stores -- `Tween<T>`
    // holds `begin` and `end` of type `T?`, but plenty do not. Rust will not
    // have an unused parameter, and `PhantomData` is what it offers instead.
    for (final unused in _unusedParameters(cls)) {
      _line(
        'pub _phantom_${snake(unused)}: '
        'std::marker::PhantomData<$unused>,',
      );
    }
    _indent--;
    _line('}');
    _line('');
    if (writesClone) {
      _line(
        'impl${_implGenerics(cls, keyed: false)} Clone for ${cls.name}${_generics(cls)} {',
      );
      _indent++;
      final copied = [
        for (final f in _allFields(cls))
          '${snake(f.name)}: self.${snake(f.name)}.clone()',
        if (cls.counted) '__self: self.__self.clone()',
        for (final unused in _unusedParameters(cls))
          '_phantom_${snake(unused)}: std::marker::PhantomData',
      ];
      _line('fn clone(&self) -> Self { ${cls.name} { ${copied.join(', ')} } }');
      _indent--;
      _line('}');
      _line('');
      // `Copy` alongside, under the same bound: a derived one asks `Clone`
      // of every `T: Copy`, which the impl above does not give.
      if (copyable) {
        final generics = _implGenerics(
          cls,
          keyed: false,
        ).replaceAll("'static", "'static + Copy");
        _line('impl$generics Copy for ${cls.name}${_generics(cls)} {}');
        _line('');
      }
    }
    if (cls.counted) {
      _line(
        'impl${_implGenerics(cls, keyed: false)} DartSelfRef for ${cls.name}${_generics(cls)} {',
      );
      _indent++;
      _line('fn dart_self_ref(&self) -> &DartSelf<Self> { &self.__self }');
      _indent--;
      _line('}');
      _line('');
    }
    // A struct holding a closure still has to print -- `Rc<DynamicColor>`
    // in a struct that derives `Debug` -- so it prints as its class.
    if (!derivesDebug) {
      // The struct's own bounds, not the impl's: `_MapEntry` holds a
      // `MapEquality<Rc<dyn Object>, ..>` and derives `Debug` over it, and
      // `dyn Object` is no `Hash` -- the key bound the methods need is
      // theirs alone.
      _line(
        'impl${_generics(cls, static: true)} std::fmt::Debug for ${cls.name}${_generics(cls)} {',
      );
      _indent++;
      _line(
        "fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {",
      );
      _indent++;
      _line('write!(f, "Instance of \'${cls.name}\'")');
      _indent--;
      _line('}');
      _indent--;
      _line('}');
    }
    if (byIdentity.isNotEmpty || (identityEq && comparable)) {
      final projected = {
        for (final f in _allFields(cls))
          if (f.type.projected)
            type(IrType(f.type.name, arguments: f.type.arguments)),
      };
      final bounds = cls.typeParameters.isEmpty
          ? ''
          : ' where ${[for (final p in cls.typeParameters) '$p: PartialEq', for (final p in projected) '<$p as DartNullable>::Or: PartialEq'].join(', ')}';
      _line(
        'impl${_implGenerics(cls)} PartialEq for ${cls.name}${_generics(cls)}$bounds {',
      );
      _indent++;
      // By name: `_allFields` builds its list afresh each call, so the
      // `IrField`s are not the same objects (`source` came out as `==`).
      final identityNames = byIdentity.map((f) => f.name).toSet();
      final terms = [
        for (final f in _allFields(cls))
          identityNames.contains(f.name)
              ? 'self.${snake(f.name)}.dart_eq(&other.${snake(f.name)})'
              : 'self.${snake(f.name)} == other.${snake(f.name)}',
      ];
      _line(
        'fn eq(&self, other: &Self) -> bool { '
        '${identityEq
            ? 'std::ptr::eq(self, other)'
            : terms.isEmpty
            ? 'true'
            : terms.join(' && ')} }',
      );
      _indent--;
      _line('}');
      _line('');
    }

    // Constructors and constants in a block of their own when the methods
    // carry a key bound (`T: PartialEq`, `_implGenerics`): an
    // `ObserverList<Rc<dyn Fn()>>` can then be *made* wherever it is held,
    // and only the methods comparing its items are out of reach.
    final keyed = _implGenerics(cls);
    final unkeyed = _implGenerics(cls, keyed: false);
    _line('impl$unkeyed ${cls.name}${_generics(cls)} {');
    _indent++;
    _emitConstructors();
    if (!_freeStatics(cls.name)) _emitConstants();
    if (keyed != unkeyed) {
      _indent--;
      _line('}');
      _line('');
      _line('impl$keyed ${cls.name}${_generics(cls)} {');
      _indent++;
    }
    _emitMethods();
    _emitLazyAccessors();
    _emitToList();
    _indent--;
    _line('}');
    if (_freeStatics(cls.name)) {
      // A generic class's statics and constants at module level, named
      // with the class, as an abstract class's are (`_freeStatics`).
      _line('');
      _emitConstants(prefix: cls.name);
      for (final method in cls.methods) {
        if (!method.isStatic || method.operator != null) continue;
        _member(
          '${cls.name}.${method.name} (static)',
          () => _emitMethod(
            method,
            as: _abstractStaticName(cls.name, _methodName(method)),
          ),
        );
      }
    }
    // One line per struct rather than one blanket impl over everything: see
    // `DartAny` in the prelude for why the blanket one is quietly wrong.
    _line('');
    _emitDartNullable();
    // `DartEq`, by the `PartialEq` the struct has -- derived, or the
    // manual one above with its bounds -- and by identity when it has none.
    // A generic struct compares field by field through `DartEq`, which
    // every field type has (the `T: DartEq` bound is the struct's own); a
    // `T: PartialEq` bound shut out every `ObserverList<VoidCallback>`
    // (48 at ws445).
    String fieldWise() {
      final parts = [
        for (final f in _allFields(cls))
          'self.${snake(f.name)}.dart_eq(&other.${snake(f.name)})',
      ];
      return parts.isEmpty ? 'true' : parts.join(' && ');
    }

    if (identityEq) {
      _emitDartEq(body: 'std::ptr::eq(self, other)');
    } else if (comparable || byIdentity.isNotEmpty) {
      _emitDartEq(
        body: cls.typeParameters.isEmpty ? 'self == other' : fieldWise(),
      );
    } else {
      _emitDartEq(body: 'std::ptr::eq(self, other)');
    }
    _line(
      // The bounds the inherent impl has: `dart_cast` calls the trait
      // impls, whose `E: Clone` a bare `'static` cannot meet (ws304).
      'impl${_implGenerics(cls, keyed: false)} DartAny for '
      '${cls.name}${_generics(cls)} {',
    );
    _indent++;
    _line('fn dart_to_string(&self) -> String { ${_dartToStringBody()} }');
    _line(
      'fn dart_eq_any(&self, other: &dyn std::any::Any) -> bool { match other.downcast_ref::<Self>() { Some(o) => self.dart_eq(o), None => false } }',
    );
    _line('fn dart_hash_any(&self) -> i64 { self.dart_hash_code() }');
    _line('fn dart_runtime_type(&self) -> Type {');
    _indent++;
    // A generic class names its arguments, as Dart's `runtimeType` does:
    // `RenderAnnotatedRegion<SystemUiOverlayStyle>` is what the render
    // walk's reference prints, and the bare name was the one node of 708
    // that differed (run773).
    _line(
      cls.typeParameters.isEmpty
          ? 'Type::of("${cls.dartName ?? cls.name}")'
          : 'dart_type_applied("${cls.dartName ?? cls.name}", &['
                '${cls.typeParameters.map((p) => 'dart_type_of::<$p>()').join(', ')}])',
    );
    _indent--;
    _line('}');
    // What this object is (`dart_cast_to`): its own struct, and every
    // trait it has an impl for, each through the handle that impl keeps.
    _line(
      'fn dart_cast(&self, __t: std::any::TypeId) -> Option<std::boxed::Box<dyn std::any::Any>> {',
    );
    _indent++;
    final own = cls.counted
        ? 'self.dart_self_ref().get()'
        : cls.typeParameters.isEmpty && _cloneable(cls)
        ? 'std::rc::Rc::new(self.clone())'
        : null;
    // ..and for the handle type itself (`dart_cast_any`): a type parameter
    // is instantiated with `Rc<ScaffoldState>`, not `ScaffoldState`.
    if (own != null) {
      _line(
        'if __t == std::any::TypeId::of::<Self>() || __t == std::any::TypeId::of::<std::rc::Rc<Self>>() { return Some(std::boxed::Box::new($own)); }',
      );
    }
    // ..and as the `Object` it is, for `dart_boxed`: the one handle, not
    // a box around a handle.
    if (cls.counted) {
      _line(
        'if __t == std::any::TypeId::of::<dyn Object>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn Object>>() { return Some(std::boxed::Box::new(self.dart_self_ref().get() as std::rc::Rc<dyn Object>)); }',
      );
    } else if (own != null) {
      // A value struct behind a fresh handle: an erased twin's `as T` is
      // `dart_cast_any::<Rc<dyn Object>>()` (`found as T` through
      // `get__erased`, the gentrait fixture).
      _line(
        'if __t == std::any::TypeId::of::<dyn Object>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn Object>>() { return Some(std::boxed::Box::new($own as std::rc::Rc<dyn Object>)); }',
      );
    }
    for (final above in _abstractAncestors(cls)) {
      final arguments = _baseArguments(above);
      if (arguments == null) continue;
      final handle = cls.extraImpls.any((w) => w.name == above.name)
          ? '<Self as ${above.name}$arguments>::dart_self_${snakeRaw(above.name)}(self)'
          : 'self.dart_self_${snakeRaw(above.name)}()';
      _line(
        'if __t == std::any::TypeId::of::<dyn ${above.name}$arguments>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn ${above.name}$arguments>>() { return Some(std::boxed::Box::new($handle)); }',
      );
    }
    for (var i = 0; i < cls.extraImpls.length; i++) {
      final wider = cls.extraImpls[i];
      final arguments = '<${wider.arguments.map((a) => type(a)).join(', ')}>';
      final self = i < cls.extraImplSelf.length ? cls.extraImplSelf[i] : null;
      if (self == null) {
        _line(
          'if __t == std::any::TypeId::of::<dyn ${wider.name}$arguments>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn ${wider.name}$arguments>>() { return Some(std::boxed::Box::new(<Self as ${wider.name}$arguments>::dart_self_${snakeRaw(wider.name)}(self))); }',
        );
        continue;
      }
      // An impl written for one instantiation of this generic class: the
      // object answers only when it *is* that instantiation.
      final me = '${cls.name}<${self.map((a) => type(a)).join(', ')}>';
      _line(
        'if __t == std::any::TypeId::of::<dyn ${wider.name}$arguments>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn ${wider.name}$arguments>>() { if let Some(__me) = (self as &dyn std::any::Any).downcast_ref::<$me>() { return Some(std::boxed::Box::new(<$me as ${wider.name}$arguments>::dart_self_${snakeRaw(wider.name)}(__me))); } }',
      );
    }
    _line('None');
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    _emitOperators();
    _emitBaseImpl();
    _emitLazyStatics();
    return _out.join('\n') + '\n';
  }

  /// `impl Base for This`, when this class extends an abstract one.
  ///
  /// The methods **delegate** to the inherent ones rather than repeating their
  /// bodies, and the reason is a real difference between the two languages:
  /// Dart allows a covariant return, so `Alignment operator -()` legally
  /// overrides one declared to return `AlignmentGeometry`. Rust requires the
  /// impl to return exactly what the trait declared. Emitting the body twice
  /// would mean emitting it at two different return types.
  ///
  /// Delegating keeps one body and one idiomatic surface: `Alignment` still has
  /// its `impl Neg` returning an `Alignment`, which is what a Rust caller wants,
  /// and the trait method boxes that up for callers who only know the base.
  /// The prelude's traits for `dart:core` interfaces, with the methods each
  /// asks for: the impl forwards to the class's own.
  static const _preludeInterfaces = {
    'Comparable': [
      'compare_to(&self, other: __A0) -> i64',
      'compare_to(other)',
    ],
    'DartIterator': [
      'move_next(&self) -> bool',
      'move_next()',
      'current(&self) -> __A0',
      'current()',
    ],
  };

  /// The `dart:core` interfaces the prelude has a trait for that this class
  /// answers -- its own, and those an abstract ancestor listed: a class
  /// `implements CharacterRange` and it is `CharacterRange` that
  /// `implements Iterator<String>` (ws814).
  List<IrType> _preludeInterfacesOf(IrClass of) {
    final out = <String, IrType>{};
    for (final c in [of, ..._abstractAncestors(of)]) {
      for (final i in c.interfaces) {
        if (_preludeInterfaces.containsKey(i.name)) out[i.name] ??= i;
      }
    }
    return out.values.toList();
  }

  /// Whether this class has its own method for one of the forwarding calls
  /// (`compare_to(other)`, `current()`).
  ///
  /// The impl forwards to an *inherent* method. Reached only through a
  /// trait, `self.compare_to(other)` names two candidates and neither wins
  /// (E0034); with another arity -- `moveNext(int count)` against the
  /// prelude's `move_next()` -- it is the wrong method. 6 at ws815.
  String? _forwardingCall(String call) {
    final open = call.indexOf('(');
    final name = call.substring(0, open);
    final inside = call.substring(open + 1, call.length - 1).trim();
    final given = inside.isEmpty
        ? const <String>[]
        : [for (final a in inside.split(',')) a.trim()];
    for (final m in cls.methods) {
      if (m.isStatic || m.isSetter || m.operator != null) continue;
      if (snake(m.name) != name) continue;
      if (m.params.length == given.length) return call;
      // An override that *widens* the interface -- `CharacterRange
      // .moveNext([int count = 1])` where the prelude's `Iterator` says
      // `move_next()` -- is forwarded to with its own defaults, which is
      // what the interface means by the call. Withheld instead, the impl
      // was missing and `current()` was on no `dyn CharacterRange` (4 at
      // ws859; the defaults are what `IrParam.defaultValue` is for).
      if (m.params.length < given.length) continue;
      final extra = m.params.skip(given.length).toList();
      if (extra.any((p) => p.defaultValue == null)) continue;
      return '$name(${[for (final a in given) a, for (final p in extra) expr(p.defaultValue!)].join(', ')})';
    }
    return null;
  }

  void _emitPreludeInterfaces() {
    for (final i in _preludeInterfacesOf(cls)) {
      final methods = _preludeInterfaces[i.name];
      if (methods == null) continue;
      // Every call it would make has to land on a method of this class.
      final calls = <int, String>{};
      var forwards = true;
      for (var k = 1; k < methods.length; k += 2) {
        final call = _forwardingCall(methods[k]);
        if (call == null) {
          forwards = false;
        } else {
          calls[k] = call;
        }
      }
      if (!forwards) continue;
      final args = i.arguments.map((a) => type(a)).toList();
      final generic = args.isEmpty ? '' : '<${args.join(', ')}>';
      _member('impl ${i.name} for ${cls.name}', () {
        _line(
          'impl${_implGenerics(cls)} ${i.name}$generic for '
          '${cls.name}${_generics(cls)} {',
        );
        _indent++;
        for (var k = 0; k + 1 < methods.length; k += 2) {
          final signature = methods[k].replaceAll(
            '__A0',
            args.isEmpty ? 'std::rc::Rc<dyn Object>' : args[0],
          );
          _line('fn $signature {');
          _indent++;
          // The class's own method returns `Result`; the prelude trait's
          // signature is fixed.
          _line('self.${calls[k + 1]}${_resultModel ? '.unwrap()' : ''}');
          _indent--;
          _line('}');
        }
        _indent--;
        _line('}');
        _line('');
      });
    }
  }

  void _emitBaseImpl() {
    _emitPreludeInterfaces();
    // Every abstract **ancestor**, not just a direct abstract base. `Padded`
    // extends the concrete `Square`, which extends the abstract `Shape`; with
    // only the direct base considered, `Padded` implemented nothing and
    // `Shape`'s methods were unreachable from it.
    for (final ancestor in _abstractAncestors(cls)) {
      // Wrapped, like every other member. A `super` call or an `is` inside one
      // delegating method used to travel out of `_emitStruct` and take the
      // class with it -- the same gap round 53 found in the constructors.
      _member(
        'impl ${ancestor.name} for ${cls.name}',
        () => _emitImplFor(ancestor),
      );
    }
    // The wider instantiations the program names (`IrClass.extraImpls`):
    // an impl each, its signatures in the wider terms, forwarding to the
    // class's own methods through the coercion rule.
    for (var i = 0; i < cls.extraImpls.length; i++) {
      final wider = cls.extraImpls[i];
      final base = library[wider.name];
      if (base == null) continue;
      final self = i < cls.extraImplSelf.length ? cls.extraImplSelf[i] : null;
      _member(
        'impl ${wider.name}<${wider.arguments.join(', ')}> for ${cls.name}',
        () => _emitImplFor(
          base,
          passedOverride: wider.arguments,
          selfOverride: self,
        ),
      );
    }
  }

  /// The abstract classes above this one, nearest first.
  ///
  /// Mixins count. `class Panel extends Measured with Scaled` has to implement
  /// `Scaled` for `Scaled`'s methods to be reachable through it, exactly as it
  /// implements an abstract superclass -- a mixin is a base that does not sit
  /// on the `extends` chain, and looking only along that chain found none of
  /// them.
  List<IrClass> _abstractAncestors(IrClass of) {
    final found = <String, IrClass>{};
    // Bases are reached by **name**, and `library[...]` resolves a name against
    // this module and then the rest of the crate -- where two libraries are
    // allowed to declare the same one. `NetworkImage` in `image_provider.dart`
    // is abstract and hands its construction to a `NetworkImage` in
    // `_network_image_io.dart`, which implements it; `BitField` does the same.
    // Under a name lookup the second reaches the first, which is the second,
    // and the walk recursed until the stack ended. `seen` is what stops it --
    // and it earns its keep on plain diamonds too, where an ancestor was
    // re-walked once per path that reached it.
    final seen = <IrClass>{};
    void climb(IrClass? from) {
      if (from == null || !seen.add(from)) return;
      for (final name in [
        from.superclass,
        ...from.mixins.map((m) => m.name),
        // `implements` reaches a base too. Dart promises the members without
        // the bodies, which is what a Rust `impl` is.
        ...from.interfaces.map((i) => i.name),
      ]) {
        // ..in another module too: a trait's ancestors decide which trait
        // declares a member (`_declaringTrait`), and `ModalRoute`'s are
        // `widgets`' when the call is in `material` (run672).
        final above = library[name] ?? library.elsewhere[name];
        if (above == null) continue;
        // A class is not its own ancestor. Had the recursion above not ended
        // the run, this is what the same name collision would have emitted:
        // `impl NetworkImage for NetworkImage`.
        if (identical(above, from) || identical(above, of)) continue;
        if (above.isAbstract) found.putIfAbsent(above.name, () => above);
        climb(above);
      }
    }

    climb(of);
    return found.values.toList();
  }

  /// `<f32>` for `impl ParametricCurve<f32> for _Linear`, or nothing.
  ///
  /// Only the *direct* superclass's arguments are known. For a generic
  /// ancestor further up the chain they would have to be composed through each
  /// step, so that impl is refused rather than emitted with the wrong ones.
  String? _baseArguments(IrClass base) {
    if (base.typeParameters.isEmpty) return '';
    final passed = _baseTypeArguments(base);
    if (passed == null) return null;
    return '<${passed.map((a) => type(a)).join(', ')}>';
  }

  /// What this class passed the base's type parameters, or null when it cannot
  /// be worked out from here.
  List<IrType>? _baseTypeArguments(IrClass base) {
    if (base.typeParameters.isEmpty) return const [];
    return _argumentsThrough(cls, const {}, base, {});
  }

  /// Walk up from `current`, carrying the arguments through each step.
  /// `_Linear extends Curve` and `Curve extends ParametricCurve<double>`, so
  /// reaching ParametricCurve means going through Curve -- and a Curve that
  /// had parameters of its own would need ours substituted into what it
  /// passes on. A mixin or interface is a direct base of whichever class
  /// named it, so its arguments are read off the `with`/`implements` clause
  /// -- and followed through: `_SelectableFragment with Selectable`, where
  /// `Selectable implements SelectionHandler` and that `extends
  /// ValueListenable<SelectionGeometry>` (the impl was refused as "arguments
  /// not known here" while the `Selectable` trait required it, 2 E0277).
  List<IrType>? _argumentsThrough(
    IrClass current,
    Map<String, IrType> bound,
    IrClass base,
    Set<String> seen,
  ) {
    if (!seen.add(current.name)) return null;
    Map<String, IrType> binding(IrClass of, List<IrType> passed) => {
      for (var i = 0; i < passed.length; i++) of.typeParameters[i]: passed[i],
    };
    for (final mixin in [...current.mixins, ...current.interfaces]) {
      // Substituted *inside* the argument, not only when the argument is a
      // bare parameter: `FormFieldState<T> extends State<FormField<T>>`
      // passes `FormField<T>`, and `bound[a.name]` left that `T` standing --
      // 28 `E0747`s reading it as a constant.
      final passed = [
        for (final a in mixin.arguments) _substituteType(a, bound),
      ];
      if (mixin.name == base.name) {
        return passed.length == base.typeParameters.length ? passed : null;
      }
      final via = library[mixin.name];
      if (via == null || passed.length != via.typeParameters.length) continue;
      final found = _argumentsThrough(via, binding(via, passed), base, seen);
      if (found != null) return found;
    }
    final next = library[current.superclass];
    if (next == null) return null;
    final passed = [
      for (final a in current.superclassArguments) _substituteType(a, bound),
    ];
    if (next.name == base.name) {
      return passed.length == base.typeParameters.length ? passed : null;
    }
    if (passed.length != next.typeParameters.length) return null;
    return _argumentsThrough(next, binding(next, passed), base, seen);
  }

  /// Whether a getter's result can fill the trait's accessor: the same
  /// type, or one the coercion rule can put there -- an `Option` around
  /// it, a subclass's handle where the slot is a trait object. A
  /// differing *type argument* is not one of those: Dart's covariance
  /// makes a `WidgetStateProperty<Color>` a `WidgetStateProperty<Color?>`
  /// and Rust's does not, so those 82 keep reading the storage and stay a
  /// debt (run701).
  static bool _fitsAccessor(IrType have, IrType slot) {
    if (sameRust(have, slot)) return true;
    if (have.isFunction != slot.isFunction) return false;
    if (have.arguments.length != slot.arguments.length) return false;
    for (var i = 0; i < have.arguments.length; i++) {
      if (!sameRust(have.arguments[i], slot.arguments[i])) return false;
    }
    return true;
  }

  /// Whether a member's body reads `super.<name>`: then it is the base's
  /// storage, whatever else it does with it, and the base's accessor has
  /// to stay that storage.
  static bool _readsSuper(IrMethod m, String name) {
    final walk = _WalkSelf();
    walk.statement(m.body);
    return walk.superMembers.contains(name);
  }

  /// The trait whose impl block is being printed (`_emitImplFor`).
  String? _implFor;

  void _emitImplFor(
    IrClass base, {
    List<IrType>? passedOverride,
    List<IrType>? selfOverride,
  }) {
    _implFor = base.name;
    _selfBinding = selfOverride == null
        ? const {}
        : {
            for (
              var i = 0;
              i < cls.typeParameters.length && i < selfOverride.length;
              i++
            )
              cls.typeParameters[i]: selfOverride[i],
          };
    // Not just the abstract ones. A class that overrides a *concrete* base
    // method needs that override in the impl too, or dynamic dispatch reaches
    // the trait's default instead -- the inherent method would still be right,
    // so only a call through `dyn Base` can tell, which is why the tests make
    // that call.
    // ..and one that an ancestor nearer than the base overrides
    // (`_overriddenAbove`), for the same reason.
    final overridden = base.methods
        .where((m) => !m.isStatic && _overriddenAbove(base, m))
        .toList();
    // Accessors come from this base alone here; a farther ancestor gets its own
    // impl block and its own.
    final ownFields = base.fields;
    final required = [...base.abstractMethods, ...overridden];
    // Accessors count as a reason to emit the impl. A base with no abstract
    // methods and nothing overridden still has fields, and without them the
    // subclass does not implement the trait at all -- so its inherited methods
    // are unreachable, which is how `area()` went missing.
    // ..less the ones an abstract supertype of the base declares, which
    // the trait left to that ancestor (see `_emitTrait`) and whose impl
    // block for this class carries them.
    final inheritedByBase = {
      for (final above in _supertypesOf(base))
        if (library.isAbstract(above.name))
          for (final f in above.fields) f.name,
    };
    final accessors = [
      for (final f in ownFields)
        if (!inheritedByBase.contains(f.name)) f,
    ];
    // No early return when both are empty. A Dart subclass *is* its base
    // whether or not it changes anything, so the impl has to exist even with
    // nothing in it -- `Panel extends Measured with Scaled` overrides neither
    // and the mixin has no fields, and without `impl Scaled for Panel {}` the
    // free function holding `Scaled`'s body cannot be called on a `Panel`:
    // "the trait bound `Panel: Scaled` is not satisfied". An empty impl block
    // is the whole statement that it is one.

    final arguments = passedOverride != null
        ? '<${passedOverride.map((a) => type(a)).join(', ')}>'
        : _baseArguments(base);
    if (arguments == null) {
      // A generic ancestor whose arguments cannot be worked out from here.
      // Emitting `impl Base for This` without them does not compile; saying so
      // is better than leaving rustc to.
      _line('');
      _line('// NOT TRANSLATED: impl ${base.name} for ${cls.name}');
      _line('//   the base is generic and its arguments are not known here');
      return;
    }
    // Every signature in the block is the trait's, so it is spelled the
    // trait's way -- a callback parameter is `&dyn Fn`, not `impl Fn`, or the
    // impl declares a type parameter the trait method does not have.
    _inTrait = true;
    // Bound for the whole block: every signature inside is written in the
    // base's terms and has to come out in this class's.
    final passed = passedOverride ?? _baseTypeArguments(base) ?? const [];
    _implBinding = {
      if (passed.length == base.typeParameters.length)
        for (var i = 0; i < passed.length; i++)
          base.typeParameters[i]: passed[i],
    };
    _line('');
    // The parameters are *declared* on the impl before they are used.
    // `impl Trait<T> for Foo<T>` does not compile -- nothing introduced the
    // first `T` -- and leaving the declaration off was 428 `cannot find type
    // T` in the widget layer alone, one for every generic class's every trait
    // impl. The struct's own inherent impl had it right all along, which is
    // why it took a slice big enough to hold a generic class to show.
    // `'static` on the parameters, because the trait requires `DartAny` and
    // `DartAny` hands out a `&dyn Any`. A generic class implementing a trait
    // is the commonest shape in the widget layer, so leaving the bound off
    // here was 620 `E0310` in one go.
    // For one concrete instantiation (`selfOverride`): no impl generics,
    // the class spelled with those arguments.
    _line(
      selfOverride != null
          ? 'impl ${base.name}$arguments for '
                '${cls.name}<${selfOverride.map((a) => type(a)).join(', ')}> {'
          : 'impl${_implGenerics(cls)} ${base.name}$arguments for '
                '${cls.name}${_generics(cls)} {',
    );
    _indent++;
    // The handle a trait body's `this` is. A struct that is not counted has
    // no identity to give, and a fresh handle around a copy is what the
    // rest of its translation does with it too.
    _line(
      'fn dart_self_${snakeRaw(base.name)}(&self) -> std::rc::Rc<dyn ${base.name}$arguments> {',
    );
    _indent++;
    // ..and a generic value class can be cloned here too: the impl's
    // generics carry `T: Clone` since ws595 (`_implGenerics`), so the
    // derived `Clone` holds (`_ModalScope<T>` had "no handle of its own"
    // where `createElement` wanted one, run640).
    _line(
      cls.counted
          ? 'self.__self.get()'
          : _cloneable(cls)
          ? 'std::rc::Rc::new(self.clone())'
          : 'todo!("${cls.name} has no handle of its own")',
    );
    _indent--;
    _line('}');
    _line('');
    // A field and a method of the same name are one item in Rust. A mixin
    // routinely has both -- `Ticker? _ticker;` beside a getter that reads it --
    // and emitting the accessor as well as the method put two `fn _ticker` in
    // one impl: 839 `E0201`s the moment mixins started being implemented. The
    // method wins, because it is the one that may have a body worth keeping.
    final taken = {for (final need in required) _methodName(need)};
    // The base's field is only *this* class's field when this class inherited
    // it. `class X extends A with M implements B` does not: a mixin's `on`
    // clause puts its constraint on the extends chain, so `B` is reached as an
    // ancestor while `X` satisfies it by implementing -- `viewId` there is a
    // getter of X's own, forwarding to something else, and reading
    // `self.view_id` names a field the struct does not have. 345 of those in
    // `PointerEvent` alone.
    final held = {for (final f in _allFields(cls)) f.name};
    for (final field in base.appliedFields) {
      if (!_handsCell(field) || accessors.any((a) => a.name == field.name)) {
        continue;
      }
      final cell = held.contains(field.name) ? _sharedField(field.name) : null;
      final substituted = _lateWrapped(
        field,
        type(_substituteType(field.type, _implBinding)),
      );
      _line(
        'fn ${snake(field.name)}_cell(&self) -> ${_wrapped(_cellType(substituted))} {',
      );
      _indent++;
      _line(
        cell != null
            ? (_resultModel
                  ? 'Ok(self.${snake(field.name)}.clone())'
                  : 'self.${snake(field.name)}.clone()')
            : 'todo!("${cls.name}.${field.name} is mutated through a trait but is not a cell")',
      );
      _indent--;
      _line('}');
      _line('');
    }
    for (final field in accessors) {
      // The cell accessor first, before a getter of the class's own can
      // take the value accessor's place: the trait asks for both.
      if (_handsCell(field)) {
        final cell = held.contains(field.name)
            ? _sharedField(field.name)
            : null;
        final substituted = _lateWrapped(
          field,
          type(_substituteType(field.type, _implBinding)),
        );
        _line(
          'fn ${snake(field.name)}_cell(&self) -> ${_wrapped(_cellType(substituted))} {',
        );
        _indent++;
        _line(
          cell != null
              ? (_resultModel
                    ? 'Ok(self.${snake(field.name)}.clone())'
                    : 'self.${snake(field.name)}.clone()')
              : 'todo!("${cls.name}.${field.name} is mutated through a trait but is not a cell")',
        );
        _indent--;
        _line('}');
        _line('');
      }
      if (taken.contains(snake(field.name))) continue;
      // Cloned out: the accessor returns a value and the field is behind
      // `&self` -- `fn _buffer(&self) -> Vec<i64> { self._buffer }` moved it.
      // ..and through the cell when the field is in one (a counted class):
      // `self.parent.clone()` handed out the `Rc<RefCell<..>>` itself.
      final cell = _sharedField(field.name);
      // A `late` field is held as an `Option` and the trait's accessor
      // gives the declared type: the read unwraps (Dart's read of an unset
      // `late` throws; this panics), the write wraps. `RenderObject`'s
      // `late bool _needsCompositing` alone was 363 mismatches in
      // `rendering` (194 `set`, 169 reads).
      final late = field.isLate ? '.unwrap()' : '';
      // A getter this class declares *overrides* the base's field: Dart
      // resolves the name to the getter, and a call through the trait is
      // the one path that can tell (`_SwitchDefaultsM3.padding` is
      // `EdgeInsets.symmetric(horizontal: 4)` where `SwitchThemeData`'s
      // field is null, and the switch's size read the field, run698).
      // Only when the getter's result is the trait's own type: a Dart
      // override may narrow it (`WidgetStateProperty<Color>` for a
      // `WidgetStateProperty<Color?>`), and that is a different Rust type
      // and a debt of its own -- those keep reading the storage.
      final ownGetter = cls.methods
          .where(
            (m) =>
                m.name == field.name &&
                !m.isStatic &&
                !m.isSetter &&
                m.params.isEmpty,
          )
          .firstOrNull;
      // ..and not a getter that reads `super.<name>` itself: the base's
      // accessor *is* the storage a `super` read reaches, so routing it
      // to such a getter is a cycle (`ListenableBuilder.listenable` and
      // `AnimatedBuilder.listenable` are both `=> super.listenable`, for
      // a doc comment, and the program overflowed its stack, run699).
      // Such a getter is the base's `x` anyway.
      // ..and only a getter this block can *call* as `self.x()`: an
      // inherent method taking `&self`. A counted class's method that
      // hands out `this` takes `&Rc<Self>` (`_receiverOf`), which a trait
      // body has no handle for; one an abstract supertype declares as a
      // *method* was emitted into that trait's impl, where the call is
      // ambiguous with the one being written here (`_TimePickerDefaults`
      // and `TimePickerThemeData` both declare `hourMinuteTextColor`,
      // ws702). Those keep reading the storage, with the covariant ones.
      final reachable =
          ownGetter != null &&
          !(cls.counted && _handles.contains(_rustName(ownGetter))) &&
          !_supertypesOf(cls)
              .where((t) => library.isAbstract(t.name))
              .any(
                (t) =>
                    t.methods.any(
                      (m) => m.name == field.name && !m.isStatic && !m.isSetter,
                    ) ||
                    t.abstractMethods.any(
                      (m) => m.name == field.name && !m.isSetter,
                    ),
              );
      final overrides =
          ownGetter != null &&
          reachable &&
          !_readsSuper(ownGetter, field.name) &&
          _fitsAccessor(
            _selfBound(ownGetter.returnType),
            _substituteType(field.type, _implBinding),
          );
      final reads = overrides
          ? 'self.${snake(field.name)}()${_resultModel ? '?' : ''}'
          : held.contains(field.name)
          ? (cell != null
                ? (_lazyLate(field)
                      ? _lazyRead(field, 'self')
                      : _isCopy(_heldDecl(cell))
                      ? 'self.${snake(field.name)}.get()$late'
                      : 'self.${snake(field.name)}.borrow().clone()$late')
                : _isCopy(type(_substituteType(field.type, _implBinding)))
                ? 'self.${snake(field.name)}$late'
                : 'self.${snake(field.name)}.clone()$late')
          : cls.methods.any((m) => m.name == field.name && !m.isStatic)
          // A getter is a method and returns `Result`; the accessor the
          // trait asks for cannot, and unwraps.
          ? 'self.${snake(field.name)}()${_resultModel ? '?' : ''}'
          : null;
      // The accessor's type is the *trait's*, so it is written in this
      // class's terms like every other signature in the block. Round 73
      // substituted the methods and left the accessors behind, which put a
      // `T` no impl declares in front of 103 field reads.
      // `todo!()`, not a refusal. A refused accessor leaves the trait
      // unimplemented -- 18 `E0046`s, one of them naming twenty-three at once
      // -- and the method path next door has always written a `todo!()` for
      // exactly this. The two owe the same answer.
      //
      // The case is real: `_TransformedPointerAddedEvent` gets `viewId` from a
      // mixin, and the IR does not copy a mixin's methods into the class, so
      // nothing here can see the getter that does exist. Reaching it means
      // going through the mixin's own trait, which is a round of its own.
      final body =
          reads ?? 'todo!("${cls.name} does not translate ${field.name} yet")';
      final substituted = _substituteType(field.type, _implBinding);
      _line(
        'fn ${snake(field.name)}(&self) -> ${_wrapped(type(substituted))} {',
      );
      _indent++;
      // The field holds one `Option`; a trait asking for the doubled one
      // gets it wrapped -- and the whole in `Ok`.
      // ..and any other difference between this class's field and the
      // trait's -- a `Matrix4` field under a `Matrix4?` accessor
      // (`_TransformedPointerCancelEvent.transform`, 15 at ws463) -- by
      // the one rule, as a method's result is.
      final own = _allFields(cls)
          .where((f) => f.name == field.name)
          .firstOrNull;
      String value;
      // What the body hands back: the getter's own result when the
      // accessor calls it, the field as this *instantiation* holds it
      // otherwise -- under `impl ValueKey<Option<i64>> for ValueKeyImpl
      // <i64>` the `T value` is an `i64`, and typed `T` the rule could
      // not see the `Some` it needed (ws659).
      final handed = overrides
          ? _selfBound(ownGetter!.returnType)
          : own == null
          ? null
          : _selfBound(own.type);
      if (reads != null &&
          handed != null &&
          (overrides || substituted.name != 'Option')) {
        final held = IrLocal('__v')..rustType = handed;
        final shaped = coerceInto(held, substituted, _world);
        // Nothing bridged the two and they are not the same type: a
        // *wider* impl of a generic trait whose accessor is a struct at
        // another instantiation (`_DelegateState<Object>.element` over a
        // `_InheritedProviderScopeElement<Listenable?>` field, ws710).
        // `todo!()` rather than code that does not compile: the method
        // path next door has always said it that way, and a body that
        // does not compile takes the whole function with it.
        value = identical(shaped, held)
            ? (sameRust(handed, substituted)
                  ? body
                  : 'todo!("${cls.name}.${field.name} is ${type(handed)} '
                        'and ${_implFor ?? base.name} asks '
                        '${type(substituted)}")')
            : '{ let __v = $body; ${expr(shaped)} }';
      } else {
        value = substituted.name == 'Option' && reads != null
            ? 'Some($body)'
            : body;
      }
      // ..and a getter the trait asks for that this class writes as a
      // *method* hands back that method's type, which an override may have
      // narrowed (`_FileSpan.end` is a `FileLocation` where
      // `SourceSpanBase.end` is `Rc<dyn SourceLocation>`, 6 at ws757). Only
      // where the rule has something to say: no conversion leaves the body
      // as it was, since this branch used to have no type to compare at all.
      if (handed == null && reads != null) {
        final ownMethod = cls.methods
            .where((m) => m.name == field.name && !m.isStatic && !m.isSetter)
            .firstOrNull;
        if (ownMethod != null) {
          final from = IrLocal('__v')
            ..rustType = _selfBound(ownMethod.returnType);
          final shaped = coerceInto(from, substituted, _world);
          if (!identical(shaped, from)) {
            value = '{ let __v = $body; ${expr(shaped)} }';
          }
        }
      }
      _line(reads != null && _resultModel ? 'Ok($value)' : value);
      _indent--;
      _line('}');
      _line('');
      // The setter the trait asks for on a mutable field (see `_emitTrait`).
      // Every setter the trait declares, held or not: an impl missing one
      // is "not all trait items implemented", and a whole crate with it
      // (`SnapshotController with ChangeNotifier`, the round the gate opened).
      if (_writable(field)) {
        final cell = held.contains(field.name)
            ? _sharedField(field.name)
            : null;
        _line(
          'fn set_${snake(field.name)}(&self, value: ${type(substituted)}) -> ${_wrapped('()')} {',
        );
        _indent++;
        if (cell != null) {
          // The trait's view of the field may be wider than this class's
          // (`Tween<T>.begin` as `T?` erased against `ColorTween`'s
          // `Color?`): the value is adapted into what the field holds.
          final own = cell.type;
          final given = IrLocal('value')..rustType = substituted;
          final into = type(substituted) == type(own)
              ? given
              : coerceInto(given, own, _world, inClosure: true);
          // ..and nothing bridged them: `todo!()`, as the read above says
          // it (`_DelegateState<Object>.element` over a
          // `_InheritedProviderScopeElement<Listenable?>` field, ws711).
          if (identical(into, given) && !sameRust(substituted, own)) {
            _line(
              'todo!("${cls.name}.${field.name} is ${type(own)} and '
              '${_implFor ?? base.name} writes ${type(substituted)}")',
            );
          } else {
            final adapted = expr(into);
            final stored = field.isLate ? 'Some($adapted)' : adapted;
            _line(
              _isCopy(_heldDecl(cell))
                  ? 'self.${snake(field.name)}.set($stored);'
                  : '*self.${snake(field.name)}.borrow_mut() = $stored;',
            );
            if (_resultModel) _line('Ok(())');
          }
        } else {
          _line(
            'todo!("${cls.name}.${field.name} is written through a trait but is not a cell")',
          );
        }
        _indent--;
        _line('}');
        _line('');
      }
    }
    for (final need in required) {
      _member(
        'impl ${base.name}::${need.operator ?? need.name} for ${cls.name}',
        () => _emitBaseMethod(need),
      );
    }
    _indent--;
    _line('}');
    _selfBinding = const {};
  }

  /// The base's type parameters, bound to what this class passed them.
  ///
  /// A trait method is declared in the base's terms -- `_RRectLike<T>` has
  /// `fn _create(..) -> T` -- and `impl _RRectLike<RRect> for RRect` has to
  /// say `-> RRect`. Copying the declaration through left a `T` no impl
  /// declares, which is the same mistake flattening made with fields one level
  /// down.
  var _implBinding = <String, IrType>{};

  /// The class's own type parameters bound to one concrete instantiation,
  /// while its wider impl for that instantiation is written (see
  /// `IrClass.extraImplSelf`); empty otherwise.
  Map<String, IrType> _selfBinding = const {};

  /// The method with each type parameter that shadows one of the class's
  /// renamed `T_` in its signature, or null when none does. The body is
  /// not rewritten: only a forwarder or a stub may use the result.
  IrMethod? _renamedShadowed(IrMethod need) {
    final shadowed = {
      for (final p in need.typeParameters)
        if (cls.typeParameters.contains(p)) p: IrType('${p}_'),
    };
    if (shadowed.isEmpty) return null;
    return IrMethod(
      need.name,
      [
        for (final p in need.params)
          IrParam(
            p.name,
            _substituteType(p.type, shadowed),
            named: p.named,
            hasDefault: p.hasDefault,
            kept: p.kept,
            mutRef: p.mutRef,
          ),
      ],
      _substituteType(need.returnType, shadowed),
      need.body,
      typeParameters: [
        for (final p in need.typeParameters)
          shadowed.containsKey(p) ? '${p}_' : p,
      ],
      isStatic: need.isStatic,
      isGetter: need.isGetter,
      isSetter: need.isSetter,
      operator: need.operator,
      throws: need.throws,
      doc: need.doc,
      isAsync: need.isAsync,
    );
  }

  void _emitBaseMethod(IrMethod need) {
    {
      // A method type parameter named like one of the class's --
      // `ParentDataElement<T>` implementing `BuildContext.
      // dependOnInheritedWidgetOfExactType<T>` -- is renamed here rather
      // than refused: this forwarder's body is the backend's own line and
      // never spells the parameter, so only the signature has to change
      // (4 "not all trait items implemented" in `widgets`, one per
      // generic `Element`).
      need = _renamedShadowed(need) ?? need;
      // A forwarder has parameters, not locals: the last body's cell locals
      // printed a parameter `child` as `child.borrow()` (7 at ws383).
      _cellLocals = {};
      // ..and the inherent method it reaches spelled with the same renaming,
      // so that its `T?` and the trait's `T_?` compare as one type and not
      // as two the coercion rule converts between (22 at ws411).
      var have = _matching(need);
      if (have != null && have.typeParameters.isNotEmpty) {
        have = _renamedShadowed(have) ?? have;
      }
      String? via;
      if (have == null) {
        final inherited = _inherited(need);
        if (inherited != null) {
          via = inherited.$1.name;
          have = inherited.$2;
          if (have.typeParameters.isNotEmpty) {
            have = _renamedShadowed(have) ?? have;
          }
        }
      }
      // Rust does not collapse `Option<Option<X>>` the way Dart collapses
      // `T?` for a nullable `T`: `MessageCodec<Object?>.decodeMessage` is
      // `-> Option<T>` in the trait and the impl must say `Option<Option<..>>`
      // -- 16 `E0053`s, the "14 members" `_substituteType`'s comment gave up
      // on. Spelled out here, with the body wrapped to match below.
      final returns = _spelledReturn(
        type(_substituteType(need.returnType, _implBinding)),
      );
      final wrappedReturns = _wrapped(returns);
      final params = [
        // The forwarder's receiver is the trait's: `&mut self` when any
        // implementer writes in this method, or `ChangeNotifier::
        // add_listener(self, ..)` under `&self` is a mutability mismatch.
        if (!need.isStatic) _sharedMutation(need) ? '&mut self' : '&self',
        ...need.params.map((p) {
          // A parameter whose type *is* one of the base's type parameters has
          // to be written the way the impl header wrote that parameter, which
          // is owned: Rust substitutes `ChildType` with the
          // `Box<dyn RenderBox>` in `impl RenderObjectWithChildMixin<Box<dyn
          // RenderBox>>`, and a borrowed `&dyn RenderBox` here is a different
          // type from the one the trait declared.
          final substituted = _substituteType(p.type, _implBinding);
          final fromParameter = _implBinding.containsKey(p.type.name);
          return _param(
            IrParam(
              p.name,
              substituted,
              named: p.named,
              hasDefault: p.hasDefault,
              // Carried, or the impl writes `&dyn Fn` where the trait it
              // implements declared `Box<dyn Fn>`.
              kept: p.kept,
              mutRef: p.mutRef,
            ),
            owned: fromParameter,
          );
        }),
      ].join(', ');
      _line(
        'fn ${_methodName(need)}${_generics(need)}($params) -> '
        '$wrappedReturns${_sizedBound(need)} {',
      );
      _indent++;
      // A mixin's field is an abstract getter and setter on its trait,
      // and the struct holds the field (flattened from the application):
      // read and written here, as an interface's field is above. 2747
      // `todo!`s at ws345 were these (`_tickerModeNotifier` 198, `_child`
      // 180, `_bucket` 110).
      final field = have == null
          ? _allFields(cls).where((f) => f.name == need.name).firstOrNull
          : null;
      if (field != null &&
          !need.isStatic &&
          (need.isSetter ? need.params.length == 1 : need.params.isEmpty)) {
        final cell = _sharedField(field.name);
        final late = field.isLate ? '.unwrap()' : '';
        final name = snake(field.name);
        if (need.isSetter) {
          if (cell != null) {
            // The trait's type is the erased bound (`Option<Rc<dyn
            // RenderObject>>`), the field's the narrower one (`RenderBox?`):
            // the trait cast narrows on the way in, and an `Option` is
            // taken off or put on (+319 mismatched at ws346).
            final given = _substituteType(
              need.params.single.type,
              _implBinding,
            );
            final held = field.type;
            var value = 'value';
            if (given.name != held.name &&
                library.isAbstract(given.name) &&
                library.isAbstract(held.name) &&
                held.name != 'Object') {
              final target = _dynOf(
                IrType(held.name, arguments: held.arguments),
              );
              value =
                  'value.dart_cast_to::<$target>()${held.nullable ? '' : '.unwrap()'}';
            } else if (held.nullable && !given.nullable) {
              value = 'Some(value)';
            } else if (!held.nullable && given.nullable) {
              value = 'value.unwrap()';
            }
            final stored = field.isLate ? 'Some($value)' : value;
            _line(
              _isCopy(_heldDecl(cell))
                  ? 'self.$name.set($stored);'
                  : '*self.$name.borrow_mut() = $stored;',
            );
            if (_resultModel) _line('Ok(())');
          } else {
            _line(
              'todo!("${cls.name}.${field.name} is written through a trait but is not a cell")',
            );
          }
        } else {
          final read = cell != null
              ? (_lazyLate(field)
                    ? _lazyRead(field, 'self')
                    : _isCopy(_heldDecl(cell))
                    ? 'self.$name.get()$late'
                    : 'self.$name.borrow().clone()$late')
              : _isCopy(type(field.type))
              ? 'self.$name$late'
              : 'self.$name.clone()$late';
          // ..and widened on the way out (`_shaped`), as a method's
          // result is.
          // ..and widened on the way out by the one rule (`coerceInto`),
          // as a method's result is.
          // ..in *this* class's terms first, as the method path does
          // (`_selfBound`): inside a wider impl for one instantiation the
          // field's `T` is the class's and the trait's `T` is the wider
          // argument, and left unresolved the two read as the same name
          // and the rule found nothing to do -- `Ok(self.value)` where an
          // `Option<f64>` goes (`AlwaysStoppedAnimation`, 2 at ws870).
          final held = IrLocal('__v')..rustType = _selfBound(field.type);
          if (Platform.environment['DART2RUST_TRACE_FWD'] == field.name) {
            stderr.writeln(
              'TRACE_FWD ${cls.name}.${field.name} field=${field.type} need=${need.returnType} for=${_implFor}',
            );
          }
          final shaped = coerceInto(
            held,
            _substituteType(need.returnType, _implBinding),
            _world,
          );
          final value = identical(shaped, held)
              ? read
              : '{ let __v = $read; ${expr(shaped)} }';
          _line(_resultModel ? 'Ok($value)' : value);
        }
      } else if (have == null) {
        // Reported in the output rather than silently skipped: a trait impl
        // missing a method does not compile, and the reader should learn why
        // from the file rather than from rustc.
        _line(
          'todo!("${cls.name} does not translate '
          '${need.operator ?? need.name} yet")',
        );
      } else {
        // ..and an async inherent method is a future the forwarder wraps
        // in `Ok` (49 `Pin<Box<impl Future>>` where `Result<..>` goes).
        // ..unless it is reached through an ancestor's trait (`via`): the
        // trait's method already returns the `Result`, and `Ok(ModalRoute::
        // will_pop(self))` doubled it (30 `will_pop` forwarders at ws535).
        final inherent = _inherentCall(have, need, via);
        final call = have.isAsync && _resultModel && via == null
            ? 'Ok($inherent)'
            : inherent;
        // One `Option` short -- the override narrowed `T?` to `T`, which Dart
        // allows, or the trait's `T?` doubled up above -- is a `Some`.
        // The trait's future carries `+ '_` (see `_lifetimed`); the
        // inherent one is the same future without the spelling.
        // An `Rc<Concrete>` returned where the trait says `Rc<dyn Base>`
        // unsizes on its own at the return (47 `Box<Rc<dyn State>>`s).
        // ..a *value* returned there is put behind a fresh handle (`impl
        // BorderRadiusGeometry for BorderRadius`'s `op_mul`, 79), and a
        // `()` where the trait says `Option<..>` is `None` (`Action.invoke`
        // overridden as `void`, 46).
        // ..all by the one rule (`coerceInto`) inside the `Result`'s `map`.
        // A future is the same future under a lifetime spelling and is
        // left alone.
        final held = IrLocal('__v')
          ..rustType = _selfBound(_inThisClassTerms(have.returnType, via));
        if (Platform.environment['DART2RUST_TRACE_FWD'] == need.name) {
          stderr.writeln(
            'TRACE_FWD ${cls.name}.${need.name} have=${have.returnType} need=${need.returnType} method',
          );
        }
        // ..an async one too: the same future is left alone by the rule
        // (identical types), and a future of a value where the trait's
        // erased twin says `dyn Object` is mapped (`LocaleNamesLocalizations
        // Delegate.load` returning `Future<LocaleNames>` under
        // `LocalizationsDelegate<T>`, run598).
        final needReturns = _substituteType(need.returnType, _implBinding);
        final shaped = coerceInto(held, needReturns, _world, inClosure: true);
        // An override may return where the trait returns nothing
        // (`Disposer addListener(..)` over `void addListener(..)` in get's
        // `ListNotifier`): the value is dropped (run489).
        final dropsValue =
            !have.isAsync &&
            type(needReturns) == '()' &&
            type(have.returnType) != '()';
        // An inherent *operator* is a `std::ops` method: it takes `self` by
        // value and hands back the `Output` itself, not a `Result`. So the
        // conversion binds the value rather than mapping a `Result`, and
        // the trait's own `Result` is put on here (`impl EdgeInsetsGeometry
        // for EdgeInsets`'s `op_mul` got `*self * other.map(|__v| ..)`,
        // which mapped the *operand*, 6 at ws757).
        final infallible =
            have.operator != null && operatorTraits.containsKey(have.operator);
        final wrapsOk = _returnType(need).startsWith('Result<');
        String infallibleText(String inner) => wrapsOk ? 'Ok($inner)' : inner;
        _line(
          dropsValue
              ? (infallible
                    ? infallibleText('{ let _ = $call; () }')
                    : '$call.map(|_| ())')
              : identical(shaped, held)
              ? (infallible ? infallibleText(call) : call)
              : infallible
              ? '{ let __v = $call; ${infallibleText(expr(shaped))} }'
              : '$call.map(|__v| ${expr(shaped)})',
        );
      }
      _indent--;
      _line('}');
      _line('');
      final implFor = _implFor;
      if (implFor != null) _emitErasedImplTwin(need, implFor);
    }
  }

  /// This class's own version of a method the base requires.
  IrMethod? _matching(IrMethod need) {
    for (final method in cls.methods) {
      if (need.operator != null) {
        if (method.operator == need.operator) return method;
      } else if (method.operator == null &&
          method.name == need.name &&
          method.isSetter == need.isSetter) {
        // The getter and the setter share a name: `ValueListenable.value`
        // asked for its getter and got `TextEditingController`'s setter,
        // whose `newValue` "the base has no value for", and the impl
        // block came out without `value` at all.
        return method;
      }
    }
    return null;
  }

  /// How to invoke this class's own version, in Rust's own spelling.
  ///
  /// An operator that became an `impl std::ops::*` is invoked as the operator,
  /// not as a method: that is the whole point of having emitted the trait impl.
  /// The nearest class above this one with a body for `need`: an open
  /// class's `Impl` struct has none of its own (`_implOf`), and a subclass
  /// inherits the base's -- both reach the base trait's default through
  /// `Base::name(self, ..)`. Until ws345 every such method was a
  /// `todo!("X does not translate Y yet")`: 26199 of them, `insert`,
  /// `perform_layout` and `first_child` of `RenderFlexImpl` and all 796
  /// getters of each `GalleryLocalizationsXxImpl` -- compiled, never ran.
  ///
  /// The walk is Dart's own lookup order: a class's members, then its mixins
  /// nearest-applied first, then the superclass -- and so on up. Walking the
  /// `extends` chain alone passed over a mixin's override, and `class X
  /// extends Element with M` reached `Element.mount` where Dart runs
  /// `M.mount`.
  (IrClass, IrMethod)? _inherited(IrMethod need) {
    IrMethod? declared(IrClass at) {
      for (final method in at.methods) {
        if (need.operator != null) {
          if (method.operator == need.operator) return method;
        } else if (method.operator == null &&
            method.name == need.name &&
            method.isSetter == need.isSetter &&
            method.isStatic == need.isStatic) {
          return method;
        }
      }
      return null;
    }

    final seen = <String>{cls.name};
    IrClass? at = cls;
    var own = true;
    while (at != null) {
      // This class's own members are `_matching`'s; the walk starts at its
      // mixins.
      if (!own) {
        if (!library.isAbstract(at.name)) return null;
        final method = declared(at);
        if (method != null) return (at, method);
      }
      own = false;
      for (final applied in at.mixins.reversed) {
        final mixin = library[applied.name];
        if (mixin == null || !seen.add(mixin.name)) continue;
        if (!library.isAbstract(mixin.name)) continue;
        final method = declared(mixin);
        if (method != null) return (mixin, method);
      }
      final above = at.superclass;
      if (above == null || !seen.add(above)) return null;
      at = library[above];
    }
    return null;
  }

  /// Whether a body nearer than `base`'s answers `need` for this class: an
  /// abstract class between the two, or a mixin, overrides it.
  ///
  /// A Rust trait's default method does not replace the one a supertrait
  /// declared: `ComponentElement::mount` is what a `dyn ComponentElement`
  /// reaches, and a `dyn Element` still reaches `Element::mount`. The
  /// struct's `impl Element` has to route the method to the nearest override
  /// itself, exactly as it routes an abstract method to the nearest body.
  /// Without it, `StatefulElementImpl.mount` ran `Element.mount` alone and
  /// never built a child (run534: the tree ended at `View`).
  bool _overriddenAbove(IrClass base, IrMethod need) {
    if (_matching(need) != null) return true;
    final inherited = _inherited(need);
    if (Platform.environment['DART2RUST_TRACE_FWD'] == need.name) {
      stderr.writeln(
        'TRACE_FWD ${cls.name}.${need.name} above=${inherited?.$1.name} base=${base.name} super=${cls.superclass} abstract=${library.isAbstract(cls.superclass)}',
      );
    }
    return inherited != null &&
        !identical(inherited.$1, base) &&
        inherited.$1.name != base.name;
  }

  /// A type of an inherited method (`via`, the ancestor declaring it) in
  /// this class's terms: the ancestor's parameters replaced by what this
  /// class passes it (`RestorableEnumN<T> extends RestorableValue<T?>`:
  /// `RestorableValue`'s `T` is `Option<T>` here, and the forwarder
  /// handed `initWithValue` a bare `Orientation`, ws628).
  IrType _inThisClassTerms(IrType t, String? via) {
    if (via == null) return t;
    final base = library[via];
    if (base == null || base.typeParameters.isEmpty) return t;
    final passed = _argumentsThrough(cls, const {}, base, {});
    if (passed == null || passed.length != base.typeParameters.length) {
      return t;
    }
    return _substituteType(t, {
      for (var i = 0; i < passed.length; i++) base.typeParameters[i]: passed[i],
    });
  }

  String _inherentCall(IrMethod method, [IrMethod? through, String? via]) {
    // Dart lets an override *widen* an optional signature:
    // `OutlinedBorder.copyWith({side})` is overridden by
    // `BeveledRectangleBorder.copyWith({side, borderRadius})`. Rust does not,
    // so the trait method has fewer parameters than the inherent one it
    // delegates to -- and passing the inherent one's names through named a
    // `border_radius` that is not in scope, 30 times.
    //
    // What a caller reaching this through the trait would get in Dart is the
    // extra optionals *absent*, so that is what is passed: `None`. An extra
    // parameter that is not optional cannot be answered that way and the
    // delegation is refused instead of guessed at.
    // Positional parameters line up by **position**, not by name. Dart lets an
    // override rename them -- `Simulation.x(double time)` is overridden by
    // `x(double timeInSeconds)` -- and matching on the name called that a
    // widening and refused it, which left the trait unimplemented: 31 `E0046`s
    // for what is only a different word.
    final named = through == null
        ? null
        : {for (final p in through.params.where((p) => p.named)) p.name};
    final positional = through == null
        ? 0
        : through.params.where((p) => !p.named).length;
    // And the name to pass is the **caller's**, not the callee's. The
    // signature being written is the trait's, so `time` is what is in scope;
    // passing the inherent method's `timeInSeconds` names nothing.
    var at = -1;
    final args = method.params.map((p) {
      if (!p.named) at++;
      if (through == null) return snake(p.name);
      final supplied = p.named ? named!.contains(p.name) : at < positional;
      if (supplied) {
        final from = p.named
            ? through.params.firstWhere((q) => q.named && q.name == p.name)
            : through.params.where((q) => !q.named).elementAt(at);
        // A trait parameter doubled to `Option<Option<..>>` arrives one
        // `Option` deeper than the inherent method takes it.
        final traitType = _substituteType(from.type, _implBinding);
        final doubled = traitType.name == 'Option';
        final flattened = doubled && traitType.arguments.length == 1
            ? IrType(
                traitType.arguments.single.name,
                nullable: true,
                arguments: traitType.arguments.single.arguments,
              )
            : traitType;
        // The argument as the trait typed it, into the inherent method's
        // parameter, by the one rule (`coerceInto`): a widened override
        // (`equals(Object? e1, ..)` under `Equality<E>.equals(E, ..)`) is
        // shared into `Object`, a covariant one (`RenderClipRect` under
        // `RenderObject`) downcast, an erased bound narrowed to the body's
        // trait, an `Option` put on.
        final IrExpr passed = doubled
            ? (IrLiteral('${snake(from.name)}.flatten()', const IrType('raw'))
                ..rustType = flattened)
            : (IrLocal(from.name)..rustType = flattened);
        return expr(
          coerceInto(
            passed,
            _selfBound(_inThisClassTerms(p.type, via)),
            _world,
          ),
        );
      }
      // The override's own default is the value the base "has no value for".
      final fallback = p.defaultValue;
      if (fallback != null) return expr(fallback);
      // A `dynamic` (an `Object?`) has no value as the `Null` object.
      if (p.type.name == 'dynamic' && !p.type.nullable) {
        return 'dart_null_object()';
      }
      if (p.type.nullable) return 'None';
      throw Unsupported(
        'override widens `${method.name}` with `${p.name}`, '
            'which the base has no value for',
        '${cls.name}.${method.name}',
      );
    }).toList();
    final op = method.operator;
    if (op != null && operatorTraits.containsKey(op)) {
      if (op == 'unary-') return '-*self';
      return '*self $op ${args.single}';
    }
    // `Type::method(self, ...)`, not `self.method(...)`. Inside `impl Base for
    // This` the trait's own method has the same name, and `self.method(...)`
    // leans on Rust preferring the inherent one -- true today, and an infinite
    // recursion the moment the inherent one is not emitted. The explicit path
    // says which one is meant.
    // A setter's inherent name is `set_x` (see `_methodName`): the trait's
    // `set__status` forwarded to `Value::_status`, which is the getter.
    final name = op == null
        ? (method.isSetter ? 'set_${snake(method.name)}' : snake(method.name))
        : _operatorName(op);
    // An inherent method that takes `self: &Rc<Self>` (`_receiverOf`) is
    // reached from the trait's `&self` through the stored handle (1297
    // "expected `&Rc<X>`, found `&X`" at ws276).
    final receiver = cls.counted && _handles.contains(_rustName(method))
        ? '&self.__self.get()'
        : 'self';
    // A generic method's type parameters go along: the forwarder declares
    // the trait's, and the inherent one it reaches names its own only in
    // its result (`getElementForInheritedWidgetOfExactType<T>()`, 36
    // "cannot infer type of the type parameter `T`" at ws397).
    final generics = through?.typeParameters ?? method.typeParameters;
    final fish = generics.length == method.typeParameters.length
        ? _turbofish([for (final g in generics) IrType(g)])
        : '';
    final call =
        '${via == null ? '${cls.name}${_selfTurbofish()}' : _implementedAs(via)}::$name$fish(${[receiver, ...args].join(', ')})';
    // An inherent method the analysis typed `Never` (`throw
    // UnimplementedError()` for a body) returns `Result<Infallible, E>`;
    // the trait's signature wants its own `T`, which the impossible value
    // maps into (`_UnspecifiedTextScaler.clamp`, ws503).
    if (method.returnType.name == 'Never') {
      return '$call.map(|__never| match __never {})';
    }
    // An `async fn` yields its own future type; the trait wants the boxed
    // one every `Future<T>` is here (`_NativeCodec::get_next_frame(self)`).
    return call;
  }

  /// A lazy `late` field's accessor (`_lazyLate`): the field filled on
  /// the first read, through whatever handle holds the object. A read
  /// from another object (`it._paintOrderIterable` in `_TheaterParentData`)
  /// has no `self` to run the initialiser on, and read the empty cell
  /// (run653's render walk).
  void _emitLazyAccessors() {
    final wanted = _foreignReadsOf(cls.name);
    for (final f in _allFields(cls)) {
      // Only where some body reads it from outside: an accessor nobody
      // calls is a body that may not compile for nothing (+4 at ws654).
      if (!_lazyLate(f) || !wanted.contains(f.name)) continue;
      _member('${cls.name}.${f.name}', () {
        _here = '${cls.name}.${f.name}';
        final savedFailure = _failure;
        final savedReturns = _rustReturns;
        final savedAsync = _asyncBody;
        final savedParams = _methodTypeParams;
        final savedReassigned = _reassigned;
        _failure = _resultModel ? _error : null;
        _asyncBody = false;
        _methodTypeParams = const [];
        _reassigned = {};
        final held = _declSpelling(() => type(f.type));
        final returns = _wrapped(held);
        _rustReturns = returns;
        _line('pub fn ${_lazyAccessor(f.name)}(&self) -> $returns {');
        _indent++;
        final read = _lazyRead(f, 'self');
        _line(_failure != null ? 'Ok($read)' : read);
        _indent--;
        _line('}');
        _line('');
        _failure = savedFailure;
        _rustReturns = savedReturns;
        _asyncBody = savedAsync;
        _methodTypeParams = savedParams;
        _reassigned = savedReassigned;
      });
    }
  }

  static String _lazyAccessor(String field) => '__lazy_${snake(field)}';

  /// Whether a class or a base of it writes its own `hashCode`.
  bool _declaresHashCode(IrClass c) =>
      c.methods.any((m) => m.name == 'hashCode' && !m.isStatic) ||
      _abstractAncestors(
        c,
      ).any((a) => a.methods.any((m) => m.name == 'hashCode' && !m.isStatic)) ||
      _superclassChain(
        c,
      ).any((a) => a.methods.any((m) => m.name == 'hashCode' && !m.isStatic));

  /// The concrete superclasses above `c`, nearest first.
  Iterable<IrClass> _superclassChain(IrClass c) sync* {
    var name = c.superclass;
    final seen = <String>{};
    while (name != null && seen.add(name)) {
      final above = library[name];
      if (above == null) return;
      yield above;
      name = above.superclass;
    }
  }

  /// The fields of `owner` some body in the program reads on another
  /// object (`_WalkSelf.foreignFieldReads`), over every module's classes.
  Set<String> _foreignReadsOf(String owner) =>
      _foreignReads.putIfAbsent(owner, () {
        final found = <String>{};
        final classes = <IrClass>{
          ...library.elsewhere.values,
          ...library.classes,
        };
        for (final c in classes) {
          final reads = _foreignReadsIn(c)[owner];
          if (reads != null) found.addAll(reads);
        }
        return found;
      });

  final Map<String, Set<String>> _foreignReads = {};

  static final _foreignReadsOfClass = Expando<Map<String, Set<String>>>();

  static Map<String, Set<String>> _foreignReadsIn(IrClass c) {
    final cached = _foreignReadsOfClass[c];
    if (cached != null) return cached;
    final walk = _WalkSelf();
    for (final m in c.methods) {
      walk.statement(m.body);
    }
    for (final k in c.constructors) {
      final body = k.body;
      if (body != null) walk.statement(body);
    }
    for (final f in c.fields) {
      final init = f.initial;
      if (init != null) walk.expression(init);
    }
    return _foreignReadsOfClass[c] = walk.foreignFieldReads;
  }

  /// Whether `owner`'s field `name` is a lazy `late` (see `_lazyLate`),
  /// read through its accessor from outside.
  bool _lazyFieldOf(String owner, String name) {
    final owned = library[owner];
    if (owned == null) return false;
    for (final f in _allFields(owned)) {
      if (f.name != name) continue;
      return f.isLate &&
          f.initial != null &&
          _mentionsThis(f.initial!) &&
          _inCellOf(owned, f);
    }
    return false;
  }

  void _emitConstructors() {
    for (final ctor in cls.constructors) {
      // Through `_member`, like every other member. Without it an
      // `Unsupported` from one constructor came out of `_emitStruct` and took
      // the **whole class** with it -- 410 classes that vanished because one
      // field was `late`. That is round 21's lesson, at a site it never
      // reached: the unit of refusal has to be the unit of work.
      _member(
        '${cls.name}.${ctor.name ?? "new"}',
        () => _emitConstructor(ctor),
      );
    }
  }

  /// Whether a constructor is written as a `const fn`: see the note at
  /// `constness` in `_emitConstructor`. Through a redirect chain, each
  /// step's own rule and the target's (`seen` stops a cycle).
  bool _constCtor(IrConstructor ctor, Set<IrConstructor> seen) {
    if (!seen.add(ctor)) return false;
    if (!ctor.isConst ||
        ctor.body != null ||
        !ctor.params.every((p) => _isCopy(type(p.type))) ||
        ctor.fieldInits.values.any((e) => expr(e).contains('.clone()'))) {
      return false;
    }
    final redirect = ctor.redirectTo;
    if (redirect == null) return true;
    final target = cls.constructors
        .where((c) => (c.name ?? '') == redirect)
        .firstOrNull;
    return target != null && _constCtor(target, seen);
  }

  void _emitConstructor(IrConstructor ctor) {
    _here = '${cls.name}.${ctor.name?.isEmpty ?? true ? 'new' : ctor.name}';
    // Dart's named constructors are Rust's associated functions already --
    // `EdgeInsets.all(8)` and `EdgeInsets::all(8.0)` are the same call, and the
    // unnamed one is `new` by Rust's convention. Nothing has to be encoded, so
    // nothing is: this is one of the places the two languages simply agree.
    final name = _ctorName(ctor.name);
    _doc(ctor.doc);
    // A parameter the constructor assigns -- `cullRect ??= Rect.largest`
    // inside a field initialiser, or in the body -- is `mut` (E0384).
    final assigned = <String>{
      for (final init in ctor.fieldInits.values)
        ..._assignedIn(IrExprStmt(init)),
      if (ctor.body != null) ..._assignedIn(ctor.body!),
    };
    final params = ctor.params
        .map(
          (p) =>
              '${assigned.contains(p.name) ? 'mut ' : ''}${snake(p.name)}: ${type(p.type)}',
        )
        .join(', ');
    // ..and the body's locals are `mut` by the same reckoning (`let` in a
    // constructor body was never `mut` once locals stopped being so by
    // default: 4 E0384s in `ParagraphStyle`).
    _reassigned = assigned;
    _cellLocals = {};
    // `const fn` because the Dart constructor was `const`, which is what lets
    // the static constants below be associated consts rather than lazy statics.
    // `const fn` even when the constructor carries asserts. An earlier round
    // dropped `const` here, on the assumption that Rust would not accept a
    // `const fn` that could panic. That assumption was wrong -- const panic has
    // been stable since 1.57, `debug_assert!` inside a `const fn` compiles, and
    // the check still fires at runtime. Both were available all along.
    //
    // It mattered: `TextAlignVertical` has asserts in its constructor and
    // `static const` fields built from it, and dropping `const` made those
    // fields uncompilable. The two rounds' rules only met on real code.
    // A constructor with a body cannot be `const`: it builds the value into a
    // local and runs statements against it, and a `const fn` may not.
    // ..and one whose parameters are not all `Copy`: a `String` field is
    // initialised with `string.clone()` now, and a `const fn` may not call
    // it (E0015, 53 of them the round the clones arrived). The `static
    // const`s that needed `const fn` hold `Copy` values -- `Offset`,
    // `TextAlignVertical` -- and keep it.
    // ..nor one whose field initialisers clone -- `Color`, a `Copy` struct
    // the front end could not know is one, arrives as `color.clone()`.
    // ..and a redirecting one (`const BorderRadius.all(r) : this.only(..)`)
    // only when the constructor it hands its arguments to is one: a
    // `const fn` may not call a plain `fn` (run686).
    final constness = _constCtor(ctor, {}) ? 'const ' : '';
    // A counted class hands out a handle, not a value: everything that
    // holds one holds an `Rc`, so the constructor is where the first one is
    // made. A `const fn` cannot allocate, so a counted constructor is not one.
    final produces = cls.counted ? 'std::rc::Rc<Self>' : 'Self';
    final signatureAt = _out.length;
    _line(
      '${_vis(ctor.name ?? cls.name)}'
      '${cls.counted ? '' : constness}fn $name($params) -> ${_wrapped(produces)} {',
    );
    _indent++;
    // A value class registers its cast function as it is first made
    // (`dart_register`); a counted one does so in `dart_rc`.
    if (!cls.counted && constness.isEmpty) _line('dart_register::<Self>();');
    // A constructor fails like any function: its body's value is `Ok`.
    _failure = _resultModel ? _error : null;
    if (ctor.redirectTo == null) _line('Ok({');
    // This constructor's own temporaries first -- a `super(#t0)` passes them
    // -- and only then the base's, computed from them.
    for (final s in [...ctor.pre, ..._inheritedPre(ctor)]) {
      stmt(s);
    }
    final redirect = ctor.redirectTo;
    if (redirect != null) {
      // Everything this constructor does is hand its arguments to another one
      // of the same class. `Self::` because it is the same class; `_ctorName`
      // because the unnamed one is `new` here as it is above.
      final args = ctor.redirectArgs.map(expr).join(', ');
      _line('Self::${_ctorName(redirect.isEmpty ? null : redirect)}($args)');
      _indent--;
      _line('}');
      // The same clone check as a constructor with a body gets below: a
      // `Copy` argument the front end could not know is one arrives as
      // `radius.clone()` (`BorderRadius.all`, run686).
      if (constness.isNotEmpty &&
          _out.sublist(signatureAt + 1).any((l) => l.contains('.clone()'))) {
        _out[signatureAt] = _out[signatureAt].replaceFirst('const fn ', 'fn ');
      }
      _line('');
      return;
    }
    for (final check in ctor.asserts) {
      stmt(check);
    }
    final inits = {..._inheritedInits(ctor), ...ctor.fieldInits};
    // The handle is made around the value: a counted class's constructor is
    // the one place an `Rc` comes from, and everything that holds one after
    // that holds the handle.
    // A `late` field whose initialiser mentions `this` -- `late final
    // nativeFilter = _ImageFilter.matrix(this)` -- starts absent in the
    // literal and is written right after it, when `__new` exists to be
    // named. Not a `late` one: it has no absence to start from, and stays
    // refused below.
    final deferred = <String, IrExpr>{
      for (final field in _allFields(cls))
        if (field.isLate &&
            (inits[field.name] ?? field.initial) != null &&
            _mentionsThis((inits[field.name] ?? field.initial)!))
          field.name: (inits[field.name] ?? field.initial)!,
    };
    // The base constructors' bodies run too, deepest first, before this
    // one's: `BindingBase()` calls `initInstances()` and
    // `initServiceExtensions()` from its body, and no binding subclass
    // ran either until run441.
    final bases = _inheritedBodies(ctor);
    // `DART2RUST_TRACE_CTOR=<Class>`: the constructor chain a class's
    // constructor runs, with each body's statements, to stderr.
    if (Platform.environment['DART2RUST_TRACE_CTOR'] == cls.name) {
      String shape(IrStmt? body) => switch (body) {
        null => 'none',
        IrBlock(:final statements) =>
          statements
              .map(
                (s) =>
                    '${s.runtimeType}${s is IrExprStmt ? '(${s.expr.runtimeType})' : ''}',
              )
              .toList()
              .toString(),
        _ => body.runtimeType.toString(),
      };
      stderr.writeln(
        'TRACE_CTOR ${cls.name}.${ctor.name ?? 'new'} own=${shape(ctor.body)} '
        'bases=${[for (final (b, c, _) in bases) '${b.name}.${c.name ?? 'new'}:${shape(c.body)}']}',
      );
    }
    final built = ctor.body != null || deferred.isNotEmpty || bases.isNotEmpty;
    // A counted class is built *inside* its handle: the body's `this`
    // (`_recorder._canvas = this` in `_NativeCanvas`) is then the `Rc`
    // every holder wants, and the fields it writes are cells reached
    // through the handle just the same.
    final handleFirst = built && cls.counted;
    _line(
      !built
          ? (cls.counted ? 'dart_rc(Self {' : 'Self {')
          : handleFirst
          ? 'let __new = dart_rc(Self {'
          : 'let mut __new = Self {',
    );
    _indent++;
    if (cls.counted) _line('__self: DartSelf::new(),');
    for (final field in _allFields(cls)) {
      if (deferred.containsKey(field.name)) {
        _line(
          _inCell(field)
              ? '${snake(field.name)}: std::rc::Rc::new(std::cell::'
                    '${_isCopy(_heldDecl(field)) ? 'Cell' : 'RefCell'}'
                    '::new(None)),'
              : '${snake(field.name)}: None,',
        );
        continue;
      }
      // The constructor first, then the declaration's own value: Dart applies
      // the latter only where the former says nothing.
      var init = inits[field.name] ?? field.initial;
      if (init == null && field.type.nullable) {
        // A nullable Dart field with no initialiser *is* null. Rust needs the
        // value written down, and `None` is exactly it -- not a stand-in.
        // Into a projected `T?` slot (`<T as DartNullable>::Or`, a
        // `RestorableValue<T?>`'s `_value` seen from `RestorableEnumN<T>`)
        // the null crosses as every value does, by `from_option`
        // (`IrNullableOf`; a bare `None` was "expected associated type",
        // ws688).
        final absent = IrLiteral('null', const IrType('Null', nullable: true));
        init = field.type.projected
            ? IrNullableOf(absent, field.type.name, toOption: false)
            : absent;
      }
      if (init == null) {
        // Dart's `late`, which starts with no value at all. `None` is that,
        // and the reads unwrap. See `IrFieldDecl.isLate`.
        if (field.isLate) {
          _line(
            _inCell(field)
                ? '${snake(field.name)}: std::rc::Rc::new(std::cell::'
                      '${_isCopy(_heldDecl(field)) ? 'Cell' : 'RefCell'}'
                      '::new(None)),'
                : '${snake(field.name)}: None,',
          );
          continue;
        }
        // Not `late` and not nullable, so Dart guaranteed a value and this
        // compiler lost it -- a constructor it could not read, most often.
        throw Unsupported('field never initialised', field.name);
      }
      // A field whose declaration initialiser mentions `this`:
      // `late final nativeFilter = _ImageFilter.matrix(this)`. In Dart the
      // object already exists when that runs; in Rust the struct literal is
      // still being built and there is no `self` at all. 152 of these came
      // out as `*self` inside `Self { .. }`, which is not a thing.
      if (_mentionsThis(init)) {
        throw Unsupported(
          'a field initialised from `this`',
          '${cls.name}.${field.name}',
        );
      }
      final held = type(field.type);
      // A closure literal into a field of function type is an `Rc<dyn Fn>`
      // there, as a constant's is (see the statics): `DateFormat
      // .dateTimeConstructor` took a bare closure where the field's type
      // named the trait object.
      final rendered = field.type.isFunction && init is IrClosure && !init.boxed
          ? 'std::rc::Rc::new(${expr(init)})'
          : expr(init);
      // A `late` field is an `Option` (`_lateField`); one with an
      // initialiser that does not mention `this` starts with it, in
      // `Some` (`ObserverList._set = HashSet<T>()`, run434).
      final value = field.isLate ? 'Some($rendered)' : rendered;
      _line(
        _inCell(field)
            ? '${snake(field.name)}: std::rc::Rc::new(std::cell::'
                  '${_isCopy(held) ? 'Cell' : 'RefCell'}::new($value)),'
            : '${snake(field.name)}: $value,',
      );
    }
    // The phantom fields the struct declaration added. They hold nothing, and
    // leaving them out of the literal is a missing field rather than a
    // harmless omission.
    for (final unused in _unusedParameters(cls)) {
      _line('_phantom_${snake(unused)}: std::marker::PhantomData,');
    }
    _indent--;
    final body = ctor.body;
    if (!built) {
      _line(cls.counted ? '})' : '}');
    } else {
      _line(handleFirst ? '});' : '};');
      // `this` inside the body is the value being built, not a `self` that
      // does not exist yet. `_selfName` is the same lever a free function
      // uses, so the body's `this.x = v` comes out as `__new.x = v`.
      final saved = _selfName;
      _selfName = '__new';
      for (final entry in deferred.entries) {
        final field = _allFields(cls).firstWhere((f) => f.name == entry.key);
        // A lazy one stays absent: its first read fills it (`_lazyRead`).
        if (_lazyLate(field)) continue;
        final value = 'Some(${expr(entry.value)})';
        _line(
          _inCell(field)
              ? (_isCopy(_heldDecl(field))
                    ? '__new.${snake(field.name)}.set($value);'
                    : '*__new.${snake(field.name)}.borrow_mut() = $value;')
              : '__new.${snake(field.name)} = $value;',
        );
      }
      // Nested, nearest base outermost: a base's parameters are bound
      // from the arguments the class below it passed -- which name that
      // class's own parameters -- so the bindings go downward, one block
      // per base, each shadowing the last; the bodies run on the way back
      // out, deepest first, as Dart runs them. A flat block per base
      // evaluated `super(child)` where no `child` was bound (ws523).
      // A bodiless base at the far end binds nothing anyone reads: no
      // block for it (an empty `{ let h = h; }` in every value class's
      // `const fn`, ws531).
      final chain = bases.reversed.toList();
      while (chain.isNotEmpty && chain.last.$2.body == null) {
        chain.removeLast();
      }
      for (final (_, baseCtor, superArgs) in chain) {
        _line('{');
        _indent++;
        final assigned = baseCtor.body == null
            ? const <String>{}
            : _assignedIn(baseCtor.body!);
        for (var i = 0; i < baseCtor.params.length; i++) {
          // Typed by the parameter: an unused `None` inferred nothing
          // (`configuration` in `_ReusableRenderView`, E0282 at ws461).
          final p = baseCtor.params[i];
          _line(
            'let ${assigned.contains(p.name) ? 'mut ' : ''}${snake(p.name)}: ${type(_substituteType(p.type, _baseTypes(cls, const {})))} = ${expr(superArgs[i])};',
          );
        }
      }
      // ..the kept bases' bodies, innermost block first -- the ones the
      // blocks were opened for, not the deepest of `bases`: with bodiless
      // `Element` and `ComponentElement` trimmed off, `bases.take(kept)`
      // picked `DiagnosticableTree`'s (none) and `StatefulElement`'s
      // `state._element = this` never ran (run535: `State.widget` on a
      // `None`).
      for (final (_, baseCtor, _) in chain.reversed) {
        if (baseCtor.body != null) {
          final savedReassigned = _reassigned;
          _reassigned = {..._reassigned, ..._assignedIn(baseCtor.body!)};
          stmt(baseCtor.body!);
          _reassigned = savedReassigned;
        }
        _indent--;
        _line('}');
      }
      if (body != null) stmt(body);
      _selfName = saved;
      _line(handleFirst || !cls.counted ? '__new' : 'dart_rc(__new)');
    }
    _line('})');
    _indent--;
    _line('}');
    // A clone reached the body by a road the initialisers' check above
    // does not see (a super constructor's argument, a widened `Duration`):
    // a `const fn` may not call it (38 in `gestures_events` at ws278).
    if (constness.isNotEmpty &&
        _out.sublist(signatureAt + 1).any((l) => l.contains('.clone()'))) {
      _out[signatureAt] = _out[signatureAt].replaceFirst('const fn ', 'fn ');
    }
    _line('');
  }

  /// The class's `static final` fields, as module-level `LazyLock`s.
  ///
  /// Written outside the `impl` because Rust has no associated `static`, and
  /// named with the class in front so two classes' `defaults` do not collide.
  void _emitLazyStatics() {
    for (final constant in cls.constants) {
      if (!constant.isLazy) continue;
      _member('${cls.name}.${constant.name}', () {
        final held = type(constant.type);
        // Wrapped in `Isolate`, which is where "a Dart static is one per
        // isolate" is written down. A Rust `static` is one per process and so
        // must hold something `Sync`; `Box<dyn Fn(Image)>` is not, and that
        // was 94 `E0277`s. See the prelude for what the wrapper's `unsafe`
        // claims and when it stops being true.
        _doc(constant.doc);
        // Assignable, so a `RefCell` inside the `Isolate`: the same cell a
        // mutable top-level gets, read with `borrow` and written with
        // `borrow_mut` in `IrAssignStatic`.
        final cell = constant.isMutable ? 'std::cell::RefCell<$held>' : held;
        final made = constant.isMutable
            ? 'std::cell::RefCell::new(${constant.value is IrClosure && !(constant.value as IrClosure).boxed ? 'std::rc::Rc::new(${expr(constant.value)})' : expr(constant.value)})'
            : expr(constant.value);
        _line(
          '${_vis(constant.name)}static ${_lazyName(cls.name, constant.name)}: '
          'std::sync::LazyLock<Isolate<$cell>> = '
          'std::sync::LazyLock::new(|| Isolate($made));',
        );
        _line('');
      });
    }
  }

  void _emitConstants({String? prefix}) {
    for (final constant in cls.constants) {
      if (constant.isLazy) continue;
      // Each constant on its own: one that cannot be built is one constant
      // missing, not a class.
      _member(
        '${cls.name}.${constant.name}',
        () => _emitConstant(constant, prefix: prefix),
      );
    }
    if (cls.constants.isNotEmpty) _line('');
  }

  void _emitConstant(IrConstDecl constant, {String? prefix}) {
    if (!_constable(type(constant.type))) {
      throw Unsupported(
        'a `const` cannot hold a collection',
        '${cls.name}.${constant.name}',
      );
    }
    _doc(constant.doc);
    final spelled = prefix == null
        ? screamingSnake(constant.name)
        : screamingSnake('${prefix}_${constant.name}');
    _line(
      '${_vis(constant.name)}const $spelled: '
      '${type(constant.type)} = ${expr(constant.value)};',
    );
  }

  void _emitMethods() {
    for (final method in cls.methods) {
      if (method.operator != null) continue;
      if (method.isStatic && _freeStatics(cls.name)) continue;
      _member(
        '${cls.name}.${method.name}',
        () => _emitMethod(method),
        stub: (reason) => _emitMethod(method, stubbed: reason),
      );
    }
    // A concrete superclass's methods, on the subclass: `ValueNotifier
    // extends ChangeNotifier` has `ChangeNotifier`'s fields (flattened in)
    // and, in Dart, its methods -- `notifyListeners()` from `set value`. A
    // struct inherits nothing, so the body is emitted again here, over the
    // same field names. Only for an ancestor without type parameters (its
    // `T` is not this class's) and not overridden here.
    final have = <String>{
      for (final m in cls.methods) m.name,
      for (final f in _allFields(cls)) f.name,
    };
    for (final ancestor in _concreteAncestors()) {
      for (final method in ancestor.methods) {
        if (method.operator != null || method.isStatic) continue;
        // Nearest first: a name already seen is overridden below this one.
        if (!have.add(method.name)) continue;
        _member(
          '${cls.name}.${method.name} (from ${ancestor.name})',
          () => _emitMethod(method),
        );
      }
    }
  }

  /// The `extends` chain above this class, nearest first: the concrete,
  /// non-generic classes of this library whose methods a struct has to
  /// carry itself.
  List<IrClass> _concreteAncestors() {
    final out = <IrClass>[];
    var name = cls.superclass;
    final seen = <String>{cls.name};
    while (name != null && seen.add(name)) {
      final ancestor = library[name];
      if (ancestor == null || ancestor.isEnum) break;
      // Past an abstract ancestor, not stopped by it: `_SwitchPainter`
      // extends the abstract `ToggleablePainter`, which extends the
      // concrete `ChangeNotifier`, and `notifyListeners` is the latter's
      // (58 "no method named `notify_listeners`" in `cupertino`).
      if (ancestor.isAbstract) {
        name = ancestor.superclass;
        continue;
      }
      if (_generics(ancestor).isNotEmpty) break;
      out.add(ancestor);
      name = ancestor.superclass;
    }
    return out;
  }

  /// Whether the method being printed takes `&mut self` (`_sharedMutation`).
  /// A lending local function's closure has to reborrow one rather than
  /// move it (see `IrLocalFunction.lends`).
  var _selfIsMut = false;

  void _emitMethod(IrMethod method, {String? as, String? stubbed}) {
    _selfIsMut = !method.isStatic && _sharedMutation(method);
    {
      // A static `of<T>` inside `ScopedModel<T>`: Rust will not have the
      // name twice (E0403, the two errors outside any body once the
      // widgets crate passed). The signature is renamed and the body,
      // which would need the same rename, is a stub that says so.
      // ..an *instance* method's: a static one is a free function with
      // no class parameter in scope to collide with, and its body spells
      // its own `T` unrenamed (`InheritedModel.inheritFrom<T>`, the
      // `MediaQuery.maybeOf` every widget asks: run541; all 7 stubs of
      // this kind were statics).
      final renamed = method.isStatic ? null : _renamedShadowed(method);
      if (renamed != null) {
        method = renamed;
        stubbed ??=
            "a method whose type parameter shadows the class's: "
            '${cls.name}.${method.name}';
      }
      // Before the signature: whether a parameter needs `mut` is decided by the
      // body, and the signature is written first.
      _reassigned = _assignedIn(method.body);
      _mutRefParams = {
        for (final p in method.params)
          if (p.mutRef) p.name,
      };
      _cellLocals = {};
      _doc(method.doc);
      final params = [
        if (!method.isStatic) _receiverOf(method),
        // Parameters are a borrowed position: a function type there is
        // `impl Fn(..)`, which a closure literal can be passed to
        // directly, rather than `Box<dyn Fn(..)>`, which would need a
        // `Box::new` at every call site.
        ...method.params.map((p) => _param(p, owned: false)),
      ].join(', ');
      // A setter returns nothing: Dart's `set x(v)` has no return type, and
      // giving one a value would make `a.x = 1` an expression, which it is not.
      final returns = _returnType(method);
      _failure = _failureOf(method);
      _rustReturns = returns;
      _referenceParams = {
        for (final p in method.params)
          // Asked of the **emitted** type, not of the Dart name. `Object` is
          // the parameter of every `operator ==` and it is not one of this
          // package's abstract classes -- it is the prelude's trait -- so a
          // rule that consulted `library.isAbstract` missed all 251 of them
          // while `&dyn Object` was sitting in the signature. The same shape
          // as `_isCopy` two rounds ago: the ruler and its name disagreed.
          if (type(p.type, owned: false).startsWith('std::rc::Rc<dyn '))
            p.name: snake(p.name)
          // A counted class is an `Rc<Foo>` by value. The handle is not the
          // object, so the object is what gets asked.
          else if (library[p.type.name]?.counted ?? false)
            p.name: '&*${snake(p.name)}',
      };
      final name = as ?? _rustName(method);
      // An `async` method that translated is the body under `name__body`
      // and the spawning wrapper under `name` (`_emitAsyncWrapper`); one
      // that did not is the wrapper alone, panicking.
      final async = method.isAsync && stubbed == null;
      if (method.isAsync) {
        final mutable =
            !method.isStatic && _receiverOf(method).startsWith('&mut');
        final receiver = method.isStatic
            ? null
            : (
                'let ${mutable ? 'mut ' : ''}__self = ${_selfHandle()};',
                _selfIsHandle
                    ? '&__self'
                    : cls.counted
                    ? '&*__self'
                    : mutable
                    ? '&mut __self'
                    : '&__self',
              );
        if (stubbed != null) {
          _line(
            '${_vis(method.name)}fn $name${_generics(method)}($params) -> ${_futureOf(method)} {',
          );
          _indent++;
          _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
          _indent--;
          _line('}');
          _line('');
          return;
        }
        _emitAsyncWrapper(
          method,
          '${_vis(method.name)}fn $name${_generics(method)}($params) -> ${_futureOf(method)}',
          // A free static (an abstract class's) has no `Self` to go through
          // (E0433, 19 at ws465).
          method.isStatic && _freeStatics(cls.name)
              ? '${name}__body'
              : 'Self::${name}__body',
          receiver: receiver,
          turbofish: method.typeParameters.isEmpty
              ? ''
              : '::<${method.typeParameters.join(', ')}>',
        );
        _line('');
      }
      _line(
        '${_vis(method.name)}${async ? "async " : ""}fn '
        '${async ? '${name}__body' : name}${_generics(method)}($params) -> $returns {',
      );
      _indent++;
      _returns = method.returnType;
      _here = '${cls.name}.${method.name}';
      _asyncBody = method.isAsync;
      _methodTypeParams = method.typeParameters;
      // A failing `void` method that falls off its end still has to
      // produce its `Ok(())`: `_validateColorStops` ends in an `if`/`else`
      // that only ever returns `Err`, and the value of that `if` is `()`.
      // An async method's value is the awaited one: `Future<void>` falls
      // off into `Ok(())` too (54 in `widgets`).
      final produced = method.isAsync
          ? _awaited(method.returnType)
          : method.returnType;
      // The null the body's type falls into, the same one a closure's body
      // gets from `_body`: `()`, a `None` of the spelled `Option`, the
      // `Null` object of a `dynamic`. Only `()` was asked for here, so an
      // `async Future<dynamic>` whose body ends in an `if`/`else if` chain
      // -- `_handleTextInputInvocation`, `_handleUndoManagerInvocation`,
      // `_handlePlatformMessage` -- left the chain's `()` in the tail
      // position of a `Result<Rc<dyn Object>, ..>`, and a bare `return;`
      // inside one became `Ok(())` (ws874).
      final falling = _fallsOffValue(type(produced));
      final fallsOff =
          _failure != null && falling != null && !_alwaysReturns(method.body);
      final savedFalling = _fallsOff;
      _fallsOff = falling;
      if (stubbed != null) {
        _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
      } else {
        stmt(method.body, tail: !fallsOff);
        if (fallsOff) _line('Ok($falling)');
      }
      _fallsOff = savedFalling;
      // `TileMode` to text as an `if`/`else if` chain over every variant with
      // no final `else`: Dart lets the body fall off the end (returning null
      // it would then refuse at runtime); Rust wants the last `if` to be an
      // expression of the return type. The chain is exhaustive by the
      // author's reckoning, and the line after it says so.
      if (!fallsOff && stubbed == null) _closeOpenIf(method.body);
      _returns = null;
      _indent--;
      _line('}');
      _line('');
    }
  }

  /// After a body: the line that ends an open `if` chain, when the method
  /// has a value to return and the chain is how it returns it.
  void _closeOpenIf(IrStmt body) {
    final returns = _returns;
    if (returns == null || type(returns) == '()') return;
    if (_alwaysReturns(body) || !_endsInOpenIf(body)) return;
    _line('unreachable!("no branch of the if chain returned")');
  }

  /// Whether a body ends in an `if` chain that returns on every branch it
  /// has, and has no `else` to end it.
  bool _endsInOpenIf(IrStmt s) => switch (s) {
    IrBlock(:final statements) =>
      statements.isNotEmpty && _endsInOpenIf(statements.last),
    IrIf(:final then, :final otherwise) =>
      otherwise == null ? _alwaysReturns(then) : _endsInOpenIf(otherwise),
    _ => false,
  };

  void _emitOperators() {
    for (final method in cls.methods) {
      final op = method.operator;
      if (op == null) continue;
      _member('${cls.name} operator $op', () => _emitOperator(method, op));
    }
  }

  void _emitOperator(IrMethod method, String op) {
    {
      final mapping = operatorTraits[op];
      if (mapping == null) {
        // `~/` has no Rust trait. Emitted as an inherent method rather than
        // forced into one that means something else.
        // ..and as a method in every respect: it returns `Result` and
        // its body may `?`, as the trait's declaration of the same
        // operator does (`stdOperators`).
        _line('');
        _line('impl${_implGenerics(cls)} ${cls.name}${_generics(cls)} {');
        _indent++;
        _emitMethod(method, as: _operatorName(op));
        _indent--;
        _line('}');
        return;
      }
      final (trait, fn) = mapping;
      final rhs = method.params.isEmpty ? null : method.params.single;
      _line('');
      _doc(method.doc);
      final generic = rhs == null ? '' : '<${type(rhs.type)}>';
      _line(
        'impl${_implGenerics(cls)} std::ops::$trait$generic for '
        '${cls.name}${_generics(cls)} {',
      );
      _indent++;
      _line('type Output = ${type(method.returnType)};');
      _line('');
      final params = [
        'self',
        if (rhs != null) '${snake(rhs.name)}: ${type(rhs.type)}',
      ].join(', ');
      // The body lives in an inherent method the trait impl forwards to.
      // Inside `impl std::ops::Add for Matrix3`, the trait is in scope, and
      // `cascaded.add(arg)` in the body of `operator +` -- Dart's own
      // `add`, `&mut self` -- resolved to the by-value `Add::add` first:
      // 8 `E0382`s and an infinite recursion in vector_math.
      final own = _operatorName(method.operator!);
      _line(
        'fn $fn($params) -> Self::Output { '
        'Self::$own(${['self', if (rhs != null) snake(rhs.name)].join(', ')}) }',
      );
      _indent--;
      _line('}');
      _line('');
      _line('impl${_implGenerics(cls)} ${cls.name}${_generics(cls)} {');
      _indent++;
      _line('pub fn $own($params) -> ${type(method.returnType)} {');
      _indent++;
      _returns = method.returnType;
      _here = '${cls.name}.${method.name}';
      _asyncBody = method.isAsync;
      _methodTypeParams = method.typeParameters;
      _reassigned = _assignedIn(method.body);
      _mutRefParams = {
        for (final p in method.params)
          if (p.mutRef) p.name,
      };
      _cellLocals = {};
      // An operator's signature is `std::ops`'s and cannot say `Result`:
      // inside it a failing call unwraps.
      final savedFailure = _failure;
      _failure = null;
      // ..and takes `self` by value: `this` inside is `self`, not `*self`
      // (`Priority.operator -` doing `this + (-offset)`, E0614 at ws463).
      _selfByValue = true;
      final closed = _body(
        method.body,
        method.isAsync ? _awaited(method.returnType) : method.returnType,
      );
      _selfByValue = false;
      if (!closed) _closeOpenIf(method.body);
      _failure = savedFailure;
      _returns = null;
      _indent--;
      _line('}');
      _indent--;
      _line('}');
    }
  }

  /// A Rust-legal name for a Dart operator.
  ///
  /// The fallback used to be `op_` plus the code units, which turned `==` into
  /// `op_61_61` -- legal, but unreadable and unsearchable. Every operator Dart
  /// has is named here instead; anything genuinely unknown stops rather than
  /// being spelled in decimal.
  static String _operatorName(String op) => switch (op) {
    '+' => 'op_add',
    '-' => 'op_sub',
    '*' => 'op_mul',
    '/' => 'op_div',
    '%' => 'op_rem',
    'unary-' => 'op_neg',
    '~/' => 'int_div',
    '[]' => 'index_of',
    '[]=' => 'index_set',
    '==' => 'op_eq',
    '<' => 'lt',
    '>' => 'gt',
    '<=' => 'le',
    '>=' => 'ge',
    '&' => 'bit_and',
    '|' => 'bit_or',
    '^' => 'bit_xor',
    '~' => 'bit_not',
    '<<' => 'shl',
    '>>' => 'shr',
    '>>>' => 'ushr',
    // The name is quoted *and* described: an empty one said
    // "operator `` has no Rust name", 367 times, which names neither the
    // operator nor where it came from.
    '' => throw Unsupported('a member with no name', '<empty>'),
    _ => throw Unsupported('operator `$op` has no Rust name', op),
  };

  /// A Rust-legal identifier for any Dart member name.
  ///
  /// `superFn` pastes the name into another identifier, so an operator's own
  /// spelling cannot go through: `superFn('AlignmentGeometry', '==')` produced
  /// `alignment_geometry_super_`, a name with nothing on the end of it.
  static String _identifier(String name) =>
      // Any letters at all: `___sendPlatformMessage$Method$FfiNative`, the
      // AOT lowering of an `@Native` external, is a name for `snake` to
      // clean, not an operator, and refusing it took `PlatformDispatcher.
      // instance` with it (20 callers).
      _stdShadowed[name] ??
      (RegExp(r'[A-Za-z]').hasMatch(name) ? snake(name) : _operatorName(name));

  /// `dart:core` methods whose snake-cased name is an *unstable* inherent
  /// method of Rust's std, which outranks any trait's: spelled by the
  /// prelude's own name (`String.replaceFirst`, E0658 16 at ws465).
  static const _stdShadowed = {
    'replaceFirst': 'dart_replace_first',
    // `str::starts_with`/`ends_with` take a `Pattern`, which a `String`
    // is not (`_findFamilyWithVariantAssetPath`, ws579).
    'startsWith': 'dart_starts_with',
    'endsWith': 'dart_ends_with',
  };
}
