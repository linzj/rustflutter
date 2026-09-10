// A mixin's method mutates a `final` Map field in place, and the object is
// reached through the abstract class it extends.
//
// The backend's `_mutating` sees `_cache.remove(k)` and makes the trait
// method `&mut self`; the front end's `counted` census walks the class's own
// procedures, and this body lives in the CFE's mixin *application*. So the
// class is not counted, its field is not a cell, and the call through
// `Rc<dyn Policy>` is E0596.
abstract class Policy {
  void invalidate(String key);
  int seen();
}

mixin CacheMixin on Policy {
  final Map<String, int> _cache = <String, int>{};

  void note(String key, int value) {
    _cache[key] = value;
  }

  @override
  void invalidate(String key) {
    _cache.remove(key);
  }

  @override
  int seen() => _cache.length;
}

class Counter extends Policy with CacheMixin {}

class Holder {
  Holder(this.policy);
  final Policy policy;
}

String use() {
  final counter = Counter();
  counter.note('a', 1);
  counter.note('b', 2);
  final holder = Holder(counter);
  holder.policy.invalidate('a');
  return '${holder.policy.seen()}';
}
