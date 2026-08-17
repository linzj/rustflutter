// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Which widget the keyboard is talking to.
//!
//! A pointer carries its own answer -- it has a position, and the tree can be
//! asked what is under it. A key carries nothing. Something has to decide
//! where it goes, and that decision is what focus is.
//!
//! Upstream this is `FocusNode`, `FocusScopeNode` and `FocusManager`, plus
//! `FocusTraversalPolicy` for deciding where Tab goes next -- sixteen thousand
//! lines, of which `focus_traversal.dart` alone is two and a half thousand
//! spent entirely on that one question. What is here is the part every
//! application needs: a set of nodes, one of them focused, keys delivered to it
//! and then to its ancestors, and Tab moving between them.
//!
//! # Registered by building
//!
//! A [`FocusNode`] here is registered by the build that draws it, every frame,
//! and the registration order is the traversal order. Upstream a node is a
//! long-lived object attached to the tree, and the default traversal is
//! `ReadingOrderTraversalPolicy`, which sorts by where things ended up on
//! screen. The order used here is upstream's other policy --
//! `WidgetOrderTraversalPolicy`, "the order the widgets were built in" -- which
//! is the one that needs no geometry. For a form built top to bottom the two
//! agree.
//!
//! What survives a frame is the *focused id*, not the node: a widget that
//! rebuilds keeps focus because it registers the same id again.

use std::cell::RefCell;
use std::rc::Rc;

use crate::framework::{
    AnyWidget, BuildContext, Component, ElementRef, StateHandle, component, single,
};
use crate::gestures::PointerHandlers;
use crate::keyboard::{KeyEvent, Keyboard, LogicalKey};

/// What a focused widget does with a key: took it, or passed it on.
///
/// Upstream's `KeyEventResult`, minus `skipRemainingHandlers`, which exists
/// there for a case this has no equivalent of (a platform view swallowing the
/// key without handling it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyResult {
    Handled,
    Ignored,
}

type KeyHandler = Rc<dyn Fn(&KeyEvent) -> KeyResult>;

/// One place the keyboard can be.
#[derive(Clone)]
struct FocusEntry {
    id: u64,
    /// Which element put it here, so that it can be taken away again when that
    /// element goes. Upstream a `FocusNode` is disposed by the `State` that
    /// made it; this is the same lifetime said in the terms this framework
    /// has.
    element: ElementRef,
    /// Ancestors, outermost first: a key the focused node ignores walks back
    /// up this. Upstream this is the node's parent chain.
    ancestors: Vec<u64>,
    on_key: Option<KeyHandler>,
    /// Whether Tab can land here. A node that only wants to *know* whether it
    /// is focused -- a row that highlights itself -- is not a stop on the way.
    traversable: bool,
    on_focus_change: Option<Rc<dyn Fn(bool)>>,
}

/// The chain of focus nodes a subtree is inside, outermost first.
///
/// Published to the subtree by each [`Focus`], which is how a node registered
/// deeper in learns what its ancestors are. Upstream `Focus` does exactly this
/// -- it inserts an inherited widget carrying its node, and a descendant reads
/// it to find its parent -- and it has to be published rather than tracked in a
/// stack, because a child widget is *built* long after the parent's build
/// returned.
#[derive(Clone, Debug, Default, PartialEq)]
struct FocusScope(Vec<u64>);

/// The nodes, and where the keyboard is.
#[derive(Default)]
struct FocusManager {
    entries: Vec<FocusEntry>,
    focused: Option<u64>,
}

thread_local! {
    /// One per UI thread, which is one per view: focus is shell state rather
    /// than application state, the same as the pressed-key set and the text
    /// input connection.
    static MANAGER: RefCell<FocusManager> = RefCell::new(FocusManager::default());
}

/// Drops the nodes whose elements have gone.
///
/// Called once per frame, before building. The registry is *not* rebuilt each
/// frame: a node lives as long as the element that registered it, which is
/// upstream's arrangement -- a `FocusNode` is made in `initState` and disposed
/// in `dispose`, and nothing about it is per-frame. It has to be that way here
/// too, because only dirty elements rebuild and only changed subtrees produce
/// new render objects, so "everything that is on screen" is not a thing any
/// one frame walks past.
///
/// Focus itself follows: a node that is gone cannot hold the keyboard, which
/// is what upstream does when a focused widget is disposed.
pub fn prune(is_live: impl Fn(ElementRef) -> bool) {
    MANAGER.with(|manager| {
        let mut manager = manager.borrow_mut();
        manager.entries.retain(|entry| is_live(entry.element));
        if let Some(focused) = manager.focused {
            if !manager.entries.iter().any(|entry| entry.id == focused) {
                manager.focused = None;
            }
        }
    });
}

