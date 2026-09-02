// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Dumps upstream Flutter's declarations to JSON, using the real analyzer.
///
/// Every other tool in this directory reads Dart with regular expressions.
/// That was enough to *count* members -- which is what `coverage.py` and
/// `depth.py` do -- and it is not enough to *port* one: a regex cannot tell a
/// named parameter from a positional one, cannot carry a default value, and
/// cannot find the doc comment that says why the default is what it is. Those
/// three things are most of what a port needs.
///
/// So this uses `package:analyzer`'s own parser, resolved through the Flutter
/// checkout's package config:
///
///     dart run --packages=<flutter>/.dart_tool/package_config.json \
///         tools/dart/dump_ast.dart <lib/src root> <out.json>
///
/// The AST is *unresolved* on purpose -- `parseFile` rather than a full
/// analysis session. Resolution needs every import to load and takes minutes
/// over the framework; what is wanted here is each declaration's shape, which
/// is a parse-level fact. The cost is that a type is the text the author
/// wrote (`Color?`, `ValueChanged<bool>`) rather than a resolved element, and
/// for driving a port that is the more useful of the two anyway: it is what
/// the reader of upstream sees.
library;

import 'dart:convert';
import 'dart:io';

import 'package:analyzer/dart/analysis/features.dart';
import 'package:analyzer/dart/analysis/utilities.dart';
import 'package:analyzer/dart/ast/ast.dart';
import 'package:analyzer/dart/ast/token.dart';

/// The doc comment above a declaration, as plain lines with the `///` gone.
///
/// Kept because upstream's comments carry the *reasons* -- which default,
/// which platform, what a null means -- and those are exactly the part a port
/// cannot reconstruct from a signature.
String? docOf(AnnotatedNode node) {
  final comment = node.documentationComment;
  if (comment == null) return null;
  final lines = <String>[];
  for (final token in comment.tokens) {
    var text = token.lexeme;
    if (text.startsWith('///')) {
      text = text.substring(3);
    } else if (text.startsWith('/**')) {
      text = text.substring(3);
    } else if (text.startsWith('*/')) {
      continue;
    }
    lines.add(text.trimRight());
  }
  return lines.join('\n').trim();
}

/// Whether any annotation on this declaration is `@<name>`.
bool hasAnnotation(AnnotatedNode node, String name) =>
    node.metadata.any((a) => a.name.name == name);

Map<String, Object?> paramOf(FormalParameter param) {
  final inner = param is DefaultFormalParameter ? param.parameter : param;
  String? type;
  var initialisesField = false;
  if (inner is SimpleFormalParameter) {
    type = inner.type?.toSource();
  } else if (inner is FieldFormalParameter) {
    // `this.autofocus` -- the constructor parameter *is* the field, which is
    // how most Flutter widgets are written and is why a port can read a whole
    // widget's surface off its constructor.
    type = inner.type?.toSource();
    initialisesField = true;
  } else if (inner is FunctionTypedFormalParameter) {
    type = inner.returnType?.toSource();
  } else if (inner is SuperFormalParameter) {
    type = inner.type?.toSource();
  }
  return <String, Object?>{
    'name': inner.name?.lexeme,
    'type': type,
    'named': param.isNamed,
    'required': param.isRequiredNamed || param.isRequiredPositional,
    'default': param is DefaultFormalParameter
        ? param.defaultValue?.toSource()
        : null,
    'initialises_field': initialisesField,
  };
}

List<Map<String, Object?>> paramsOf(FormalParameterList? list) =>
    list == null
        ? const []
        : list.parameters.map(paramOf).toList(growable: false);

Map<String, Object?> memberOf(ClassMember member) {
  if (member is ConstructorDeclaration) {
    return <String, Object?>{
      'kind': 'constructor',
      'name': member.name?.lexeme ?? '',
      'const': member.constKeyword != null,
      'factory': member.factoryKeyword != null,
      'params': paramsOf(member.parameters),
      'doc': docOf(member),
    };
  }
  if (member is FieldDeclaration) {
    return <String, Object?>{
      'kind': 'field',
      'static': member.isStatic,
      'type': member.fields.type?.toSource(),
      'const': member.fields.isConst,
      'final': member.fields.isFinal,
      'doc': docOf(member),
      'names': member.fields.variables
          .map((v) => <String, Object?>{
                'name': v.name.lexeme,
                'default': v.initializer?.toSource(),
              })
          .toList(growable: false),
    };
  }
  if (member is MethodDeclaration) {
    return <String, Object?>{
      'kind': member.isGetter
          ? 'getter'
          : member.isSetter
              ? 'setter'
              : 'method',
      'name': member.name.lexeme,
      'static': member.isStatic,
      'abstract': member.isAbstract,
      'override': hasAnnotation(member, 'override'),
      'protected': hasAnnotation(member, 'protected'),
      'return': member.returnType?.toSource(),
      'params': paramsOf(member.parameters),
      'doc': docOf(member),
    };
  }
  return <String, Object?>{'kind': 'other', 'source': member.toSource()};
}

