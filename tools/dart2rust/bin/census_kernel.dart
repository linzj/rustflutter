// The census, run over a `.dill` instead of over source.
//
// `bin/census.dart` measures the analyzer front end. That was the only front
// end when it was written, and it is no longer the one a release would use --
// so the queue it prints is the queue for the wrong compiler. The two front
// ends share an IR and a backend, but they do **not** see the same program:
// Kernel arrives with mixins applied, super calls resolved and constants
// evaluated, so a blocker that is real for one can be absent for the other.
// Only measuring both says which.
//
//     dart run --packages=<kernel config> \
//         tools/dart2rust/bin/census_kernel.dart app.dill [package:flutter/] \
//         [--limit N] [--examples]
//
// Build the dill and the config with `bin/dill.py`.

import 'dart:io';

import 'package:kernel/ast.dart';
import 'package:kernel/kernel.dart';

import '../lib/backend_rust.dart';
import '../lib/frontend_kernel.dart';
import '../lib/ir.dart';

/// Collapses a refusal to the thing that has to be built to remove it.
String category(String refusal) {
  final colon = refusal.indexOf(': ');
  final head = colon == -1 ? refusal : refusal.substring(0, colon);
  return head.replaceFirst('unsupported ', '');
}

/// Top-level constants are a library's, not a class's, and are counted apart.
int _topLevel(IrLibrary library) => library.constants.length;

void main(List<String> args) {
  if (args.isEmpty) {
    stderr.writeln(
      'usage: census_kernel.dart <app.dill> [uri prefix] '
      '[--limit N] [--examples]',
    );
    exit(2);
  }
  final dill = args[0];
  final prefix = args.length > 1 && !args[1].startsWith('--')
      ? args[1]
      : 'package:flutter/';
  final showExamples = args.contains('--examples');
  var limit = 25;
  for (var i = 1; i < args.length - 1; i++) {
    if (args[i] == '--limit') limit = int.parse(args[i + 1]);
  }

  final started = DateTime.now();
  final component = loadComponentFromBinary(dill);

  final counts = <String, int>{};
  final examples = <String, String>{};
  var libraries = 0;
  var classesSeen = 0;
  var classesClean = 0;
  var membersEmitted = 0;

  for (final library in component.libraries) {
    if (!library.importUri.toString().startsWith(prefix)) continue;
    libraries++;

    // Lowered class by class rather than through `lowerLibrary`, so a refusal
    // is attributed to its class without parsing it back out of a string.
    // The first version of this did parse it, and got the *class name* as the
    // category for every row -- a ruler printing the names of the classes it
    // could not translate, sorted by how badly.
    final frontend = KernelFrontend(library);
    final all = <String>[];
    final lowered = <IrClass>[];
    for (final node in library.classes) {
      if (node.isAnonymousMixin) continue;
      // `lowerLibrary` guards each class, and this does not go through it. A
      // class whose *header* cannot be lowered -- `extends Foo<Never>` is the
      // one that found this -- is a refusal to count, not the end of the run.
      // Without the guard one class ended the measurement instead of appearing
      // in it, and the queue it would have printed was never printed at all.
      final (IrClass, List<String>) result;
      try {
        result = frontend.lowerClass(node);
      } on Unsupported catch (error) {
        classesSeen++;
        all.add('${node.name}: $error');
        continue;
      }
      final (cls, problems) = result;
      lowered.add(cls);
      classesSeen++;
      membersEmitted +=
          cls.methods.length +
          cls.constants.length +
          cls.constructors.length +
          cls.values.length;
      final backend = RustBackend.emitLibrary(IrLibrary([cls]));
      final classRefusals = [...problems, ...backend.$2];
      if (classRefusals.isEmpty) classesClean++;
      all.addAll(classRefusals);
    }

    for (final refusal in all) {
      final key = category(refusal);
      counts[key] = (counts[key] ?? 0) + 1;
      examples.putIfAbsent(key, () {
        final colon = refusal.indexOf(': ');
        final source = colon == -1 ? refusal : refusal.substring(colon + 2);
        return source.length > 76 ? '${source.substring(0, 76)}...' : source;
      });
    }
  }
  final elapsed = DateTime.now().difference(started);

  final ranked = counts.entries.toList()
    ..sort((a, b) => b.value.compareTo(a.value));

  print(
    '$prefix: $libraries libraries, $classesSeen classes, '
    '${elapsed.inSeconds}s',
  );
  print(
    '$classesClean classes with no refusal, '
    '$membersEmitted members emitted',
  );
  print('');
  print('${'count'.padLeft(6)}  what has to be built');
  for (final entry in ranked.take(limit)) {
    print('${entry.value.toString().padLeft(6)}  ${entry.key}');
    if (showExamples) print('        e.g. ${examples[entry.key]}');
  }
  print('');
  print(
    '${ranked.length} distinct blockers, '
    '${counts.values.fold(0, (a, b) => a + b)} refusals total',
  );
}
