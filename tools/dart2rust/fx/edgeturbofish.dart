// A call's type argument is spelled the way its value is.
//
// A nullable type parameter crosses a call edge *projected* -- `<T as
// DartNullable>::Or`, not `Option<T>` -- so a type argument that names one
// has to say the same, or the turbofish and the value disagree:
//
//     entry.complete::<Option<T>>(<T as DartNullable>::from_option(result))
//
// in `NavigatorState.removeRoute`, whose callee is `complete<T>(T result)`.
// That is the one shape where the two spellings meet: a parameter whose
// declared type *is* the type parameter.
//
// Not otherwise. Where the type parameter only shapes the *return* the
// projected spelling puts the nesting out by one -- `resourcesFor<T>` hands
// back a `T?` and the caller flattens an `Option<Option<T>>` -- which cost
// a stub in `Localizations.of` when this was written unconditionally.
//
// That negative half is **measured in the gallery, not pinned here**: two
// tries at writing it as a fixture (`V? read<V>()` with an `is V` test, and
// `V? firstOf<V>(List<V?>)`) each landed in a different unrelated hole --
// a cast to a method's own type parameter, and the element projection of a
// `List<V?>` -- which are worth their own rounds and are not this rule.

class Sink<T> {
  final List<String> seen = <String>[];

  // The parameter *is* the type parameter: the two spellings meet here.
  void take<V>(V value) {
    seen.add('$value');
  }
}

// The caller is generic and hands the callee *its own* `T?` -- the shape
// `NavigatorState.removeRoute<T>` has. A `T?` local in a non-generic
// function is an `Option<T>` outright and never asks the question.
void pass<T>(Sink<T> sink, T? value) {
  sink.take<T?>(value);
}

String use() {
  final Sink<int> sink = Sink<int>();
  pass<int>(sink, 7);
  pass<int>(sink, null);
  final Sink<String> words = Sink<String>();
  pass<String>(words, 'a');
  pass<String>(words, null);
  // ..and the same callee at a non-nullable type argument, which keeps the
  // plain spelling: `_edgeType` is the identity there.
  sink.take<int>(3);
  return '${sink.seen.join(",")}|${words.seen.join(",")}';
}
