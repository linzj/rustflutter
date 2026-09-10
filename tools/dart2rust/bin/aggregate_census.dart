// How much of this program is constant data written as code.
//
//   dart run bin/aggregate_census.dart .build/gallery/app_aot_sig.dill \
//       "package:,dart:ui"
//
// Step 0 of `/tmp/work.md`: count from the IR what the plan so far has only
// reverse-engineered from binary symbols ("15.3 MB / ~67 functions"), and
// give the threshold N a basis. Changes no code generation; the rulers do
// not move because nothing downstream reads any of this.
import 'dart:io';

import 'package:kernel/kernel.dart';

import '../lib/aggregates.dart';

// What a tree costs in `.text`, as three rates rather than one.
//
// Fitted by least squares against `nm -S` on the ws976 release binary, over
// the 18 generated Rust modules whose Dart library could be joined to a
// module name. Pricing a tree by its construction count alone is wrong by up
// to 8x: `raw_keyboard_android` is 1,003 constructions and 35 KB, while
// `dateSymbols` is 98 constructions and 1.16 MB. The difference is string
// leaves -- every Dart string becomes a `to_string()`, an allocation and a
// store -- which is why they are counted apart from scalars.
//
// Where the fit can be checked it holds: codeviewer_code_segments 1.00,
// generated_date_localizations 1.04, number_symbols_data 1.15. On modules
// under ~60 KB it over-predicts by 2-4x, because rustc const-folds the small
// trees into statics. So the projected total below is an upper bound, and
// only its head is evidence.
const bytesPerCtor = 215;
const bytesPerStringLeaf = 85;
const bytesPerScalarLeaf = 2;

int project(int ctors, int stringLeaves, int scalarLeaves) =>
    ctors * bytesPerCtor +
    stringLeaves * bytesPerStringLeaf +
    scalarLeaves * bytesPerScalarLeaf;

String mb(num bytes) => '${(bytes / 1e6).toStringAsFixed(2)} MB';

