/// `super` into a Dart 3 `mixin class`.
///
/// A `mixin class` can be extended *and* mixed in, and the CFE treats it the
/// way it treats a mixin: the declaration is left hollow and each application
/// gets a copy of the bodies. But in Kernel it is a `Class` with
/// `isMixinDeclaration == false`, and this compiler asked that flag before
/// looking for the body -- so a `super` call into one was refused as "not
/// translated" even though thirteen applications carried the body.
///
/// `abstract mixin class WidgetsBindingObserver` in Flutter is the shape, and
/// `_WidgetsAppState.didChangeAppLifecycleState` -- which calls
/// `super.didChangeAppLifecycleState(state)` -- was the refusal (ws1063).
///
/// The question to ask is not how the class was spelled but whether the CFE
/// applied it anywhere.
library;

abstract mixin class Recorder {
  final List<String> log = <String>[];

  void record(String what) {
    log.add('base:$what');
  }

  String get seen => log.join(',');
}

/// Mixed in, and overriding through `super`.
class Loud with Recorder {
  @override
  void record(String what) {
    super.record(what.toUpperCase());
  }
}

/// Mixed in without overriding, so the default body is reached directly.
class Plain with Recorder {}

String use() {
  final List<String> out = <String>[];

  final Loud loud = Loud();
  loud.record('a');
  loud.record('b');
  out.add(loud.seen);

  final Plain plain = Plain();
  plain.record('c');
  out.add(plain.seen);

  return out.join('|');
}
