// `a ?? b` where the two arms are of one class whose *arguments* differ.
//
// Dart's type for the whole is the least upper bound -- `List<Object>` for a
// `List<Tile>` and a `List<Note>` -- and both arms go into it. Taking the
// left arm's spelling instead maps the right one's elements up into a `Tile`
// they are not: `children ?? buttonItems` in both
// `AdaptiveTextSelectionToolbar.build`s is that shape.
abstract class Piece {
  String get tag;
}

class Tile implements Piece {
  @override
  String get tag => 'tile';
}

class Note {
  const Note(this.text);

  final String text;
}

/// Opaque to the tree shaker's constant folding: the answer depends on an
/// argument, so neither arm of the `??` below is known to be null.
List<T>? pickOrNothing<T>(List<T> xs, bool keep) => keep ? xs : null;

String use() {
  final List<Tile> tiles = <Tile>[Tile()];
  final List<Note> notes = <Note>[const Note('n')];
  final List<Tile>? someTiles = pickOrNothing(tiles, tiles.isEmpty);
  final List<Note>? someNotes = pickOrNothing(notes, notes.isNotEmpty);
  final both = someTiles ?? someNotes;
  final empty = (someTiles ?? someNotes)?.isEmpty ?? true;
  final kept = pickOrNothing(tiles, tiles.isNotEmpty) ?? <Tile>[];
  return '${both?.length}/$empty/${kept.length}/${kept.first.tag}';
}
