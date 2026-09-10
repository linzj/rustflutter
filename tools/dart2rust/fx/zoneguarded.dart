/// `runZonedGuarded(body, onError)` is a guard, not a zone.
///
/// With one zone there is nothing for the *zone* half to do -- `Zone.run`
/// already calls the callback directly. The guard half is what the program
/// depends on: `dart:ui`'s `_invoke1WithReturn` runs a platform callback
/// under it, so an error out of a hit-test handler reaches the zone's
/// uncaught-error hook instead of unwinding into the engine.
///
/// The three facts this pins down are the ones a caller can see: the value
/// comes back when the body returns, `null` comes back when it throws, and
/// `onError` is called in between with the error itself -- the same object,
/// not a copy or a string of it.
library;

import 'dart:async';

class Marker {
  const Marker(this.tag);
  final String tag;
  @override
  String toString() => 'Marker($tag)';
}

String use() {
  final List<String> log = <String>[];

  final int? value = runZonedGuarded<int>(() => 41 + 1, (
    Object e,
    StackTrace s,
  ) {
    log.add('unexpected');
  });
  log.add('value=$value');

  // The throwing body still has a `return` in it, and that is not padding:
  // a Dart body that only throws gives the Rust closure no return type to
  // infer, and `R` is then only reachable backwards through `<R as
  // DartNullable>::Or`, which rustc will not invert. Every call the gallery
  // makes has a returning path (`dart:ui`'s `_invoke1WithReturn` returns
  // `callback(arg1)`), so this is the shape under test; the other shape is
  // written down as a boundary rather than pretended away.
  final int? thrown = runZonedGuarded<int>(
    () {
      if (log.isNotEmpty) {
        throw const Marker('boom');
      }
      return 0;
    },
    (Object e, StackTrace s) {
      log.add('caught=$e');
    },
  );
  log.add('thrown=$thrown');

  // The body runs before the guard decides anything, and a body that returns
  // after doing work still returns its value.
  final String? built = runZonedGuarded<String>(
    () {
      log.add('ran');
      return 'ok';
    },
    (Object e, StackTrace s) {
      log.add('unexpected');
    },
  );
  log.add('built=$built');

  return log.join('|');
}