void main(List<String> args) {
  final component = loadComponentFromBinary(args[0]);
  final prefixes = (args.length > 1 ? args[1] : 'package:,dart:ui').split(',');
  final sw = Stopwatch()..start();
  final census = AggregateCensus.of(component, prefixes);
  sw.stop();

  stdout.writeln('== data constructors ==');
  stdout.writeln(
    'data: ${census.dataConstructors.length} / ${census.constructorsSeen} '
    '(${(100 * census.dataConstructors.length / census.constructorsSeen).toStringAsFixed(1)}%), '
    'rounds: ${census.rounds}, ${sw.elapsedMilliseconds} ms',
  );
  final why = census.rejections.entries.toList()
    ..sort((a, b) => b.value.compareTo(a.value));
  for (final e in why) {
    stdout.writeln('  ${e.value.toString().padLeft(6)}  ${e.key}');
  }

  final aggs = census.aggregates;
  final totalCtors = aggs.fold(0, (a, x) => a + x.ctors);
  final totalNodes = aggs.fold(0, (a, x) => a + x.nodes);
  final totalBytes = aggs.fold(0, (a, x) => a + x.literalBytes);
  stdout.writeln(
    '\n== aggregates (members scanned ${census.membersScanned}) ==',
  );
  stdout.writeln(
    'trees: ${aggs.length}, constructions: $totalCtors, nodes: $totalNodes, '
    'string bytes: ${mb(totalBytes)}',
  );
  final totalStr = aggs.fold(0, (a, x) => a + x.stringLeaves);
  final totalScalar = aggs.fold(0, (a, x) => a + x.scalarLeaves);
  stdout.writeln(
    'string leaves: $totalStr (an allocation each), scalar leaves: $totalScalar',
  );
  stdout.writeln(
    'projected .text (upper bound): ${mb(project(totalCtors, totalStr, totalScalar))}',
  );

  // The threshold is on *string leaves*, not constructions: that is the term
  // the cost actually follows, and it is what separates the 62 trees that
  // are worth a table from the 2,750 that are not.
  stdout.writeln('\n== by threshold N (string leaves per tree) ==');
  stdout.writeln(
    '     N     trees   constructions   string leaves   share   projected .text',
  );
  for (final n in [1, 8, 32, 64, 128, 200, 256, 512, 1024, 4096]) {
    final kept = aggs.where((a) => a.stringLeaves >= n).toList();
    final c = kept.fold(0, (a, x) => a + x.ctors);
    final sl = kept.fold(0, (a, x) => a + x.stringLeaves);
    final kl = kept.fold(0, (a, x) => a + x.scalarLeaves);
    stdout.writeln(
      '  ${n.toString().padLeft(4)}  ${kept.length.toString().padLeft(8)}  '
      '${c.toString().padLeft(14)}  ${sl.toString().padLeft(14)}  '
      '${totalStr == 0 ? '-' : '${(100 * sl / totalStr).toStringAsFixed(1)}%'.padLeft(6)}  '
      '${mb(project(c, sl, kl)).padLeft(16)}',
    );
  }

  stdout.writeln('\n== outer invariants (the assumed part) ==');
  final pure = aggs.where((a) => a.invariants.isEmpty).toList();
  stdout.writeln(
    'trees with no invariant leaf: ${pure.length} '
    '(${pure.fold(0, (a, x) => a + x.ctors)} constructions, '
    '${pure.fold(0, (a, x) => a + x.stringLeaves)} string leaves) '
    '-- these need no judgement about getters',
  );
  final buckets = <String, List<int>>{};
  for (final a in aggs) {
    final n = a.invariants.length;
    final key = n == 0
        ? '0'
        : n <= 4
        ? '1-4'
        : n <= 16
        ? '5-16'
        : n <= 64
        ? '17-64'
        : '65+';
    final row = buckets.putIfAbsent(key, () => [0, 0]);
    row[0]++;
    row[1] += a.stringLeaves;
  }
  for (final key in ['0', '1-4', '5-16', '17-64', '65+']) {
    final row = buckets[key];
    if (row == null) continue;
    stdout.writeln(
      '  distinct invariants ${key.padRight(6)} trees ${row[0].toString().padLeft(7)}  '
      'string leaves ${row[1].toString().padLeft(8)}',
    );
  }

  if (args.contains('--tsv')) {
    // One line per tree, for joining against `nm -S` on the release binary.
    // The projection below rests on a rate measured in *one* function; this
    // is how the other rates get measured rather than assumed.
    final out = File('${args[0]}.aggregates.tsv').openWrite();
    out.writeln(
      'ctors\tnodes\tstrLeaves\tscalarLeaves\tstringBytes\tinvariants\t'
      'member\tlibrary\tshape',
    );
    for (final a in aggs) {
      final cls = a.member.enclosingClass?.name;
      out.writeln(
        '${a.ctors}\t${a.nodes}\t${a.stringLeaves}\t${a.scalarLeaves}\t'
        '${a.literalBytes}\t${a.invariants.length}\t'
        '${cls == null ? '' : '$cls.'}${a.member.name.text}\t${a.library}\t${a.shape}',
      );
    }
    out.close();
    stdout.writeln('\nwrote ${args[0]}.aggregates.tsv');
  }

  stdout.writeln('\n== by library ==');
  final byLib = <String, List<int>>{};
  for (final a in aggs) {
    final row = byLib.putIfAbsent(a.library, () => [0, 0, 0, 0]);
    row[0]++;
    row[1] += a.ctors;
    row[2] += a.literalBytes;
    row[3] += a.nodes;
  }
  final libs = byLib.entries.toList()
    ..sort((a, b) => b.value[3].compareTo(a.value[3]));
  stdout.writeln('  constructions      nodes   trees   strings   library');
  for (final e in libs.take(25)) {
    stdout.writeln(
      '  ${e.value[1].toString().padLeft(13)}  ${e.value[3].toString().padLeft(9)}   '
      '${e.value[0].toString().padLeft(5)}   ${mb(e.value[2]).padLeft(8)}   ${e.key}',
    );
  }

  stdout.writeln('\n== by member (top 20) ==');
  final ranked = aggs.toList()..sort((a, b) => b.nodes.compareTo(a.nodes));
  for (final a in ranked.take(20)) {
    final cls = a.member.enclosingClass?.name;
    stdout.writeln(
      '  ${a.nodes.toString().padLeft(7)} nodes  ${a.ctors.toString().padLeft(6)} ctors  '
      'inv ${a.invariants.length.toString().padLeft(3)}  '
      '${cls == null ? '' : '$cls.'}${a.member.name.text}  '
      '(${a.member.enclosingLibrary.importUri.pathSegments.last})',
    );
  }

  stdout.writeln('\n== builders: distinct shapes ==');
  final byShape = <String, List<int>>{};
  for (final a in aggs) {
    final row = byShape.putIfAbsent(a.shape, () => [0, 0]);
    row[0]++;
    row[1] += a.ctors;
  }
  stdout.writeln(
    'distinct shapes: ${byShape.length} over ${aggs.length} trees',
  );
  final shapes = byShape.entries.toList()
    ..sort((a, b) => b.value[1].compareTo(a.value[1]));
  for (final e in shapes.take(15)) {
    final shape = e.key.length > 110 ? '${e.key.substring(0, 110)}...' : e.key;
    stdout.writeln(
      '  ${e.value[1].toString().padLeft(8)} ctors in ${e.value[0].toString().padLeft(5)} trees  $shape',
    );
  }
}
