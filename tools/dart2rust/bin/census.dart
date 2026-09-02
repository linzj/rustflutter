// What is dart2rust actually unable to translate, ranked?
//
// The compiler reports what stopped it, one class at a time. That is the right
// thing when you are porting a class and the wrong thing when you are deciding
// what to build next: `EdgeInsets` refusing thirteen times is one data point,
// and the question is whether those thirteen are thirteen problems or one
// problem thirteen times.
//
// So this runs the front end over a whole tree and groups the refusals by kind.
// The top row is the next thing to build. It is the same instrument as
// tools/depth.py, pointed at the compiler instead of at the hand port.
//
//     dart run --packages=<flutter>/.dart_tool/package_config.json \
//         tools/dart2rust/bin/census.dart <dir-or-file> [--limit N] [--examples]

import 'dart:io';

import 'package:analyzer/dart/analysis/analysis_context_collection.dart';
import 'package:analyzer/dart/analysis/results.dart';
import 'package:analyzer/dart/ast/ast.dart';
import 'package:path/path.dart' as p;

import '../lib/backend_rust.dart';
import '../lib/frontend.dart';
import '../lib/ir.dart';

/// Collapses a refusal to the thing that has to be built to remove it.
///
/// `unsupported statement AssertStatementImpl: assert(a)` and the same for
/// `assert(b)` are one job, not two. The message's own head is the category;
/// the source that followed it is the example.
String category(String refusal) {
  final colon = refusal.indexOf(': ');
  final head = colon == -1 ? refusal : refusal.substring(0, colon);
  return head.replaceFirst('unsupported ', '');
}

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    stderr.writeln('usage: census.dart <dir-or-file> [--limit N] [--examples]');
    exit(2);
  }
  final root = p.normalize(p.absolute(args[0]));
  final showExamples = args.contains('--examples');
  var limit = 25;
  String? emitDir;
  for (var i = 1; i < args.length - 1; i++) {
    if (args[i] == '--limit') limit = int.parse(args[i + 1]);
    if (args[i] == '--emit-dir') emitDir = args[i + 1];
  }
  if (emitDir != null) Directory(emitDir).createSync(recursive: true);

  final files = <String>[];
  final entity = FileSystemEntity.typeSync(root);
  if (entity == FileSystemEntityType.directory) {
    for (final f in Directory(root).listSync(recursive: true)) {
      if (f is File && f.path.endsWith('.dart')) files.add(f.path);
    }
    files.sort();
  } else {
    files.add(root);
  }

  // One collection for the whole run: resolution is expensive to set up and
  // cheap to reuse, and a per-file collection turns a two-minute census into
  // an hour of the same work repeated.
  final collection = AnalysisContextCollection(includedPaths: [root]);

  final counts = <String, int>{};
  final examples = <String, String>{};
  var classesSeen = 0;
  var classesClean = 0;
  var membersTranslated = 0;
  var filesFailed = 0;
  final cleanClasses = <String>[];

  final started = DateTime.now();
  for (final file in files) {
    ResolvedUnitResult resolved;
    try {
      final session = collection.contextFor(file).currentSession;
      final result = await session.getResolvedUnit(file);
      if (result is! ResolvedUnitResult) {
        filesFailed++;
        continue;
      }
      resolved = result;
    } catch (_) {
      filesFailed++;
      continue;
    }

    for (final declaration in resolved.unit.declarations) {
      if (declaration is! ClassDeclaration) continue;
      final name = declaration.name.lexeme;
      if (name.startsWith('_')) continue;
      classesSeen++;

      final (cls, refused) = Frontend(name).lowerClass(declaration);
      String? rust;
      try {
        rust = RustBackend(cls, library: IrLibrary([cls])).emit();
      } on Unsupported catch (error) {
        refused.add('$error');
      } catch (error) {
        refused.add('backend crash: $error');
      }
      // Written out so the caller can check it *parses*. "Nothing was refused"
      // and "the output is Rust" turned out to be very different claims:
      // `CupertinoApp` refuses nothing and emits
      // `Option<Route<dynamic>? Function(RouteSettings)?>` as a parameter type.
      if (emitDir != null && rust != null && refused.isEmpty) {
        File('$emitDir/$name.rs').writeAsStringSync(rust);
      }
      // Constructors count. Without them this round's work -- four named
      // constructors on EdgeInsets alone -- moved the number not at all, and a
      // ruler that cannot see the work is a ruler that will be ignored.
      membersTranslated +=
          cls.methods.length + cls.constants.length + cls.constructors.length;

      if (refused.isEmpty) {
        classesClean++;
        if (cleanClasses.length < 40) cleanClasses.add(name);
      }
      for (final refusal in refused) {
        final key = category(refusal);
        counts[key] = (counts[key] ?? 0) + 1;
        examples.putIfAbsent(key, () {
          final colon = refusal.indexOf(': ');
          final source = colon == -1 ? refusal : refusal.substring(colon + 2);
          return source.length > 76 ? '${source.substring(0, 76)}...' : source;
        });
      }
    }
  }
  final elapsed = DateTime.now().difference(started);

  final ranked = counts.entries.toList()
    ..sort((a, b) => b.value.compareTo(a.value));

  print('${files.length} files, $classesSeen public classes, '
      '${elapsed.inSeconds}s'
      '${filesFailed > 0 ? ", $filesFailed did not resolve" : ""}');
  print('$classesClean translated with no refusal, '
      '$membersTranslated members emitted');
  print('');
  print('${'count'.padLeft(6)}  what has to be built');
  for (final entry in ranked.take(limit)) {
    print('${entry.value.toString().padLeft(6)}  ${entry.key}');
    if (showExamples) print('        e.g. ${examples[entry.key]}');
  }
  print('');
  print('${ranked.length} distinct blockers, '
      '${counts.values.fold(0, (a, b) => a + b)} refusals total');
  if (cleanClasses.isNotEmpty) {
    print('');
    print('clean: ${cleanClasses.join(", ")}');
  }
}
