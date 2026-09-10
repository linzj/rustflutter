// A prelude *static* whose callback slot only calls what it is given.
//
// The prelude's callback slots that are `impl Fn` -- called and dropped,
// never kept -- take the closure itself, and an `Rc<dyn Fn>` is not one.
// That lend was given to instance calls on a prelude receiver and not to a
// static of a prelude class, so `Timeline.timeSync(label, () { .. })` was
// handed `Rc::new(closure)`: "expected an `Fn()` closure, found
// `Rc<{closure}>`".
//
// It matters beyond the spelling: an `Rc<dyn Fn>` has to be `'static`, and
// a closure that reads `&self` cannot be -- which is how this showed up,
// as "lifetime may not live long enough" in `_TaskEntry.run`, whose
// closure reads two of its own fields.
//
// **This fixture does not go red before the change**, and it is written
// down here so that nobody reads it as though it did. Whether the closure
// borrows `this` or copies out of it turns on `_keeps`, which reads the
// *callee's body*: the gallery's AOT dill carries `Timeline.timeSync`'s
// and a fixture's minimal one does not, so here the argument was boxed
// (and compiled) all along. What it does pin is the behaviour of the
// changed prelude method: the closure handed to a lent slot still sees the
// object it was written against, across more than one call.

import 'dart:developer';

class Entry {
  Entry(this.label, this.step);
  final String label;
  final int step;
  int total = 0;

  // The shape that was wrong: a closure reading `this`, handed to a
  // prelude static that only calls it. Called for its effect, as
  // `_TaskEntry.run` calls it.
  void run() {
    Timeline.timeSync<int>(label, () {
      total += step;
      return total;
    });
  }
}

String use() {
  final Entry e = Entry('e', 3);
  e.run();
  e.run();
  // ..and a closure that reads nothing of `this`, through the same static.
  final List<String> seen = <String>[];
  Timeline.timeSync<int>('plain', () {
    seen.add('ok');
    return 0;
  });
  return '${e.total}/${seen.join(",")}';
}
