// A test harness the size of the thing it tests.
//
// `package:test` is not used here on purpose: it needs a `pubspec.yaml`, and a
// pubspec makes `dart run` do an implicit `pub get` that overwrites the
// hand-written `.dart_tool/package_config.json` the compiler needs to start
// (`bin/devsetup.py` says why). These tests import `../lib/...` the way
// `bin/` already does, so they run with no resolution at all:
//
//     dart test/coerce_test.dart
//     bin/check.sh                 # analyzer and every test
library;

import 'dart:io';

int _passed = 0;
final List<String> _failures = [];
String _group = '';

/// Names the rule the following checks are about; printed on failure.
void group(String name) => _group = name;

/// Fails the run unless [actual] equals [expected].
void expect(Object? actual, Object? expected, String what) {
  if (_equal(actual, expected)) {
    _passed++;
    return;
  }
  _failures.add(
    '$_group: $what\n    expected: $expected\n    actual:   $actual',
  );
}

void expectTrue(bool actual, String what) => expect(actual, true, what);

bool _equal(Object? a, Object? b) {
  if (a is Set && b is Set) return a.length == b.length && a.containsAll(b);
  if (a is Iterable && b is Iterable) {
    final x = a.toList(), y = b.toList();
    return x.length == y.length &&
        Iterable.generate(x.length).every((i) => _equal(x[i], y[i]));
  }
  if (a is Map && b is Map) {
    return a.length == b.length &&
        a.keys.every((k) => b.containsKey(k) && _equal(a[k], b[k]));
  }
  return a == b;
}

/// Prints the result and exits non-zero if anything failed.
Never report(String suite) {
  if (_failures.isEmpty) {
    stdout.writeln('$suite: $_passed checks OK');
    exit(0);
  }
  stdout.writeln('$suite: ${_failures.length} FAILED, $_passed OK');
  for (final f in _failures) {
    stdout.writeln('  $f');
  }
  exit(1);
}
