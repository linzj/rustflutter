// A *promoted* type parameter: Kernel writes `T% & Object` where an
// `if (x == null) return;` proved a `T?` non-null, and a call taking the
// promoted value records that type in a position this compiler has to spell.
//
// Promotion changes what is *known*, not what is held: the spelling is the
// parameter's own, non-nullable. `UndoHistoryState._update` is the shape --
// `void _update(T? nextValue) { if (nextValue == null) return;
// widget.onTriggered(nextValue); }`.
class Trigger<T> {
  Trigger(this.onTriggered);

  final void Function(T) onTriggered;
  final List<String> seen = <String>[];

  void update(T? next) {
    if (next == null) {
      seen.add('-');
      return;
    }
    onTriggered(next);
    seen.add('$next');
  }
}

String use() {
  final out = <String>[];
  final strings = Trigger<String>(out.add);
  strings.update(null);
  strings.update('a');
  strings.update('b');
  final numbers = Trigger<int>((int n) => out.add('$n'));
  numbers.update(7);
  numbers.update(null);
  return '${out.join(",")}/${strings.seen.join(",")}/${numbers.seen.join(",")}';
}
