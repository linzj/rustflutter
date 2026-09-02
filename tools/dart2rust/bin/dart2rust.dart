// The driver: resolve a file, lower one class, emit Rust.
//
//     dart run --packages=<flutter>/.dart_tool/package_config.json \
//         tools/dart2rust/bin/dart2rust.dart <file.dart> <ClassName> [-o out.rs]
//
// What it refuses to translate it *reports*, with the source that stopped it.
// A compiler that quietly emits something for input it did not understand is
// worse than one that stops, because its output compiles.

import 'dart:io';

import 'package:analyzer/dart/analysis/analysis_context_collection.dart';
import 'package:analyzer/dart/analysis/results.dart';
import 'package:analyzer/dart/ast/ast.dart';
import 'package:analyzer/dart/element/element.dart';
import 'package:path/path.dart' as p;

import '../lib/backend_rust.dart';
import '../lib/frontend.dart';
import '../lib/ir.dart';

Future<void> main(List<String> args) async {
  if (args.length < 2) {
    stderr.writeln('usage: dart2rust <file.dart> <ClassName> [-o out.rs]');
    exit(2);
  }
  // Normalised through `package:path`, not `File.absolute`: analyzer requires
  // the platform's own spelling, and on Windows a forward-slash path that every
  // other tool here accepts is rejected outright.
  final path = p.normalize(p.absolute(args[0]));
  final wanted = args[1];
  String? out;
  for (var i = 2; i < args.length - 1; i++) {
    if (args[i] == '-o') out = args[i + 1];
  }

  final collection = AnalysisContextCollection(includedPaths: [path]);
  final session = collection.contextFor(path).currentSession;
  final resolved = await session.getResolvedUnit(path);
  if (resolved is! ResolvedUnitResult) {
    stderr.writeln('did not resolve: $resolved');
    exit(1);
  }

  // Errors in the *input* are reported before anything is emitted. Translating
  // a file the analyser could not make sense of would produce Rust built on
  // whatever it guessed the broken part meant.
  final errors = resolved.diagnostics
      .where((d) => d.diagnosticCode.severity.name == 'ERROR')
      .toList();
  if (errors.isNotEmpty) {
    stderr.writeln('${errors.length} error(s) in the input:');
    for (final e in errors.take(5)) {
      stderr.writeln('  ${e.message}');
    }
    exit(1);
  }

  // Whole-file mode: every class, traits before the structs that implement
  // them. This is the only mode in which a hierarchy can be emitted at all.
  if (wanted == '--all') {
    final frontend = Frontend('');
    final (lib, refused) = frontend.lowerLibrary(resolved.unit);
    final (rust, backendRefused) = RustBackend.emitLibrary(
      lib,
      frontEndRefusals: refused,
    );
    refused.addAll(backendRefused);
    stderr.writeln(
      '${lib.classes.length} classes '
      '(${lib.classes.where((c) => c.isAbstract).length} abstract), '
      '${refused.length} refused',
    );
    for (final r in refused.take(20)) {
      stderr.writeln('  REFUSED $r');
    }
    if (out != null) {
      File(out).writeAsStringSync(rust);
      stderr.writeln('-> $out');
    } else {
      stdout.write(rust);
    }
    return;
  }

  ClassDeclaration? target;
  for (final declaration in resolved.unit.declarations) {
    if (declaration is ClassDeclaration && declaration.name.lexeme == wanted) {
      target = declaration;
      break;
    }
  }
  if (target == null) {
    stderr.writeln('no class `$wanted` in $path');
    exit(1);
  }

  final frontend = Frontend(wanted);
  final (cls, refused) = frontend.lowerClass(target);

  String? rust;
  try {
    rust = RustBackend(cls).emit();
  } on Unsupported catch (error) {
    refused.add('backend: $error');
  }

  // The evaluated constants, printed beside the translated ones. The front end
  // lowers a `static const` from its *source* so the output stays recognisable;
  // this is the check that the source it lowered means what the analyser says
  // it means -- the 100000-microsecond fact, kept honest.
  final element = target.declaredFragment?.element;
  final evaluated = <String>[];
  if (element is ClassElement) {
    for (final field in element.fields) {
      if (!field.isStatic || !field.isConst) continue;
      final value = field.computeConstantValue();
      if (value != null) evaluated.add('${field.name} = $value');
    }
  }

  stderr.writeln(
    '$wanted: ${cls.fields.length} fields, '
    '${cls.constants.length} constants, ${cls.methods.length} methods, '
    '${refused.length} refused',
  );
  for (final r in refused) {
    stderr.writeln('  REFUSED $r');
  }
  if (evaluated.isNotEmpty) {
    stderr.writeln(
      '  (analyser evaluated ${evaluated.length} constants, '
      'e.g. ${evaluated.first})',
    );
  }

  if (rust == null) {
    exit(1);
  }
  if (out != null) {
    File(out).writeAsStringSync(rust);
    stderr.writeln('-> $out');
  } else {
    stdout.write(rust);
  }
}