/// Forgets every node and where the keyboard was.
///
/// Only for tests: the registry outlives frames now, so a test that mounts its
/// own tree has to say when the last one stopped existing. Nothing in a
/// running application calls this -- `prune` is the real thing.
#[cfg(test)]
pub(crate) fn reset() {
    MANAGER.with(|manager| *manager.borrow_mut() = FocusManager::default());
}

/// Adds a node, or replaces what an id already had.
///
/// Replacing in place rather than appending is what keeps Tab order stable
/// across a rebuild: the position in this list is the order the tree was
/// walked in when the node first appeared, and rebuilding a widget does not
/// move it. New handlers do replace the old ones, because they close over the
/// state the rebuild produced.
fn register(entry: FocusEntry) {
    MANAGER.with(|manager| {
        let mut manager = manager.borrow_mut();
        match manager
            .entries
            .iter()
            .position(|existing| existing.id == entry.id)
        {
            Some(index) => manager.entries[index] = entry,
            None => manager.entries.push(entry),
        }
    });
}

/// Which node has the keyboard, if any.
pub fn focused() -> Option<u64> {
    MANAGER.with(|manager| manager.borrow().focused)
}

/// Whether `id` has the keyboard.
pub fn has_focus(id: u64) -> bool {
    focused() == Some(id)
}

/// Moves the keyboard to `id`. Returns whether anything changed.
///
/// `id` must name a registered node: upstream's `FocusNode.requestFocus` on a
/// node that is not attached to the tree is a no-op -- there is nothing to
/// hand the keyboard to -- and so is this, leaving focus exactly where it was
/// rather than parked on an id nothing owns.
pub fn focus(id: u64) -> bool {
    let (changed, lost, gained) = MANAGER.with(|manager| {
        let mut manager = manager.borrow_mut();
        if manager.focused == Some(id) {
            return (false, None, None);
        }
        // An unregistered id is not a node. Focusing it anyway would move the
        // keyboard somewhere no widget can give it back.
        if !manager.entries.iter().any(|entry| entry.id == id) {
            return (false, None, None);
        }
        let lost = manager
            .focused
            .and_then(|old| manager.entries.iter().find(|e| e.id == old))
            .and_then(|entry| entry.on_focus_change.clone());
        let gained = manager
            .entries
            .iter()
            .find(|e| e.id == id)
            .and_then(|entry| entry.on_focus_change.clone());
        manager.focused = Some(id);
        (true, lost, gained)
    });
    // Outside the borrow: a handler is free to focus something else, and a
    // manager borrowed across a call into application code is a panic waiting
    // for the first application that does.
    if let Some(lost) = lost {
        lost(false);
    }
    if let Some(gained) = gained {
        gained(true);
    }
    changed
}

/// Takes the keyboard away from everything.
pub fn unfocus() {
    let lost = MANAGER.with(|manager| {
        let mut manager = manager.borrow_mut();
        let lost = manager
            .focused
            .and_then(|old| manager.entries.iter().find(|e| e.id == old))
            .and_then(|entry| entry.on_focus_change.clone());
        manager.focused = None;
        lost
    });
    if let Some(lost) = lost {
        lost(false);
    }
}

/// Moves focus to the next traversable node, wrapping at the end.
///
/// Upstream's `NextFocusIntent`, resolved by the traversal policy. Returns
/// whether there was anywhere to go.
pub fn next() -> bool {
    step(1)
}

/// Moves focus to the previous traversable node. Upstream's
/// `PreviousFocusIntent`.
pub fn previous() -> bool {
    step(-1)
}

fn step(direction: isize) -> bool {
    let target = MANAGER.with(|manager| {
        let manager = manager.borrow();
        let stops: Vec<u64> = manager
            .entries
            .iter()
            .filter(|e| e.traversable)
            .map(|e| e.id)
            .collect();
        if stops.is_empty() {
            return None;
        }
        let current = manager
            .focused
            .and_then(|id| stops.iter().position(|s| *s == id));
        let next = match current {
            Some(index) => {
                let count = stops.len() as isize;
                (((index as isize + direction) % count + count) % count) as usize
            }
            // Nothing focused: Tab goes to the first, Shift+Tab to the last.
            None if direction > 0 => 0,
            None => stops.len() - 1,
        };
        Some(stops[next])
    });
    match target {
        Some(id) => focus(id),
        None => false,
    }
}

