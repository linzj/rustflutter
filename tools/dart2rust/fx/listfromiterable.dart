// `List.from(xs)` where `xs` is a `LinkedList`, not a list.
//
// It was lowered as `xs.clone()`, which copies whatever `xs` is:
// `List<_ListenerEntry>.from(_listeners!)` came out as a `LinkedList` in a
// `Vec` slot (`_ScrollNotificationObserverState._notifyListeners`, the stub
// run905 walked into once microtasks began running).
import 'dart:collection';

final class Entry extends LinkedListEntry<Entry> {
  Entry(this.name);

  final String name;
}

String use() {
  final entries = LinkedList<Entry>();
  entries.add(Entry('b'));
  entries.add(Entry('a'));
  final copy = List<Entry>.from(entries);
  return '${copy.length}/${copy.map((e) => e.name).join("-")}';
}
