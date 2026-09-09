// The five sets of receiver-changing member names, and the differences
// between them.
//
// These are the sets that had drifted apart while nothing could see them
// (`lib/member_names.dart` says what the drift was). Composition can bring
// them back together silently as easily as copying kept them apart, so the
// contents are written out here in full: a name added to the wrong group
// fails, and a gap closed on purpose fails too -- loudly, next to the note
// saying it was a gap.
library;

import '../lib/member_names.dart';
import 'check.dart';

void main() {
  group('the composed sets');
  expect(mutatingNames.length, 34, 'every Dart mutator');
  expect(mutatingSequenceNames.length, 21, 'sequence members');
  expect(mutatingMapNames.length, 3, 'map members');
  expect(mutatingByteDataNames.length, 10, 'ByteData setters');
  expect(mutatingListNames.length, 22, 'what _listReceiver looks for');
  expect(mutatingThisFieldNames.length, 22, 'what _ThisWriteFinder looks for');
  expect(mutatingRustOnlyNames.length, 19, 'Rust names with no Dart member');

  expect(mutatingNames, {
    ...mutatingSequenceNames,
    ...mutatingMapNames,
    ...mutatingByteDataNames,
  }, 'the whole is exactly the three groups');
  expect(mutatingSequenceNames.difference(mutatingSequenceCoreNames), {
    'removeRange',
  }, 'the core is the sequence set without removeRange');

  group('the differences that remain');
  // Each of these is a fact about the compiler as it stands, not a wish.
  // Closing one changes what is emitted, so it changes this test too, in the
  // same commit as the chain measurement that justifies it.
  expect(mutatingListNames.difference(mutatingNames), {
    'length',
  }, 'a list receiver adds `length` -- `list.length = n` is a write');
  expect(mutatingNames.difference(mutatingListNames), {
    ...mutatingMapNames,
    ...mutatingByteDataNames,
  }, 'a list receiver has no map or byte-view members');
  expect(mutatingNames.difference(mutatingThisFieldNames), {
    'removeRange',
    'updateAll',
    ...mutatingByteDataNames,
  }, 'KNOWN GAP: `this.field.removeRange(..)` is not seen as a write');
  expect(
    mutatingThisFieldNames.difference(mutatingNames),
    <String>{},
    'and it looks for nothing that is not a mutator',
  );
  expect(noRustMutatorNames, {
    '[]=',
    'updateAll',
  }, 'KNOWN GAP: `updateAll` has no prelude method; `[]=` is index assignment');

  group('the Rust half');
  // What `backend_rust.dart` composes, spelled again here so that a change to
  // `snake` or to the groups above cannot quietly change the set of methods
  // the backend borrows `&mut` for.
  final inPlace = {
    for (final n in mutatingNames)
      if (!noRustMutatorNames.contains(n)) _snake(n),
    ...mutatingRustOnlyNames,
  };
  expect(inPlace.length, 51, 'Rust names that change the receiver');
  expectTrue(inPlace.contains('add_all'), 'addAll snakes into it');
  expectTrue(inPlace.contains('set_uint16'), 'the byte-view setters are in it');
  expectTrue(
    inPlace.contains('!map_remove'),
    "the prelude's Map::remove is in it",
  );
  expectTrue(!inPlace.contains('update_all'), 'and updateAll is not (the gap)');
  expectTrue(!inPlace.contains('[]='), 'nor index assignment');

  group('what _WalkSelf asks for');
  // The fifth copy, gathered 2026-09-09. It decides which local gets `let mut`
  // and which method takes `&mut self`, and it is far smaller than `_inPlace`.
  // Both directions are written out, so folding the table into one place
  // cannot turn into closing the gap without this test saying so.
  expect(mutatingWalkSelfRustNames.length, 12, 'what _WalkSelf looks for');
  expect(mutatingWalkSelfRustNames.difference(inPlace), {
    '!insert',
    '!remove_at',
  }, "the backend's markers, which _inPlace never carried");
  expect(
    inPlace.difference(mutatingWalkSelfRustNames).length,
    41,
    'KNOWN GAP: 41 in-place names do not put `mut` on their receiver',
  );
  expectTrue(
    inPlace.difference(mutatingWalkSelfRustNames).contains('add_all'),
    '`xs.addAll(..)` among them',
  );
  expectTrue(
    inPlace.difference(mutatingWalkSelfRustNames).contains('remove_where'),
    'and `xs.removeWhere(..)`',
  );

  report('member_names');
}

/// `backend_rust.dart`'s `snakeRaw`, which is what composes the Rust set.
String _snake(String name) => name
    .replaceAllMapped(RegExp(r'(?<!^)([A-Z])'), (m) => '_${m[1]}')
    .toLowerCase();