/// Offers a key to the focused node and then to its ancestors.
///
/// Returns whether anything took it. Upstream this is
/// `FocusManager.handleKeyMessage`, which walks the same chain for the same
/// reason: a shortcut belongs to a region of the screen, and the region is the
/// ancestor of whatever is focused inside it.
pub fn dispatch_key(event: &KeyEvent) -> bool {
    let chain: Vec<KeyHandler> = MANAGER.with(|manager| {
        let manager = manager.borrow();
        let Some(focused) = manager.focused else {
            return Vec::new();
        };
        let Some(entry) = manager.entries.iter().find(|e| e.id == focused) else {
            return Vec::new();
        };
        let mut chain: Vec<KeyHandler> = Vec::new();
        if let Some(handler) = &entry.on_key {
            chain.push(Rc::clone(handler));
        }
        // Innermost ancestor first, which is why this walks the recorded chain
        // backwards: it was recorded outermost first.
        for ancestor in entry.ancestors.iter().rev() {
            if let Some(handler) = manager
                .entries
                .iter()
                .find(|e| e.id == *ancestor)
                .and_then(|e| e.on_key.as_ref())
            {
                chain.push(Rc::clone(handler));
            }
        }
        chain
    });

    for handler in chain {
        if handler(event) == KeyResult::Handled {
            return true;
        }
    }
    false
}

/// Handles Tab, if nothing else did.
///
/// Upstream this is a `Shortcuts` widget installed by `WidgetsApp` mapping Tab
/// to `NextFocusIntent` and Shift+Tab to `PreviousFocusIntent`. It is a
/// default rather than a rule -- an application that wants Tab for something
/// else handles it first, and this never sees it.
pub fn handle_traversal_key(event: &KeyEvent, keyboard: &Keyboard) -> bool {
    if !event.is_down() || event.logical != LogicalKey::TAB {
        return false;
    }
    // Shift comes from the pressed set rather than from the event: an event
    // says what changed, and whether another key is held is a question about
    // something else. See `Keyboard`.
    if keyboard.shift() { previous() } else { next() }
}

/// A widget that can hold the keyboard.
///
/// ```ignore
/// component(Focus::new(7, component(SearchBox)).with_on_key(|key| {
///     if key.logical == LogicalKey::ESCAPE { clear(); KeyResult::Handled }
///     else { KeyResult::Ignored }
/// }))
/// ```
pub struct Focus {
    id: u64,
    child: RefCell<Option<AnyWidget>>,
    on_key: Option<KeyHandler>,
    on_focus_change: Option<Rc<dyn Fn(bool)>>,
    traversable: bool,
    /// Whether tapping this region focuses it. On by default, because a
    /// reader who taps something expects to be typing into it.
    focus_on_tap: bool,
}

impl Focus {
    pub fn new(id: u64, child: AnyWidget) -> Focus {
        Focus {
            id,
            child: RefCell::new(Some(child)),
            on_key: None,
            on_focus_change: None,
            traversable: true,
            focus_on_tap: true,
        }
    }

    pub fn with_on_key(mut self, handler: impl Fn(&KeyEvent) -> KeyResult + 'static) -> Self {
        self.on_key = Some(Rc::new(handler));
        self
    }

    /// Called when this node gains or loses the keyboard.
    pub fn with_on_focus_change(mut self, handler: impl Fn(bool) + 'static) -> Self {
        self.on_focus_change = Some(Rc::new(handler));
        self
    }

    /// Whether Tab stops here. A scope that only wants the keys of whatever is
    /// focused inside it says no.
    pub fn with_traversable(mut self, traversable: bool) -> Self {
        self.traversable = traversable;
        self
    }

    pub fn with_focus_on_tap(mut self, focus_on_tap: bool) -> Self {
        self.focus_on_tap = focus_on_tap;
        self
    }

    /// Marks the subtree dirty when focus changes, so a widget that draws
    /// itself differently when focused is rebuilt.
    pub fn wired<S: 'static>(self, handle: StateHandle<S>, touch: fn(&mut S)) -> Self {
        self.with_on_focus_change(move |_| {
            handle.set_state(move |state| touch(state));
        })
    }
}

impl Component for Focus {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let scope = context
            .inherited::<FocusScope>()
            .map(|s| s.0.clone())
            .unwrap_or_default();
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        // Anything focusable below this is inside it, and says so by reading
        // what this publishes.
        let mut inner = scope.clone();
        inner.push(self.id);
        let built = crate::framework::provide(FocusScope(inner), child);

