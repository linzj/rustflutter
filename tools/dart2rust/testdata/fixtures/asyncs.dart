// A fixture for `async` and `await`.
//
// The CFE does not desugar them: a dill carries `asyncMarker: Async` and an
// `await` still standing in the body. Rust has both words for the same things,
// so the translation is nearly one to one -- the only differences are that
// Rust writes `await` after the expression, and that a Rust `async fn`
// returning `T` already *is* a future, so the `Future<T>` Dart declared is the
// wrapper rather than the value and comes off the signature.
//
// What is *not* here is a runtime. Nothing in this file needs one, because
// none of these futures ever pend; the test drives them with four lines of
// `poll`. Flutter's do pend, and the executor that answers that is a separate
// job -- round 66 measured this whole layer at 106 functions.

class Asyncs {
  const Asyncs(this.factor);

  final double factor;

  Future<double> scaled(double x) async {
    return x * factor;
  }

  /// Awaits twice, so the two calls cannot be folded into one.
  Future<double> twice(double x) async {
    final double once = await scaled(x);
    return await scaled(once);
  }

  /// An `await` in the middle of a body, not in the return.
  Future<double> plus(double x, double y) async {
    final double got = await scaled(x);
    return got + y;
  }

  /// Returns nothing: `Future<void>` becomes a Rust `async fn` with no return
  /// type at all.
  Future<void> ignore(double x) async {
    await scaled(x);
  }
}
