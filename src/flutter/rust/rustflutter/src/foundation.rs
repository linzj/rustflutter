//! The reactive floor, from upstream `foundation/change_notifier.dart` and
//! `foundation/key.dart`: listenables, notifiers, and the key spellings.
//!
//! The crate's own widgets rebuild wholesale, so nothing here is wired into
//! the element tree yet -- it is the floor the material wave stands on
//! (`WidgetStateNotifier`, text fields, controllers), exactly as upstream's
//! is.
//!
//! Recorded divergences (see PORTING_STATUS.md):
//!
//! * Upstream's `Key` hierarchy (`ValueKey`/`ObjectKey`/`UniqueKey`/
//!   `LabeledGlobalKey`/`GlobalObjectKey`) is this crate's `Option<u64>`
//!   plus `GlobalKey`'s atomic counter. The spellings below wrap those
//!   semantics without changing the element tree's key type.
//! * The debug-assertion surface is not ported; those classes are ledgered
//!   out of scope. `DiagnosticsNode` itself **is** here, as a trait in
//!   `diagnostics.rs` -- this line said otherwise until tick 286.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Upstream `Listenable`: something whose listeners want to hear when it
/// changes.
pub trait Listenable {
    fn add_listener(&self, callback: Rc<dyn Fn()>);
    fn remove_listener(&self, callback: &Rc<dyn Fn()>);
}

/// Upstream `Listenable.merge`: several listenables behind one.
pub struct ListenableMerge {
    children: Vec<Rc<dyn Listenable>>,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
}

impl ListenableMerge {
    pub fn new(children: Vec<Rc<dyn Listenable>>) -> Rc<ListenableMerge> {
        Rc::new(ListenableMerge {
            children,
            listeners: Rc::new(RefCell::new(Vec::new())),
        })
    }
}

impl Listenable for ListenableMerge {
    fn add_listener(&self, callback: Rc<dyn Fn()>) {
        // A listener on the merge hears every child.
        for child in &self.children {
            child.add_listener(Rc::clone(&callback));
        }
        self.listeners.borrow_mut().push(callback);
    }

    fn remove_listener(&self, callback: &Rc<dyn Fn()>) {
        for child in &self.children {
            child.remove_listener(callback);
        }
        self.listeners
            .borrow_mut()
            .retain(|existing| !Rc::ptr_eq(existing, callback));
    }
}

/// Upstream `ChangeNotifier`: listeners told, at most once per microtask
/// upstream; here the notification is immediate and re-entrant telling is
/// held until the outer tell returns, which is the same shape of guarantee.
pub struct ChangeNotifier {
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
    /// A notify in flight: inner tells are dropped instead of recursing.
    notifying: RefCell<bool>,
    /// Listeners added while a notify was in flight, kept after.
    pending_additions: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl Default for ChangeNotifier {
    fn default() -> ChangeNotifier {
        ChangeNotifier {
            listeners: RefCell::new(Vec::new()),
            notifying: RefCell::new(false),
            pending_additions: RefCell::new(Vec::new()),
        }
    }
}

impl ChangeNotifier {
    pub fn new() -> ChangeNotifier {
        ChangeNotifier::default()
    }

    /// Whether anyone is listening -- upstream's `hasListeners`.
    pub fn has_listeners(&self) -> bool {
        !self.listeners.borrow().is_empty()
    }

    /// Upstream `dispose`: nobody is listening any more.
    pub fn dispose(&self) {
        self.listeners.borrow_mut().clear();
    }

    /// Upstream `notifyListeners`.
    pub fn notify_listeners(&self) {
        if *self.notifying.borrow() {
            return;
        }
        *self.notifying.borrow_mut() = true;
        // Copied, so a listener may remove itself or add another mid-tell.
        let listeners = self.listeners.borrow().clone();
        for listener in &listeners {
            listener();
        }
        *self.notifying.borrow_mut() = false;
        let additions = std::mem::take(&mut *self.pending_additions.borrow_mut());
        for addition in additions {
            self.listeners.borrow_mut().push(addition);
        }
    }
}

impl Listenable for ChangeNotifier {
    fn add_listener(&self, callback: Rc<dyn Fn()>) {
        if *self.notifying.borrow() {
            // Upstream asserts against modification during notification;
            // deferring an addition keeps the tell loop stable.
            self.pending_additions.borrow_mut().push(callback);
            return;
        }
        self.listeners.borrow_mut().push(callback);
    }

