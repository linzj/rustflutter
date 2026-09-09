part of '../backend_rust.dart';

// Free functions, erased twins, and the bodies super calls land in.
augment class RustBackend {
  void _emitFreeFunction(IrMethod method, {String? stubbed}) {
    _doc(method.doc);
    // Before the parameters are spelled: `_param` asks `_reassigned`
    // whether each is written, and it held the previous method's answer.
    _reassigned = _assignedIn(method.body);
    _mutRefParams = {
      for (final p in method.params)
        if (p.mutRef) p.name,
    };
    _cellLocals = {};
    final params = method.params.map((p) => _param(p, owned: false)).join(', ');
    final async = method.isAsync && stubbed == null;
    if (method.isAsync) {
      if (stubbed != null) {
        _line(
          '${_vis(method.name)}fn ${snake(method.name)}${_generics(method)}($params) -> ${_futureOf(method)} {',
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
        '${_vis(method.name)}fn ${snake(method.name)}${_generics(method)}($params) -> ${_futureOf(method)}',
        '${snake(method.name)}__body',
        turbofish: method.typeParameters.isEmpty
            ? ''
            : '::<${method.typeParameters.join(', ')}>',
      );
      _line('');
    }
    _line(
      '${_vis(method.name)}${async ? 'async ' : ''}fn '
      '${async ? '${snake(method.name)}__body' : snake(method.name)}${_generics(method)}'
      '($params) -> ${_returnType(method)} {',
    );
    _indent++;
    final saved = _selfName;
    // There is no receiver. Anything in the body that wanted one is a bug in
    // the front end, not something to paper over here.
    _selfName = '<no self>';
    _returns = method.returnType;
    _here = '${cls.name}.${method.name}';
    // The Rust return type too: a `try` body that returns carries
    // `Option<..>` of it out of its closure, and without it `_isLoopback`'s
    // `return address.isLoopback` came out as an `Option<()>`.
    _rustReturns = _returnType(method);
    _failure = _failureOf(method);
    _asyncBody = method.isAsync;
    _methodTypeParams = method.typeParameters;
    if (stubbed != null) {
      _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
    } else {
      if (!_body(
        method.body,
        method.isAsync ? _awaited(method.returnType) : method.returnType,
      )) {
        _closeOpenIf(method.body);
      }
    }
    _returns = null;
    _rustReturns = null;
    _selfName = saved;
    _indent--;
    _line('}');
    _line('');
  }

  /// The bodies of this abstract class's concrete methods, as free functions.
  ///
  /// Generic over the implementor and `?Sized`, so both the trait's own default
  /// and a subclass's override can call it -- the default has an unsized `Self`,
  /// and a subclass has a concrete one.
  /// Names whose free function could not be emitted.
  ///
  /// The trait's default for such a method cannot delegate to a function that
  /// does not exist, so it gets a `todo!()` instead -- the trait and every impl
  /// of it still line up, which a missing method would not.
  final _superFailed = <String>{};

  void _emitSuperFns() {
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      if (!_member(
        superFn(cls.name, method.name, isSetter: method.isSetter),
        () => _emitSuperFn(method),
      )) {
        _superFailed.add(method.name);
      }
    }
  }

  /// A generic method's type parameters read as `Object` (the erased
  /// twin's view; see the prelude's `CastErased`).
  Map<String, IrType> _erasure(IrMethod method) => {
    for (final p in method.typeParameters) p: const IrType('Object'),
  };

  String _erasedSignature(IrMethod method) {
    final erasure = _erasure(method);
    final params = [
      if (!method.isStatic) _sharedMutation(method) ? '&mut self' : '&self',
      ...method.params.map(
        (p) => _param(
          IrParam(
            p.name,
            _substituteType(p.type, erasure),
            named: p.named,
            hasDefault: p.hasDefault,
            kept: p.kept,
            mutRef: p.mutRef,
          ),
          owned: false,
        ),
      ),
    ].join(', ');
    final returns = _substituteType(method.returnType, erasure);
    final spelled = method.isAsync
        ? 'DartFuture<${type(_awaited(returns))}>'
        : _spelledReturn(type(returns));
    // The class's own parameters bounded as the trait's defaults bound
    // them (`_traitWhere`): the super function the default body reaches
    // asks `V: Clone` (`CanonicalizedMap.cast__erased`, ws483).
    final clauses = [
      for (final p in cls.typeParameters) '$p: Clone${_nbp(cls, p)}',
    ];
    final where = clauses.isEmpty ? '' : ' where ${clauses.join(', ')}';
    return 'fn ${_methodName(method)}__erased($params) -> ${_wrapped(spelled)}$where';
  }

  /// The erased twin of a generic trait method, in the trait: declared
  /// beside a required method, with the super function's body (its type
  /// parameters `Rc<dyn Object>`) beside a default one. Object-safe, so
  /// a `dyn` receiver reaches the method through it.
  void _emitErasedTwin(IrMethod method, {required bool defaultBody}) {
    if (method.typeParameters.isEmpty || method.isStatic) return;
    if (!defaultBody) {
      _line('${_erasedSignature(method)};');
      _line('');
      return;
    }
    _line('${_erasedSignature(method)} {');
    _indent++;
    final erased = method.typeParameters
        .map((_) => 'std::rc::Rc<dyn Object>')
        .join(', ');
    final spelled =
        '::<Self${[...cls.typeParameters].map((p) => ', $p').join()}, $erased>';
    final call =
        '${superFn(cls.name, method.name, isSetter: method.isSetter)}$spelled('
        '${['self', ...method.params.map((p) => snake(p.name))].join(', ')})';
    _line(method.isAsync && _resultModel ? 'Ok($call)' : call);
    _indent--;
    _line('}');
    _line('');
  }

  /// The erased twin in an implementer: through the class's own generic
  /// version, at `Rc<dyn Object>`.
  void _emitErasedImplTwin(IrMethod need, String trait) {
    if (need.typeParameters.isEmpty || need.isStatic) return;
    _line('${_erasedSignature(need)} {');
    _indent++;
    final erased = need.typeParameters
        .map((_) => 'std::rc::Rc<dyn Object>')
        .join(', ');
    _line(
      '<Self as $trait${_traitArgsOf(trait)}>::${_methodName(need)}::<$erased>'
      '(${['self', ...need.params.map((p) => snake(p.name))].join(', ')})',
    );
    _indent--;
    _line('}');
    _line('');
  }

  /// `where Self: Sized` for a generic method on a trait, or nothing.
  ///
  /// `RenderObject.invokeLayoutCallback<T extends Constraints>` is generic,
  /// and a generic method makes a trait dyn-incompatible -- so it used to be
  /// refused, on the reading that emitting it "would take `dyn RenderObject`
  /// away from the whole layer". That reading had a hole in it: Rust leaves a
  /// `where Self: Sized` method **out of the vtable**, so the trait stays
  /// dyn-compatible and every concrete implementor still has the method. It
  /// is the bound the standard library puts on `Iterator::by_ref` and friends
  /// for exactly this reason.
  ///
  /// What is given up is calling it *through* a trait object, which Dart does
  /// allow. That call is a refusal of its own where it happens, rather than
  /// 302 members deleted where they are declared.
  // A type parameter, or an `impl Future` parameter -- which is a type
  // parameter in a coat -- keeps a method out of the vtable, and a trait
  // used as `dyn` needs it kept out: `TransitionRoute` was "not dyn
  // compatible" for `_setSecondaryAnimation(.., Future<void>? disposed)`.
  /// A trait default method's `where`: `Self: Sized` when it needs it,
  /// and `T: Clone` for the class's parameters, which the super function
  /// holding its body asks for (see the trait header).
  String _traitWhere(IrMethod method) {
    final clauses = [
      if (_sizedBound(method).isNotEmpty) 'Self: Sized',
      for (final p in cls.typeParameters) '$p: Clone${_nbp(cls, p)}',
    ];
    return clauses.isEmpty ? '' : ' where ${clauses.join(', ')}';
  }

  static String _sizedBound(IrMethod method) =>
      method.typeParameters.isEmpty ? '' : ' where Self: Sized';

  /// A method whose type parameter has the same name as one of the class's.
  ///
  /// Dart allows the shadowing -- `Element.findAncestorStateOfType<T>` inside
  /// a `State<T>` -- and Rust does not: 44 `E0403`, all of them `T` inside a
  /// `T`. Renaming it would mean renaming it in the body too, which is a
  /// substitution this backend does not do, so the member is refused and says
  /// which name collided.
  void _refuseShadowedGeneric(IrMethod method) {
    for (final p in method.typeParameters) {
      if (cls.typeParameters.contains(p)) {
        throw Unsupported(
          "a method whose type parameter shadows the class's",
          '${cls.name}<$p>.${method.name}<$p>',
        );
      }
    }
  }

  /// Whether a super fn's body is being printed: `this` is a `&__Self`
  /// there, and so is `this` inside a closure of it, whose `_selfName` is
  /// the handle (`<Self as RendererBinding>` in `initMouseTracker`'s
  /// closure, E0411, run459).
  var _inSuperFn = false;

  void _emitSuperFn(IrMethod method) {
    final wasSuperFn = _inSuperFn;
    _inSuperFn = true;
    try {
      _emitSuperFnBody(method);
    } finally {
      _inSuperFn = wasSuperFn;
    }
  }

  void _emitSuperFnBody(IrMethod method) {
    {
      _line('');
      _line('/// The body of `${cls.name}.${method.name}`, reachable from an');
      _line('/// override the way Dart\'s `super.${method.name}` is.');
      final params = [
        // The body writes fields through `this_` when the method is one of
        // this class's mutating ones (or the trait's, for every class).
        // `&__Self` always: a write to a field in here goes through the
        // setter the trait declares, on `&self` (typed_data, 7 mismatches
        // once the trait's defaults went back to `&self`).
        'this_: &__Self',
        ...method.params.map(
          // `mut` when the body assigns it (`start = index + 1` in a loop).
          // A lent place (`IrParam.mutRef`) as the trait declares it.
          (p) =>
              '${_assignedIn(method.body).contains(p.name) ? 'mut ' : ''}'
              '${snake(p.name)}: ${p.mutRef ? '&mut ${type(p.type, owned: true)}' : type(p.type, owned: false)}',
        ),
      ].join(', ');
      // ..and by every trait a `super` call inside reaches that this
      // class is not below: a mixin's `super.initInstances()` dispatches
      // to the previous mixin of the application (`_realOwner`), which
      // its `on` clause never named (`SchedulerBinding`'s reaching
      // `GestureBinding`'s, 3 stubs on the start path at run448).
      final reached = _WalkSelf()..statement(method.body);
      final superBounds = [
        for (final MapEntry(key: base, value: arguments)
            in reached.superBases.entries)
          if (base != cls.name &&
              base != 'Object' &&
              _world.isTrait(base) &&
              !_world.isBelow(cls.name, base))
            ' + $base${arguments.isEmpty ? _traitArgsOf(base) : '<${arguments.map(type).join(', ')}>'}',
      ].join();
      final generics =
          '<__Self: ${cls.name}${_generics(cls)}$superBounds + ?Sized + \'static'
          '${cls.typeParameters.isEmpty ? '' : ', ${cls.typeParameters.map((p) => "$p: Clone${_nbp(cls, p)} + 'static").join(', ')}'}'
          '${method.typeParameters.isEmpty ? '' : ', ${method.typeParameters.map((p) => "$p: Clone${_nbm(method)} + 'static").join(', ')}'}'
          '>';
      final name = superFn(cls.name, method.name, isSetter: method.isSetter);
      if (method.isAsync) {
        // The wrapper holds the object through the trait's own handle
        // (`dart_self_<trait>()`, an `Rc<dyn Trait>`), and the body runs
        // on that: `__Self` there is the trait object.
        _emitAsyncWrapper(
          method,
          '${_vis(cls.name)}fn $name$generics($params) -> ${_futureOf(method)}',
          '${name}__body',
          receiver: (
            'let __self = this_.dart_self_${snakeRaw(cls.name)}();',
            '&*__self',
          ),
          turbofish:
              '::<_${cls.typeParameters.isEmpty ? '' : ', ${cls.typeParameters.join(', ')}'}${method.typeParameters.isEmpty ? '' : ', ${method.typeParameters.join(', ')}'}>',
        );
        _line('');
      }
      _line(
        '${_vis(cls.name)}${method.isAsync ? 'async ' : ''}fn '
        '${method.isAsync ? '${name}__body' : name}'
        '$generics($params) -> '
        // An `async fn` returns the awaited type. A boxed future returned by
        // a non-async one borrows `this_`: `+ '_`.
        '${_lifetimed(_returnType(method))} {',
      );
      _indent++;
      _selfName = 'this_';
      _returns = method.returnType;
      // ..and the Rust spelling, which a `try` that returns from inside
      // carries out through its closure (`Option<()>` carried an
      // `Rc<dyn Element>` in `inflateWidget`'s super function, ws475).
      final outerRustReturns = _rustReturns;
      _rustReturns = _returnType(method);
      _here = '${cls.name}.${method.name}';
      // A super function fails like the method whose body it holds.
      _failure = _failureOf(method);
      _asyncBody = method.isAsync;
      _methodTypeParams = method.typeParameters;
      _reassigned = _assignedIn(method.body);
      _mutRefParams = {
        for (final p in method.params)
          if (p.mutRef) p.name,
      };
      _cellLocals = {};
      // `this_` is a `&__Self: Trait`, and a trait has no fields: the base's
      // fields are its accessor methods here, as they are inside the trait
      // itself. `this_.start` was read as a field 6 times in `source_span`.
      final accessors = _fieldsAreAccessors;
      _fieldsAreAccessors = true;
      if (!_body(
        method.body,
        method.isAsync ? _awaited(method.returnType) : method.returnType,
      )) {
        _closeOpenIf(method.body);
      }
      _fieldsAreAccessors = accessors;
      _rustReturns = outerRustReturns;
      _returns = null;
      _selfName = 'self';
      _indent--;
      _line('}');
    }
  }

  /// A method's name, with Dart's operators mapped onto Rust's trait methods
  /// where one exists. Inside a trait there is no `impl std::ops::Add` to hang
  /// them on, so they become ordinary named methods.
  String _methodName(IrMethod method) {
    final op = method.operator;
    // A getter and a setter of the same Dart name are two members there and
    // one name here. The inherent path has always prefixed the setter; the
    // trait impls had not, so a mixin carrying `Ticker? get _ticker` beside
    // `set _ticker(v)` put two `fn _ticker` in one impl -- 839 `E0201`s.
    if (method.isSetter) return 'set_${snake(method.name)}';
    if (op == null) return snake(method.name);
    final mapping = operatorTraits[op];
    return mapping == null ? _operatorName(op) : 'op_${mapping.$2}';
  }

  /// A parameter's declaration, `mut` when the body reassigns it.
  ///
  /// Dart parameters are ordinary variables and get reassigned freely; Rust
  /// parameters are immutable unless the declaration says otherwise, and
  /// `mut x: f32` is where that is said. Without it,
  /// `shadow(start) { start = start + 1; }` emitted an assignment to something
  /// that cannot be assigned.
}
