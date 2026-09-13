/// A generic method returning `Future<T?>` through `then` on a wider
/// future, with `T?` as the callback's result.
///
/// `NavigatorState.pushNamed<T>` is `push<Object?>(route).then((Object?
/// result) => result as T?)`. The call's type argument for `then` was
/// spelled at the edge, `<T as DartNullable>::Or`, while the closure's
/// body hands back the plain `Option<T>` every body works with:
/// "`FutureOr<Option<T>>: IntoFutureOr<<T as DartNullable>::Or>` is not
/// satisfied" (1 stub since ws1098).
library;

import 'dart:async';

final List<String> log = <String>[];

class Stack {
  final List<Object?> results = <Object?>[];

  Future<Object?> push(Object? value) {
    results.add(value);
    log.add('push:$value');
    return Future<Object?>.value(value);
  }

  Future<T?> pushNamed<T extends Object?>(String name, Object? value) {
    log.add('named:$name');
    return push(value).then((Object? result) => result as T?);
  }
}

String use() {
  final Stack s = Stack();
  s.pushNamed<int>('a', 3);
  s.pushNamed<String>('b', null);
  // The futures resolve later; the calls are the compile's pin.
  return '${log.join(',')}|${s.results.length}';
}