Map<String, Object?> declarationOf(CompilationUnitMember node, String file) {
  if (node is ClassDeclaration) {
    return <String, Object?>{
      'kind': 'class',
      'name': node.name.lexeme,
      'file': file,
      'abstract': node.abstractKeyword != null,
      'extends': node.extendsClause?.superclass.toSource(),
      'implements':
          node.implementsClause?.interfaces.map((t) => t.toSource()).toList() ??
              const [],
      'with': node.withClause?.mixinTypes.map((t) => t.toSource()).toList() ??
          const [],
      'doc': docOf(node),
      'members': node.members.map(memberOf).toList(growable: false),
    };
  }
  if (node is EnumDeclaration) {
    return <String, Object?>{
      'kind': 'enum',
      'name': node.name.lexeme,
      'file': file,
      'doc': docOf(node),
      'values': node.constants
          .map((c) => <String, Object?>{
                'name': c.name.lexeme,
                'doc': docOf(c),
              })
          .toList(growable: false),
      'members': node.members.map(memberOf).toList(growable: false),
    };
  }
  if (node is MixinDeclaration) {
    return <String, Object?>{
      'kind': 'mixin',
      'name': node.name.lexeme,
      'file': file,
      'doc': docOf(node),
      'members': node.members.map(memberOf).toList(growable: false),
    };
  }
  if (node is ExtensionDeclaration) {
    return <String, Object?>{
      'kind': 'extension',
      'name': node.name?.lexeme ?? '',
      'file': file,
      'on': node.onClause?.extendedType.toSource(),
      'doc': docOf(node),
      'members': node.members.map(memberOf).toList(growable: false),
    };
  }
  if (node is FunctionDeclaration) {
    return <String, Object?>{
      'kind': 'function',
      'name': node.name.lexeme,
      'file': file,
      'return': node.returnType?.toSource(),
      'params': paramsOf(node.functionExpression.parameters),
      'doc': docOf(node),
    };
  }
  if (node is TopLevelVariableDeclaration) {
    return <String, Object?>{
      'kind': 'variable',
      'file': file,
      'const': node.variables.isConst,
      'type': node.variables.type?.toSource(),
      'doc': docOf(node),
      'names': node.variables.variables
          .map((v) => <String, Object?>{
                'name': v.name.lexeme,
                'default': v.initializer?.toSource(),
              })
          .toList(growable: false),
    };
  }
  if (node is TypeAlias) {
    return <String, Object?>{
      'kind': 'typedef',
      'name': node.name.lexeme,
      'file': file,
      'doc': docOf(node),
      'source': node.toSource(),
    };
  }
  return <String, Object?>{'kind': 'other', 'file': file};
}

void main(List<String> args) {
  if (args.length < 2) {
    stderr.writeln('usage: dump_ast.dart <lib/src root> <out.json>');
    exit(2);
  }
  final root = Directory(args[0]);
  final out = File(args[1]);

  final files = root
      .listSync(recursive: true)
      .whereType<File>()
      .where((f) => f.path.endsWith('.dart'))
      .toList()
    ..sort((a, b) => a.path.compareTo(b.path));

  final declarations = <Map<String, Object?>>[];
  var failed = 0;
  for (final file in files) {
    // The path as the rest of the tooling spells it: forward slashes, relative
    // to lib/src, so a row here joins against `coverage.py`'s file column
    // without either side having to know about the other's root.
    final relative = file.path
        .substring(root.path.length + 1)
        .replaceAll(r'\', '/');
    try {
      final unit = parseFile(
        path: file.path,
        featureSet: FeatureSet.latestLanguageVersion(),
        throwIfDiagnostics: false,
      ).unit;
      for (final node in unit.declarations) {
        declarations.add(declarationOf(node, relative));
      }
    } catch (error) {
      // Reported rather than swallowed: a file that would not parse is a hole
      // in the inventory, and an inventory with a silent hole is worse than
      // one that says where it stops.
      stderr.writeln('could not parse $relative: $error');
      failed++;
    }
  }

  out.writeAsStringSync(
    const JsonEncoder.withIndent('  ').convert(<String, Object?>{
      'files': files.length,
      'declarations': declarations,
    }),
  );
  stdout.writeln('${declarations.length} declarations from ${files.length} '
      'files -> ${out.path}');
  if (failed > 0) {
    stdout.writeln('$failed file(s) did not parse; see stderr');
    exit(1);
  }
}
