// Which members can fail, over the whole program.
//
// The Result model (STATUS: 决定 2026-09-04) puts `Result<T, Rc<dyn Object>>`
// on every member that can throw. Which those are is computed here, as a
// fixed point over the closed world: a member fails when its body throws
// outside a catch-all `try`, or calls a member that fails -- an instance
// call through every implementer of its interface target. This census
// prints the sizes before the analysis drives any code generation.
//
//   dart run bin/throws_census.dart app.dill "package:,dart:ui"
import 'dart:io';

import 'package:kernel/class_hierarchy.dart';
import 'package:kernel/core_types.dart';
import 'package:kernel/kernel.dart';

import '../lib/throws.dart';

void main(List<String> args) {
  final component = loadComponentFromBinary(args[0]);
  final prefixes = (args.length > 1 ? args[1] : 'package:,dart:ui').split(',');
  final hierarchy = ClassHierarchy(
    component,
    CoreTypes(component),
  ) as ClosedWorldClassHierarchy;
  final sw = Stopwatch()..start();
  final analysis = ThrowsAnalysis.of(component, hierarchy, prefixes);
  sw.stop();
  stdout.writeln('members considered: ${analysis.considered}');
  stdout.writeln('throw directly: ${analysis.direct.length}');
  stdout.writeln(
    'fail after propagation: ${analysis.failing.length} '
    '(${(100 * analysis.failing.length / analysis.considered).toStringAsFixed(1)}%)',
  );
  stdout.writeln(
    'closures/tear-offs that fail: ${analysis.failingFunctionValues}',
  );
  stdout.writeln('function-value calls seen: ${analysis.functionValueCalls}');
  stdout.writeln('rounds: ${analysis.rounds}, ${sw.elapsedMilliseconds} ms');
  final byPackage = <String, List<int>>{};
  for (final m in analysis.members) {
    final uri = m.enclosingLibrary.importUri.toString();
    final pkg = uri.startsWith('package:')
        ? uri.split('/').first
        : uri.split('/').first;
    final row = byPackage.putIfAbsent(pkg, () => [0, 0]);
    row[0]++;
    if (analysis.failing.contains(m)) row[1]++;
  }
  final rows = byPackage.entries.toList()
    ..sort((a, b) => b.value[1].compareTo(a.value[1]));
  for (final e in rows.take(20)) {
    stdout.writeln(
      '  ${e.value[1].toString().padLeft(6)} / ${e.value[0].toString().padLeft(6)}  ${e.key}',
    );
  }
}
