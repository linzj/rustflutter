part of '../backend_rust.dart';

// Failure in the return value: which signatures carry `Result`.
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
}