        // Registered from the build, which is where upstream registers too
        // (`_FocusState.initState` and `didUpdateWidget`). It survives the
        // frames this widget does not rebuild, because the registry is not
        // rebuilt per frame -- see `prune`.
        register(FocusEntry {
            id: self.id,
            element: context.element_ref(),
            ancestors: scope,
            on_key: self.on_key.clone(),
            traversable: self.traversable,
            on_focus_change: self.on_focus_change.clone(),
        });

        let id = self.id;
        let focus_on_tap = self.focus_on_tap;
        single(built, move |child| {
            let mut handlers = PointerHandlers::new();
            if focus_on_tap {
                handlers = handlers.with_tap(move |_| {
                    focus(id);
                });
            }
            Box::new(crate::render::RenderPointerRegion::new(id, child).with_handlers(handlers))
        })
    }
}

/// [`Focus`] as a widget.
pub fn focusable(id: u64, child: AnyWidget) -> AnyWidget {
    component(Focus::new(id, child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, leaf};
    use crate::keyboard::{KeyChange, PhysicalKey};
    use crate::widgets::{Column, Empty, SizedBox};

    fn key(logical: LogicalKey) -> KeyEvent {
        KeyEvent {
            change: KeyChange::Down,
            physical: PhysicalKey(0),
            logical,
            character: None,
            time_stamp_micros: 0,
            synthesized: false,
        }
    }

    /// A keyboard with shift held, for the Shift+Tab case.
    fn with_shift() -> Keyboard {
        let mut keyboard = Keyboard::new();
        let mut down = KeyEvent {
            physical: PhysicalKey::SHIFT_LEFT,
            logical: LogicalKey::SHIFT_LEFT,
            ..key(LogicalKey::TAB)
        };
        keyboard.record(&mut down);
        keyboard
    }

    /// Two focusable boxes in a column, mounted and rendered -- nodes
    /// register when the render objects are made, which is what the host does
    /// once per frame.
    fn two_fields() -> ElementTree {
        reset();
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                focusable(1, leaf(|| SizedBox::new(10.0, 10.0))),
                focusable(2, leaf(|| SizedBox::new(10.0, 10.0))),
            ],
            |children| {
                let mut column = Column::new();
                for child in children {
                    column = column.push(child);
                }
                Box::new(column)
            },
        ));
        let _ = tree.build_render_tree();
        tree
    }

    #[test]
    fn nothing_is_focused_to_start_with() {
        let _tree = two_fields();
        assert_eq!(focused(), None);
    }

    #[test]
    fn tab_walks_the_nodes_in_the_order_they_were_built() {
        let _tree = two_fields();
        assert!(next());
        assert_eq!(focused(), Some(1));
        assert!(next());
        assert_eq!(focused(), Some(2));
        // And wraps, which is what a form with two fields should do.
        assert!(next());
        assert_eq!(focused(), Some(1));
    }

    #[test]
    fn shift_tab_walks_the_other_way() {
        let _tree = two_fields();
        assert!(previous());
        assert_eq!(focused(), Some(2), "backwards from nothing is the last one");
        assert!(previous());
        assert_eq!(focused(), Some(1));
    }

    #[test]
    fn tab_is_the_default_and_only_the_default() {
        let _tree = two_fields();
        let plain = Keyboard::new();
        assert!(handle_traversal_key(&key(LogicalKey::TAB), &plain));
        assert_eq!(focused(), Some(1));
        assert!(handle_traversal_key(&key(LogicalKey::TAB), &with_shift()));
        assert_eq!(
            focused(),
            Some(2),
            "shift+tab from the first wraps to the last"
        );
        assert!(!handle_traversal_key(&key(LogicalKey::ESCAPE), &plain));
    }

    #[test]
    fn a_key_goes_to_the_focused_node() {
        reset();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let first = seen.clone();
        let second = seen.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                component(Focus::new(1, leaf(|| Empty)).with_on_key(move |_| {
                    first.borrow_mut().push(1);
                    KeyResult::Handled
                })),
                component(Focus::new(2, leaf(|| Empty)).with_on_key(move |_| {
                    second.borrow_mut().push(2);
                    KeyResult::Handled
                })),
            ],
            |children| {
                let mut column = Column::new();
                for child in children {
                    column = column.push(child);
                }
                Box::new(column)
            },
        ));

        let _ = tree.build_render_tree();
        focus(2);
        assert!(dispatch_key(&key(LogicalKey::ENTER)));
        assert_eq!(
            *seen.borrow(),
            vec![2],
            "the unfocused node should hear nothing"
        );
        drop(tree);
    }

    #[test]
    fn a_key_the_focused_node_ignores_goes_to_its_ancestor() {
        reset();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let inner = seen.clone();
        let outer = seen.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            Focus::new(
                10,
                component(Focus::new(11, leaf(|| Empty)).with_on_key(move |_| {
                    inner.borrow_mut().push("inner");
                    KeyResult::Ignored
                })),
            )
            .with_on_key(move |_| {
                outer.borrow_mut().push("outer");
                KeyResult::Handled
            }),
        ));

        let _ = tree.build_render_tree();
        focus(11);
        assert!(dispatch_key(&key(LogicalKey::ESCAPE)));
        assert_eq!(
            *seen.borrow(),
            vec!["inner", "outer"],
            "innermost first, then out"
        );
        drop(tree);
    }

    #[test]
    fn losing_focus_is_reported_as_well_as_gaining_it() {
        reset();
        let log = Rc::new(RefCell::new(Vec::new()));
        let first = log.clone();
        let second = log.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                component(
                    Focus::new(1, leaf(|| Empty)).with_on_focus_change(move |has| {
                        first.borrow_mut().push(format!("1:{has}"));
                    }),
                ),
                component(
                    Focus::new(2, leaf(|| Empty)).with_on_focus_change(move |has| {
                        second.borrow_mut().push(format!("2:{has}"));
                    }),
                ),
            ],
            |children| {
                let mut column = Column::new();
                for child in children {
                    column = column.push(child);
                }
                Box::new(column)
            },
        ));

        let _ = tree.build_render_tree();
        focus(1);
        focus(2);
        assert_eq!(*log.borrow(), vec!["1:true", "1:false", "2:true"]);
        drop(tree);
    }

    #[test]
    fn focusing_an_id_nothing_registered_changes_nothing() {
        // Upstream `requestFocus` on a detached node is a no-op: the keyboard
        // stays with whatever held it, and nothing is told it moved -- not the
        // focused node, which did not lose it, and not the id asked for,
        // which has no node to be told anything.
        let log = Rc::new(RefCell::new(Vec::new()));
        let first = log.clone();
        let second = log.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                component(
                    Focus::new(1, leaf(|| Empty)).with_on_focus_change(move |has| {
                        first.borrow_mut().push(format!("1:{has}"));
                    }),
                ),
                component(
                    Focus::new(2, leaf(|| Empty)).with_on_focus_change(move |has| {
                        second.borrow_mut().push(format!("2:{has}"));
                    }),
                ),
            ],
            |children| {
                let mut column = Column::new();
                for child in children {
                    column = column.push(child);
                }
                Box::new(column)
            },
        ));
        let _ = tree.build_render_tree();
        focus(1);
        assert_eq!(*log.borrow(), vec!["1:true".to_string()]);

        assert!(!focus(999), "there is no node 999 to focus");
        assert_eq!(focused(), Some(1), "and focus stayed where it was");
        assert_eq!(
            *log.borrow(),
            vec!["1:true".to_string()],
            "nothing was told the keyboard moved"
        );
        drop(tree);
    }

    #[test]
    fn a_frame_that_rebuilds_nothing_keeps_its_focus_nodes() {
        // The bug this rules out: a registry rebuilt once per frame is a
        // registry that empties on the first frame with nothing to do, and
        // only dirty elements rebuild. Tab would stop working until something
        // happened to change.
        let mut tree = two_fields();
        focus(2);

        // A frame in which nothing at all happens.
        prune(|element| tree.is_live(element));
        assert_eq!(tree.rebuild_dirty(), 0, "nothing is dirty");
        let _ = tree.build_render_tree();

        assert_eq!(focused(), Some(2));
        assert!(next(), "Tab still has somewhere to go");
        assert_eq!(focused(), Some(1), "and it wrapped round to the first");
    }

    #[test]
    fn a_node_whose_widget_is_gone_holds_nothing() {
        let mut tree = two_fields();
        focus(2);
        assert_eq!(focused(), Some(2));

        // The same shape of tree with the focusables taken out of it. Their
        // elements are released, so their nodes go with them -- and the
        // keyboard cannot be somewhere that no longer exists.
        tree.rebuild(crate::framework::many(
            vec![
                leaf(|| SizedBox::new(10.0, 10.0)),
                leaf(|| SizedBox::new(10.0, 10.0)),
            ],
            |children| {
                let mut column = Column::new();
                for child in children {
                    column = column.push(child);
                }
                column
            },
        ));
        prune(|element| tree.is_live(element));

        assert_eq!(focused(), None, "a disposed node cannot hold the keyboard");
        assert!(!dispatch_key(&key(LogicalKey::ENTER)));
        assert!(!next(), "and nowhere for Tab to go");
    }
}