    fn remove_listener(&self, callback: &Rc<dyn Fn()>) {
        self.listeners
            .borrow_mut()
            .retain(|existing| !Rc::ptr_eq(existing, callback));
    }
}

/// Upstream `ValueNotifier<T>`: one value; telling listeners when it
/// actually changed.
pub struct ValueNotifier<T: PartialEq + Clone> {
    pub notifier: ChangeNotifier,
    value: RefCell<T>,
}

impl<T: PartialEq + Clone> ValueNotifier<T> {
    pub fn new(value: T) -> ValueNotifier<T> {
        ValueNotifier {
            notifier: ChangeNotifier::new(),
            value: RefCell::new(value),
        }
    }

    pub fn value(&self) -> T {
        self.value.borrow().clone()
    }

    /// Upstream `ValueNotifier.value=`: same value, no tell.
    pub fn set_value(&self, value: T) {
        if *self.value.borrow() == value {
            return;
        }
        *self.value.borrow_mut() = value;
        self.notifier.notify_listeners();
    }
}

impl<T: PartialEq + Clone> Listenable for ValueNotifier<T> {
    fn add_listener(&self, callback: Rc<dyn Fn()>) {
        self.notifier.add_listener(callback);
    }

    fn remove_listener(&self, callback: &Rc<dyn Fn()>) {
        self.notifier.remove_listener(callback);
    }
}

/// The key spellings, upstream `foundation/key.dart`. The crate's element
/// tree keys on an `Option<u64>` (see [`crate::framework::Key`]); these are
/// the constructors that give that number each spelling's semantics.
pub mod keys {
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Upstream `UniqueKey`: a key unequal to every key, including another
    /// fresh one. Each call draws a new number.
    pub fn unique() -> Option<u64> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Some(NEXT.fetch_add(1, Ordering::Relaxed) | (1 << 63))
    }

    /// Upstream `ValueKey<T>`: equality by value. The crate's key already
    /// compares by number, so the value is the key.
    pub fn value(value: u64) -> Option<u64> {
        Some(value)
    }

    /// Upstream `ObjectKey`: equality by object identity -- the address is
    /// the identity, the same role it plays upstream.
    pub fn object<T: ?Sized>(object: &T) -> Option<u64> {
        Some(&object as *const &T as *const () as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_notifier_tells_its_listeners() {
        let notifier = ChangeNotifier::new();
        assert!(!notifier.has_listeners());
        let count = Rc::new(Cell::new(0));
        let listener: Rc<dyn Fn()> = {
            let count = Rc::clone(&count);
            Rc::new(move || count.set(count.get() + 1))
        };
        notifier.add_listener(Rc::clone(&listener));
        assert!(notifier.has_listeners());
        notifier.notify_listeners();
        notifier.notify_listeners();
        assert_eq!(count.get(), 2);

        // Removing stops the telling; a removed listener stays removed.
        notifier.remove_listener(&listener);
        notifier.notify_listeners();
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn a_listener_may_remove_another_mid_tell() {
        let notifier = Rc::new(ChangeNotifier::new());
        let first = Rc::new(Cell::new(0));
        let first_listener = Rc::new({
            let first = Rc::clone(&first);
            move || first.set(first.get() + 1)
        }) as Rc<dyn Fn()>;
        let second_listener = {
            let notifier = Rc::clone(&notifier);
            let first_listener = Rc::clone(&first_listener);
            Rc::new(move || {
                notifier.remove_listener(&first_listener);
            })
        };
        notifier.add_listener(first_listener);
        notifier.add_listener(second_listener);
        notifier.notify_listeners();
        assert_eq!(first.get(), 1);
        notifier.notify_listeners();
        assert_eq!(first.get(), 1);
    }

    #[test]
    fn inner_tells_do_not_recurse() {
        let notifier = Rc::new(ChangeNotifier::new());
        let depth = Rc::new(Cell::new(0));
        let listener = {
            let notifier = Rc::clone(&notifier);
            let depth = Rc::clone(&depth);
            Rc::new(move || {
                depth.set(depth.get() + 1);
                if depth.get() == 1 {
                    // A tell from inside a tell: held, not recursed.
                    notifier.notify_listeners();
                }
            })
        };
        notifier.add_listener(listener);
        notifier.notify_listeners();
        assert_eq!(depth.get(), 1);
    }

    #[test]
    fn a_value_notifier_tells_only_on_change() {
        let notifier = ValueNotifier::new(10);
        let seen = Rc::new(Cell::new(0));
        notifier.add_listener({
            let seen = Rc::clone(&seen);
            Rc::new(move || seen.set(seen.get() + 1))
        });
        notifier.set_value(20);
        assert_eq!(notifier.value(), 20);
        assert_eq!(seen.get(), 1);
        // Same value: silent.
        notifier.set_value(20);
        assert_eq!(seen.get(), 1);
    }

    #[test]
    fn unique_keys_never_collide() {
        let a = keys::unique();
        let b = keys::unique();
        assert_ne!(a, b);
        // Value keys are their value.
        assert_eq!(keys::value(42), Some(42));
    }
}

// -- Telling a leak tracker that an object came and went -----------------------

/// Upstream `ObjectCreated`: an object came into existence, and where from.
///
/// The library and class name are carried here and **not** on the disposal
/// side, and that asymmetry is the design: a tracker matches the two by the
/// object's identity, so anything still unmatched at the end of a run is a leak
/// that already has a name attached to it. Carrying the name twice would be
/// paying for it on every disposal to learn nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectCreated {
    pub object: u64,
    pub library: &'static str,
    pub class_name: &'static str,
}

/// Upstream `ObjectDisposed`: an object is gone. Nothing but which one -- see
/// [`ObjectCreated`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectDisposed {
    pub object: u64,
}

/// Upstream `ObjectEvent`: something happened to an object worth tracking.
///
/// Upstream is an abstract class with two subclasses, which is an enum of the
/// two here: the set is closed, and a tracker has to handle both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectEvent {
    Created(ObjectCreated),
    Disposed(ObjectDisposed),
}

