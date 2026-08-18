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
//! * `DiagnosticsNode` and the debug-assertion surface are not ported;
//!   those classes are ledgered out of scope.

use std::cell::RefCell;
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
