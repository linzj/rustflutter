// An object registering and unregistering *itself* by identity.
//
// `Element._ensureDeactivated` does `dependency.removeDependent(this)`, and
// `InheritedElement._dependents` is keyed by the element. If `this` handed
// out of a method is a fresh handle each time, the key removed is not the
// key inserted, the element stays a dependent after it is unmounted, and the
// next `notifyClients` reaches an element whose render object is gone
// (`widgets_framework.rs:5471`, run906).
class Registry {
  final Set<Object> seen = <Object>{};

  void add(Object o) {
    seen.add(o);
  }

  void remove(Object o) {
    seen.remove(o);
  }
}

abstract class Member {
  void join(Registry r);
  void leave(Registry r);
}

class Item extends Member {
  @override
  void join(Registry r) {
    r.add(this);
  }

  @override
  void leave(Registry r) {
    r.remove(this);
  }
}

String use() {
  final registry = Registry();
  final Member item = Item();
  item.join(registry);
  final joined = registry.seen.length;
  item.leave(registry);
  return '$joined/${registry.seen.length}';
}