impl ObjectEvent {
    /// Upstream's `ObjectCreated(...)`, as a constructor.
    pub fn created(object: u64, library: &'static str, class_name: &'static str) -> ObjectEvent {
        ObjectEvent::Created(ObjectCreated {
            object,
            library,
            class_name,
        })
    }

    /// Upstream's `ObjectDisposed(...)`.
    pub fn disposed(object: u64) -> ObjectEvent {
        ObjectEvent::Disposed(ObjectDisposed { object })
    }

    /// Upstream's `ObjectEvent.object`, the one member the base class has --
    /// and the one a tracker matches on.
    pub fn object(&self) -> u64 {
        match self {
            ObjectEvent::Created(created) => created.object,
            ObjectEvent::Disposed(disposed) => disposed.object,
        }
    }

    /// Upstream's `toMap`, which is the wire format a tracker reads. The
    /// `eventType` string is the contract.
    pub fn to_map(&self) -> Vec<(&'static str, String)> {
        match self {
            ObjectEvent::Created(created) => vec![
                ("libraryName", created.library.to_string()),
                ("className", created.class_name.to_string()),
                ("eventType", "created".to_string()),
            ],
            ObjectEvent::Disposed(_) => vec![("eventType", "disposed".to_string())],
        }
    }
}

type ObjectEventListener = Rc<dyn Fn(&ObjectEvent)>;

/// Upstream `FlutterMemoryAllocations`: the registry those events go through.
///
/// # Everything is behind one flag
///
/// Upstream checks `kFlutterMemoryAllocationsEnabled` at the top of every
/// method, so that in a release build the whole thing compiles to nothing and
/// the call sites scattered through the framework cost nothing either. That is
/// why the checks are repeated rather than done once at the edge.
///
/// # Removing a listener during a dispatch writes a hole
///
/// A listener is entitled to remove itself while it is being called, and
/// upstream's dispatch walks the list by index -- so removing an element would
/// shift everything after it and skip a listener. While a dispatch is running,
/// removal writes `None` in place instead, and the list is compacted when the
/// last dispatch finishes. The nested case is why it is a *count* of active
/// loops and not a flag.
///
/// The dispatch also snapshots the length first, so a listener added during a
/// dispatch is not called by the dispatch that added it.
#[derive(Default)]
pub struct FlutterMemoryAllocations {
    listeners: RefCell<Vec<Option<ObjectEventListener>>>,
    active_dispatch_loops: Cell<usize>,
    listeners_contain_nulls: Cell<bool>,
}

thread_local! {
    static MEMORY_ALLOCATIONS: FlutterMemoryAllocations = FlutterMemoryAllocations::default();
}

impl FlutterMemoryAllocations {
    /// Upstream's `kFlutterMemoryAllocationsEnabled`, which is debug-only.
    pub const ENABLED: bool = cfg!(debug_assertions);

    /// Upstream's `FlutterMemoryAllocations.instance`, with its private
    /// constructor.
    pub fn with_instance<R>(body: impl FnOnce(&FlutterMemoryAllocations) -> R) -> R {
        MEMORY_ALLOCATIONS.with(body)
    }

    /// Upstream's `addListener`.
    pub fn add_listener(&self, listener: impl Fn(&ObjectEvent) + 'static) -> usize {
        if !FlutterMemoryAllocations::ENABLED {
            return usize::MAX;
        }
        let mut listeners = self.listeners.borrow_mut();
        listeners.push(Some(Rc::new(listener)));
        listeners.len() - 1
    }

    /// Upstream's `removeListener`. See the type's docs for why a dispatch in
    /// progress changes what this does.
    pub fn remove_listener(&self, token: usize) {
        if !FlutterMemoryAllocations::ENABLED {
            return;
        }
        let mut listeners = self.listeners.borrow_mut();
        let Some(slot) = listeners.get_mut(token) else {
            return;
        };
        if slot.take().is_some() && self.active_dispatch_loops.get() > 0 {
            self.listeners_contain_nulls.set(true);
        }
        drop(listeners);
        self.try_defragment_listeners();
    }

    /// Upstream's `_tryDefragmentListeners`, which does nothing until the last
    /// dispatch has finished.
    fn try_defragment_listeners(&self) {
        if self.active_dispatch_loops.get() > 0 {
            return;
        }
        self.listeners.borrow_mut().retain(|slot| slot.is_some());
        self.listeners_contain_nulls.set(false);
    }

    /// Upstream's `hasListeners`.
    pub fn has_listeners(&self) -> bool {
        if !FlutterMemoryAllocations::ENABLED {
            return false;
        }
        self.listeners.borrow().iter().any(|slot| slot.is_some())
    }

    /// Upstream's `dispatchObjectEvent`.
    pub fn dispatch_object_event(&self, event: &ObjectEvent) {
        if !FlutterMemoryAllocations::ENABLED {
            return;
        }
        if self.listeners.borrow().is_empty() {
            return;
        }
        self.active_dispatch_loops
            .set(self.active_dispatch_loops.get() + 1);
        // The length is read once, before anything is called: a listener added
        // during this dispatch is not called by it.
        let end = self.listeners.borrow().len();
        for index in 0..end {
            let listener = self.listeners.borrow().get(index).cloned().flatten();
            if let Some(listener) = listener {
                listener(event);
            }
        }
        self.active_dispatch_loops
            .set(self.active_dispatch_loops.get() - 1);
        if self.listeners_contain_nulls.get() {
            self.try_defragment_listeners();
        }
    }

    /// Upstream's `dispatchObjectCreated`, which is the shape the framework's
    /// own call sites use.
    pub fn dispatch_object_created(
        &self,
        object: u64,
        library: &'static str,
        class_name: &'static str,
    ) {
        self.dispatch_object_event(&ObjectEvent::created(object, library, class_name));
    }

    /// Upstream's `dispatchObjectDisposed`.
    pub fn dispatch_object_disposed(&self, object: u64) {
        self.dispatch_object_event(&ObjectEvent::disposed(object));
    }

    /// For tests: forget everything.
    pub fn reset(&self) {
        self.listeners.borrow_mut().clear();
        self.active_dispatch_loops.set(0);
        self.listeners_contain_nulls.set(false);
    }
}

#[cfg(test)]
mod memory_allocations_tests {
    use super::*;

    fn heard() -> Rc<RefCell<Vec<String>>> {
        Rc::new(RefCell::new(Vec::new()))
    }

    #[test]
    fn creation_carries_where_it_came_from_and_disposal_does_not() {
        // A tracker matches them by identity, so anything unmatched at the end
        // is a leak with a class name attached -- which is why only one side
        // needs to carry one.
        let created = ObjectEvent::created(7, "widgets", "Focus");
        assert_eq!(created.object(), 7);
        assert_eq!(
            created.to_map(),
            vec![
                ("libraryName", "widgets".to_string()),
                ("className", "Focus".to_string()),
                ("eventType", "created".to_string()),
            ]
        );
        assert_eq!(
            ObjectEvent::disposed(7).to_map(),
            vec![("eventType", "disposed".to_string())]
        );
    }

    #[test]
    fn a_listener_hears_both_kinds() {
        FlutterMemoryAllocations::with_instance(|allocations| {
            allocations.reset();
            let log = heard();
            let recorder = Rc::clone(&log);
            allocations.add_listener(move |event| {
                recorder.borrow_mut().push(format!("{:?}", event.object()))
            });
            assert!(allocations.has_listeners());

            allocations.dispatch_object_created(1, "widgets", "Focus");
            allocations.dispatch_object_disposed(1);
            assert_eq!(*log.borrow(), vec!["1", "1"]);
            allocations.reset();
        });
    }

    #[test]
    fn a_listener_may_remove_itself_while_it_is_being_called() {
        // And the ones after it must still be called -- removing an element
        // outright would shift them and skip one.
        FlutterMemoryAllocations::with_instance(|allocations| {
            allocations.reset();
            let log = heard();
            let first = Rc::new(std::cell::Cell::new(usize::MAX));

            let recorder = Rc::clone(&log);
            let token = Rc::clone(&first);
            first.set(allocations.add_listener(move |_| {
                recorder.borrow_mut().push("a".to_string());
                FlutterMemoryAllocations::with_instance(|a| a.remove_listener(token.get()));
            }));
            let recorder = Rc::clone(&log);
            allocations.add_listener(move |_| recorder.borrow_mut().push("b".to_string()));
            let recorder = Rc::clone(&log);
            allocations.add_listener(move |_| recorder.borrow_mut().push("c".to_string()));

            allocations.dispatch_object_disposed(1);
            assert_eq!(*log.borrow(), vec!["a", "b", "c"], "nobody was skipped");

            log.borrow_mut().clear();
            allocations.dispatch_object_disposed(2);
            assert_eq!(*log.borrow(), vec!["b", "c"], "and the removal took effect");
            allocations.reset();
        });
    }

    #[test]
    fn a_listener_added_during_a_dispatch_is_not_called_by_it() {
        // The length is read once, before anything runs.
        FlutterMemoryAllocations::with_instance(|allocations| {
            allocations.reset();
            let log = heard();
            let recorder = Rc::clone(&log);
            let late = Rc::clone(&log);
            allocations.add_listener(move |_| {
                recorder.borrow_mut().push("first".to_string());
                let late = Rc::clone(&late);
                FlutterMemoryAllocations::with_instance(move |a| {
                    let late = Rc::clone(&late);
                    a.add_listener(move |_| late.borrow_mut().push("late".to_string()));
                });
            });

            allocations.dispatch_object_disposed(1);
            assert_eq!(*log.borrow(), vec!["first"]);

            log.borrow_mut().clear();
            allocations.dispatch_object_disposed(2);
            assert!(log.borrow().contains(&"late".to_string()), "next time, yes");
            allocations.reset();
        });
    }

    #[test]
    fn removing_outside_a_dispatch_compacts_at_once() {
        FlutterMemoryAllocations::with_instance(|allocations| {
            allocations.reset();
            let log = heard();
            let recorder = Rc::clone(&log);
            let token = allocations.add_listener(move |_| {
                recorder.borrow_mut().push("a".to_string());
            });
            let recorder = Rc::clone(&log);
            allocations.add_listener(move |_| recorder.borrow_mut().push("b".to_string()));

            allocations.remove_listener(token);
            allocations.dispatch_object_disposed(1);
            assert_eq!(*log.borrow(), vec!["b"]);
            assert!(allocations.has_listeners());
            allocations.reset();
        });
    }

    #[test]
    fn dispatching_with_nobody_listening_is_not_an_error() {
        FlutterMemoryAllocations::with_instance(|allocations| {
            allocations.reset();
            assert!(!allocations.has_listeners());
            allocations.dispatch_object_created(1, "widgets", "Focus");
            allocations.reset();
        });
    }

    #[test]
    fn removing_a_token_twice_or_one_that_was_never_given_is_not_an_error() {
        FlutterMemoryAllocations::with_instance(|allocations| {
            allocations.reset();
            let token = allocations.add_listener(|_| {});
            allocations.remove_listener(token);
            allocations.remove_listener(token);
            allocations.remove_listener(9999);
            assert!(!allocations.has_listeners());
            allocations.reset();
        });
    }
}
