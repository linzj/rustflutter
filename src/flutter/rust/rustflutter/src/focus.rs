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

use std::cell::{Cell, RefCell};
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
    /// The explicit traversal order this node was given, if any --
    /// upstream's `FocusTraversalOrder` read off the node's context.
    order: Option<FocusOrder>,
    /// The innermost [`FocusTraversalGroup`] this node is inside, if any --
    /// its *enclosing* group, so that a group node itself sits among its
    /// parent's members and stands in for its whole subtree there.
    group: Option<u64>,
    /// Whether this node is a [`FocusTraversalGroup`] boundary.
    is_group: bool,
    /// Upstream's `descendantsAreFocusable`: whether anything **below** this
    /// node may take the keyboard.
    descendants_focusable: bool,
    /// Upstream's `descendantsAreTraversable`: whether Tab may wander below.
    ///
    /// Independent of the one above, and the pair is the point. A subtree can
    /// be focusable and not traversable -- a page under a dialog, say, which
    /// something may still focus deliberately while Tab must stay in the
    /// dialog -- and there is no way to say that with one flag.
    descendants_traversable: bool,
}

/// Upstream's `FocusNode.canRequestFocus`:
/// `_canRequestFocus && ancestors.every((a) => a.descendantsAreFocusable)`.
///
/// **Every** ancestor has to allow it. One that says no is enough, however far
/// up it is, which is what makes `ExcludeFocus` around a whole page work.
fn can_take_focus(manager: &FocusManager, id: u64) -> bool {
    let Some(entry) = manager.entries.iter().find(|entry| entry.id == id) else {
        return false;
    };
    entry.ancestors.iter().all(|ancestor| {
        manager
            .entries
            .iter()
            .find(|other| other.id == *ancestor)
            .is_none_or(|other| other.descendants_focusable)
    })
}

/// Upstream's `FocusNode.skipTraversal`:
/// `_skipTraversal || ancestors.any((a) => !a.descendantsAreTraversable)`.
///
/// The dual of the one above, and the shapes differ on purpose: focusability
/// is an **and** over permission, traversability an **or** over refusal. They
/// come to the same thing per ancestor and read as opposites, which is why
/// each is written the way upstream writes it rather than one in terms of the
/// other.
fn skips_traversal(manager: &FocusManager, entry: &FocusEntry) -> bool {
    if !entry.traversable {
        return true;
    }
    // Upstream's `traversalDescendants` is
    // `where((node) => !node.skipTraversal && node.canRequestFocus)`, so a
    // node that cannot take the keyboard is not a stop on the way to it
    // either. **The implication runs one way**: unfocusable is untraversable,
    // and untraversable is not unfocusable. The first draft of this file had
    // only the two ancestor walks and let Tab land on a node that then refused
    // the focus -- which reads as a dead key press.
    if !can_take_focus(manager, entry.id) {
        return true;
    }
    entry.ancestors.iter().any(|ancestor| {
        manager
            .entries
            .iter()
            .find(|other| other.id == *ancestor)
            .is_some_and(|other| !other.descendants_traversable)
    })
}

/// Upstream `FocusOrder`: where a node goes in an explicit traversal order.
///
/// Upstream this is an abstract class with two subclasses, and comparing two
/// of different subclasses is an assertion failure -- there is no meaning to
/// "is 3.0 before or after \"b\"". Here they are two variants, and the
/// comparison of a mixed pair falls back to numeric-before-lexical rather
/// than failing: an assertion that fires in debug and orders arbitrarily in
/// release is worse than one rule that always holds.
#[derive(Clone, Debug, PartialEq)]
pub enum FocusOrder {
    /// Upstream `NumericFocusOrder`.
    Numeric(f32),
    /// Upstream `LexicalFocusOrder`.
    Lexical(String),
}

impl FocusOrder {
    /// Upstream `FocusOrder.compareTo`, which is `doCompare` on the subclass.
    fn compare(&self, other: &FocusOrder) -> std::cmp::Ordering {
        match (self, other) {
            (FocusOrder::Numeric(a), FocusOrder::Numeric(b)) => {
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
            }
            (FocusOrder::Lexical(a), FocusOrder::Lexical(b)) => a.cmp(b),
            // Mixed: upstream asserts. See the type's own note.
            (FocusOrder::Numeric(_), FocusOrder::Lexical(_)) => std::cmp::Ordering::Less,
            (FocusOrder::Lexical(_), FocusOrder::Numeric(_)) => std::cmp::Ordering::Greater,
        }
    }
}

/// Upstream `NumericFocusOrder`.
pub struct NumericFocusOrder;

impl NumericFocusOrder {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(order: f32) -> FocusOrder {
        FocusOrder::Numeric(order)
    }
}

/// Upstream `LexicalFocusOrder`.
pub struct LexicalFocusOrder;

impl LexicalFocusOrder {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(order: impl Into<String>) -> FocusOrder {
        FocusOrder::Lexical(order.into())
    }
}

/// Upstream `FocusTraversalOrder`: gives the focus nodes below it an
/// explicit place in the traversal order.
///
/// Upstream it is an `InheritedNotifier` the policy reads off each node's
/// context; here it is published the same way, and the [`Focus`] below picks
/// it up in its own build.
pub struct FocusTraversalOrder;

impl FocusTraversalOrder {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(order: FocusOrder, child: AnyWidget) -> AnyWidget {
        crate::framework::provide(order, child)
    }
}

/// Upstream `FocusTraversalGroup`: the nodes inside it are traversed
/// together, before traversal moves on to whatever is outside.
///
/// Upstream a group also carries the policy its descendants are sorted by.
/// The policies here are not objects (see the ledger: `WidgetOrder` is the
/// registration order and `Ordered` is [`OrderedTraversalPolicy`], which is
/// always in force), so a group is the grouping and nothing else.
pub struct FocusTraversalGroup;

impl FocusTraversalGroup {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: u64, child: AnyWidget) -> AnyWidget {
        component(Focus::new(id, child).with_traversable(false).as_group())
    }

    /// Upstream's `descendantsAreFocusable: false`: nothing inside may take
    /// the keyboard, however it is asked.
    ///
    /// This is what `ExcludeFocus` is built out of upstream, and the reason it
    /// is a *group* flag rather than a per-node one: a page put behind a
    /// dialog says it once, and every control on it stops answering.
    #[allow(clippy::new_ret_no_self)]
    pub fn unfocusable(id: u64, child: AnyWidget) -> AnyWidget {
        component(
            Focus::new(id, child)
                .with_traversable(false)
                .with_descendants_focusable(false)
                .as_group(),
        )
    }

    /// Upstream's `descendantsAreTraversable: false`: Tab does not wander in,
    /// but something may still focus a node inside deliberately.
    ///
    /// The pair with [`FocusTraversalGroup::unfocusable`] is the point. A
    /// subtree that is untraversable but focusable is a page under a dialog
    /// whose fields an application may still address; one that is unfocusable
    /// is a page nothing may touch at all. One flag cannot say both.
    #[allow(clippy::new_ret_no_self)]
    pub fn untraversable(id: u64, child: AnyWidget) -> AnyWidget {
        component(
            Focus::new(id, child)
                .with_traversable(false)
                .with_descendants_traversable(false)
                .as_group(),
        )
    }
}

/// Upstream `OrderedTraversalPolicy`: nodes with an explicit
/// [`FocusOrder`] first, in that order, then the rest in the order they
/// registered.
///
/// Upstream you choose this policy; here it is always the rule, because with
/// no orders anywhere it is exactly the registration order --
/// `WidgetOrderTraversalPolicy`, which is what this always was.
pub struct OrderedTraversalPolicy;

impl OrderedTraversalPolicy {
    /// Upstream `sortDescendants`'s comparison: the ordered ones first, in
    /// their order, and the unordered ones after them.
    ///
    /// Equal for two unordered nodes, so a stable sort leaves them in the
    /// order they registered -- which is `WidgetOrderTraversalPolicy`, the
    /// secondary policy this falls back to.
    fn compare(first: &Option<FocusOrder>, second: &Option<FocusOrder>) -> std::cmp::Ordering {
        match (first, second) {
            (Some(first), Some(second)) => first.compare(second),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }
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
struct FocusAncestors(Vec<u64>);

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
// -- Scopes -------------------------------------------------------------------

/// Upstream `FocusScope` (`widgets/focus_scope.dart`), built on upstream's
/// `FocusScopeNode`.
///
/// A scope is a [`Focus`] that **remembers**. Focusing a plain node moves the
/// keyboard to that node; focusing a scope moves it to whichever descendant
/// held it last, and only falls back to the first one if the scope has never
/// been entered.
///
/// # What it is for
///
/// Two panes, or a dialog over a page, or a tab bar's pages: the reader types
/// in one, moves away, comes back -- and expects to be back where they were,
/// not at the top. Without a scope the framework has nowhere to keep "where
/// they were", because the node that had focus is one of many and nothing
/// distinguishes it afterwards.
///
/// # How this differs from upstream's, and it does
///
/// Upstream's `FocusScopeNode` keeps a **stack of its immediate children**
/// (`_focusedChildren`, most recent last), each of which may itself be a scope,
/// and restoring walks down that chain -- `_doRequestFocus(findFirstFocus:
/// true)` descends until it reaches a node that takes focus. This crate's
/// registry is flat: a node knows its ancestor chain but a scope does not hold
/// its children.
///
/// So a scope here remembers **the descendant that most recently held focus**,
/// at any depth, rather than the immediate child that leads to it. The two
/// agree whenever nothing changed in between, which is the case a reader
/// notices. They differ when a nested scope's own memory has moved on since:
/// upstream would descend and land on the inner scope's *current* choice, and
/// this lands on the node that was actually last focused. Written down rather
/// than papered over.
pub struct FocusScope {
    focus: Focus,
}

impl FocusScope {
    pub fn new(id: u64, child: AnyWidget) -> FocusScope {
        FocusScope {
            // Not a tab stop itself. Upstream's scope has
            // `skipTraversal` left alone but is not something Tab lands on --
            // it is the thing containing the stops.
            focus: Focus::new(id, child).with_traversable(false),
        }
    }

    /// The scope's id, which is what [`focused_child`] and
    /// [`focus_scope`](fn@focus_scope) are asked about.
    pub fn id(&self) -> u64 {
        self.focus.id
    }

    pub fn with_on_key(mut self, handler: impl Fn(&KeyEvent) -> KeyResult + 'static) -> Self {
        self.focus = self.focus.with_on_key(handler);
        self
    }

    pub fn with_on_focus_change(mut self, handler: impl Fn(bool) + 'static) -> Self {
        self.focus = self.focus.with_on_focus_change(handler);
        self
    }

    /// Whether tapping the scope's own area focuses it -- and so restores its
    /// remembered child. Off by default, because a scope covers everything
    /// inside it and a tap on a child would otherwise be a tap on the scope
    /// too.
    pub fn with_focus_on_tap(mut self, focus_on_tap: bool) -> Self {
        self.focus = self.focus.with_focus_on_tap(focus_on_tap);
        self
    }
}

impl Component for FocusScope {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        register_scope(self.focus.id);
        self.focus.build(context)
    }
}

/// [`FocusScope`] as a widget.
pub fn focus_scope_widget(id: u64, child: AnyWidget) -> AnyWidget {
    component(FocusScope::new(id, child).with_focus_on_tap(false))
}

thread_local! {
    /// What each scope last had focused inside it. Upstream's
    /// `FocusScopeNode._focusedChildren`, flattened -- see [`FocusScope`].
    static SCOPE_MEMORY: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
    /// Which ids are scopes. A node's ancestor chain is just ids, so this is
    /// how the walk tells a scope from an ordinary ancestor.
    static SCOPES: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Marks `id` as a scope. Idempotent: a scope re-registers on every build, as
/// its [`Focus`] does.
fn register_scope(id: u64) {
    SCOPES.with(|scopes| {
        let mut scopes = scopes.borrow_mut();
        if !scopes.contains(&id) {
            scopes.push(id);
        }
    });
}

/// Whether `id` names a scope.
pub fn is_scope(id: u64) -> bool {
    SCOPES.with(|scopes| scopes.borrow().contains(&id))
}

/// Upstream's `FocusScopeNode.focusedChild`: what this scope would restore.
///
/// `None` for a scope nothing has been focused inside yet, and for an id that
/// is not a scope at all.
pub fn focused_child(scope: u64) -> Option<u64> {
    SCOPE_MEMORY.with(|memory| {
        memory
            .borrow()
            .iter()
            .find(|(id, _)| *id == scope)
            .map(|(_, child)| *child)
    })
}

/// Records that `node` holds the keyboard, in every scope that encloses it.
///
/// Upstream's `_setAsFocusedChildForScope`, which walks up doing the same. Every
/// enclosing scope is told, not only the nearest, because a reader leaving an
/// outer scope and coming back expects the same place as one leaving the inner
/// one -- and the outer scope has no way to ask the inner one later.
fn remember_focus(node: u64) {
    let ancestors = MANAGER.with(|manager| {
        manager
            .borrow()
            .entries
            .iter()
            .find(|entry| entry.id == node)
            .map(|entry| entry.ancestors.clone())
    });
    let Some(ancestors) = ancestors else {
        return;
    };
    SCOPE_MEMORY.with(|memory| {
        let mut memory = memory.borrow_mut();
        for scope in ancestors.into_iter().filter(|id| is_scope(*id)) {
            match memory.iter_mut().find(|(id, _)| *id == scope) {
                Some(slot) => slot.1 = node,
                None => memory.push((scope, node)),
            }
        }
    });
}

/// Upstream's `FocusScopeNode.requestFocus`: give the keyboard to whatever this
/// scope had last, or to its first traversable descendant if it has had none.
///
/// Answers whether the keyboard moved.
pub fn focus_scope(scope: u64) -> bool {
    if let Some(child) = focused_child(scope) {
        // The remembered node may have gone since -- a list row scrolled out of
        // the registry, a dialog's field dismissed. `focus` refuses an
        // unregistered id, so fall through to the first descendant.
        if focus(child) {
            return true;
        }
        if has_focus(child) {
            return false;
        }
    }
    match first_focusable_in(scope) {
        Some(first) => focus(first),
        None => false,
    }
}

/// The first traversable node inside `scope`, in traversal order.
///
/// Upstream's `findFirstFocus`, which walks the scope's children in the order
/// the policy sorts them. Here that is [`traversal_order`], filtered to the ones
/// inside this scope -- so a scope's first stop is the same node Tab would
/// reach first, which is what makes entering a scope and tabbing into it agree.
pub fn first_focusable_in(scope: u64) -> Option<u64> {
    let order = MANAGER.with(|manager| traversal_order(&manager.borrow()));
    MANAGER.with(|manager| {
        let manager = manager.borrow();
        order.into_iter().find(|id| {
            manager
                .entries
                .iter()
                .find(|entry| entry.id == *id)
                .is_some_and(|entry| entry.ancestors.contains(&scope) && entry.traversable)
        })
    })
}

/// Forgets every scope and what it remembered. For tests, and for the same
/// reason [`reset`] exists.
pub fn reset_scopes() {
    SCOPES.with(|scopes| scopes.borrow_mut().clear());
    SCOPE_MEMORY.with(|memory| memory.borrow_mut().clear());
}

// -- Whether to draw the focus ring --------------------------------------------

/// Upstream `FocusHighlightMode`: whether the focused control should look
/// focused.
///
/// A focus ring is right on a keyboard and wrong on a touchscreen. On a phone
/// the reader taps the thing they want and there is no "where am I" to answer,
/// so a persistent ring on the last-tapped button is visual noise -- and worse,
/// it looks like something is selected when nothing is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusHighlightMode {
    /// No ring. Upstream's exception is worth keeping in mind: controls that
    /// bring up the soft keyboard still show one, because there the ring is
    /// answering "where will I be typing".
    Touch,
    /// A ring. Keyboards and mice.
    Traditional,
}

/// Upstream `FocusHighlightStrategy`: whether the mode follows the input device
/// or is pinned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusHighlightStrategy {
    /// Follow the last interaction. The default.
    #[default]
    Automatic,
    AlwaysTouch,
    AlwaysTraditional,
}

thread_local! {
    static HIGHLIGHT_STRATEGY: Cell<FocusHighlightStrategy> =
        const { Cell::new(FocusHighlightStrategy::Automatic) };
    /// Upstream's `_lastInteractionRequiresTraditionalHighlights`, `None` until
    /// something has happened. See [`highlight_mode`] for the name.
    static LAST_INTERACTION_WAS_TOUCH: Cell<Option<bool>> = const { Cell::new(None) };
    static HIGHLIGHT_LISTENERS: RefCell<Vec<Option<Rc<dyn Fn(FocusHighlightMode)>>>> =
        const { RefCell::new(Vec::new()) };
}

thread_local! {
    /// The opening guess, before anything has been touched or typed.
    static DEFAULT_HIGHLIGHT: Cell<FocusHighlightMode> =
        const { Cell::new(FocusHighlightMode::Traditional) };
}

/// Sets the opening guess, before anything has been touched or typed.
///
/// Upstream works this out itself, from `defaultTargetPlatform` **and** whether
/// a mouse is connected: a mobile platform with no mouse starts at touch, and
/// everything else at traditional. This crate has
/// [`TargetPlatform::is_mobile`](crate::editable_text::TargetPlatform::is_mobile)
/// -- the same grouping -- but no query for the *current* platform and none at
/// all for mouse presence, so the guess is the host's to state rather than
/// something to infer from half the inputs.
///
/// It only affects the first moments: upstream says so too, and the value is
/// replaced as soon as any key or touch arrives.
pub fn set_default_highlight_mode(mode: FocusHighlightMode) {
    let before = highlight_mode();
    DEFAULT_HIGHLIGHT.with(|d| d.set(mode));
    announce_highlight(before);
}

/// What [`set_default_highlight_mode`] was told, or `Traditional`.
fn default_mode_for_platform() -> FocusHighlightMode {
    DEFAULT_HIGHLIGHT.with(|d| d.get())
}

/// Upstream `FocusManager.highlightMode`.
///
/// # Upstream's flag is named the opposite of what it does
///
/// The state behind this is `_lastInteractionRequiresTraditionalHighlights`,
/// and in `updateMode` a **true** produces `FocusHighlightMode.touch`. A field
/// whose name says "traditional" answers "touch". Kept as a fact about
/// upstream rather than tidied away, because anybody reading the two files side
/// by side will hit it; the flag here is named for what it holds.
pub fn highlight_mode() -> FocusHighlightMode {
    match HIGHLIGHT_STRATEGY.with(|s| s.get()) {
        FocusHighlightStrategy::AlwaysTouch => FocusHighlightMode::Touch,
        FocusHighlightStrategy::AlwaysTraditional => FocusHighlightMode::Traditional,
        FocusHighlightStrategy::Automatic => match LAST_INTERACTION_WAS_TOUCH.with(|s| s.get()) {
            None => default_mode_for_platform(),
            Some(true) => FocusHighlightMode::Touch,
            Some(false) => FocusHighlightMode::Traditional,
        },
    }
}

/// Upstream `FocusManager.highlightStrategy`.
pub fn set_highlight_strategy(strategy: FocusHighlightStrategy) {
    let before = highlight_mode();
    HIGHLIGHT_STRATEGY.with(|s| s.set(strategy));
    announce_highlight(before);
}

/// A touch or stylus went down. Upstream's `handlePointerEvent`, which reacts
/// to `touch`, `stylus` and `invertedStylus`.
///
/// **A mouse or trackpad changes nothing**, which is upstream's deliberate
/// omission -- those kinds fall through its switch with no statement. A mouse
/// moving across a tablet does not mean the reader stopped touching it, and the
/// thing that does say "keyboard and mouse" is a key.
pub fn note_touch_interaction() {
    let before = highlight_mode();
    LAST_INTERACTION_WAS_TOUCH.with(|s| s.set(Some(true)));
    announce_highlight(before);
}

/// A key was pressed. Upstream sets the flag the other way here.
pub fn note_key_interaction() {
    let before = highlight_mode();
    LAST_INTERACTION_WAS_TOUCH.with(|s| s.set(Some(false)));
    announce_highlight(before);
}

/// An assistive technology performed an action. Upstream treats this as touch:
/// a reader driving the interface through a screen reader is not looking for a
/// focus ring.
pub fn note_semantics_interaction() {
    note_touch_interaction();
}

/// Upstream's `addListener` on the highlight manager. Answers a token, for the
/// reason [`SemanticsBinding::add_enabled_listener`] does.
///
/// [`SemanticsBinding::add_enabled_listener`]: crate::semantics::SemanticsBinding::add_enabled_listener
pub fn add_highlight_listener(listener: impl Fn(FocusHighlightMode) + 'static) -> usize {
    HIGHLIGHT_LISTENERS.with(|listeners| {
        let mut listeners = listeners.borrow_mut();
        listeners.push(Some(Rc::new(listener)));
        listeners.len() - 1
    })
}

pub fn remove_highlight_listener(token: usize) -> bool {
    HIGHLIGHT_LISTENERS.with(|listeners| {
        let mut listeners = listeners.borrow_mut();
        match listeners.get_mut(token) {
            Some(slot) => slot.take().is_some(),
            None => false,
        }
    })
}

/// Upstream's `if (highlightMode != oldMode) notifyListeners()`: the edge, not
/// every interaction. A listener that heard every keystroke would repaint every
/// focus ring on every keystroke.
fn announce_highlight(before: FocusHighlightMode) {
    let now = highlight_mode();
    if now == before {
        return;
    }
    let listeners: Vec<Rc<dyn Fn(FocusHighlightMode)>> =
        HIGHLIGHT_LISTENERS.with(|l| l.borrow().iter().flatten().cloned().collect());
    for listener in listeners {
        listener(now);
    }
}

/// Forgets the highlight state. For tests, as [`reset`] is.
pub fn reset_highlight() {
    HIGHLIGHT_STRATEGY.with(|s| s.set(FocusHighlightStrategy::Automatic));
    LAST_INTERACTION_WAS_TOUCH.with(|s| s.set(None));
    DEFAULT_HIGHLIGHT.with(|d| d.set(FocusHighlightMode::Traditional));
    HIGHLIGHT_LISTENERS.with(|l| l.borrow_mut().clear());
}

// -- Watching an action, and the detector built on it --------------------------

/// Upstream `ActionListener`: told whenever an action is invoked, without being
/// the one that invoked it.
///
/// Upstream is a widget whose `State` adds a listener in `initState`, swaps it
/// in `didUpdateWidget` and removes it in `dispose`. Those three hooks are the
/// two `State` members this crate's ledger records as absent, so this is the
/// registry that would sit behind them: a caller adds and removes, and holds
/// the token.
///
/// What it is *for* is a control that draws itself differently while its action
/// runs -- a button that stays pressed for as long as the thing it started is
/// still going -- without that control owning the action.
#[derive(Default)]
pub struct ActionListener {
    listeners: Vec<Option<Rc<dyn Fn(&crate::actions::Intent)>>>,
}

impl ActionListener {
    pub fn new() -> ActionListener {
        ActionListener::default()
    }

    /// Upstream's `Action.addActionListener`.
    pub fn add(&mut self, listener: impl Fn(&crate::actions::Intent) + 'static) -> usize {
        self.listeners.push(Some(Rc::new(listener)));
        self.listeners.len() - 1
    }

    /// Upstream's `Action.removeActionListener`. A token, and a hole rather
    /// than a shift, so the tokens after it keep meaning what they meant.
    pub fn remove(&mut self, token: usize) -> bool {
        match self.listeners.get_mut(token) {
            Some(slot) => slot.take().is_some(),
            None => false,
        }
    }

    /// Upstream's `Action.notifyActionListeners`, called around an invocation.
    ///
    /// The list is copied before it is walked, which is upstream's habit
    /// throughout and is load-bearing here: a listener is entitled to remove
    /// itself, and a walk over the live list would be iterating something being
    /// changed underneath it.
    pub fn notify(&self, intent: &crate::actions::Intent) {
        let listeners: Vec<Rc<dyn Fn(&crate::actions::Intent)>> =
            self.listeners.iter().flatten().cloned().collect();
        for listener in listeners {
            listener(intent);
        }
    }

    pub fn len(&self) -> usize {
        self.listeners.iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Upstream `FocusableActionDetector`: focus, hover and shortcuts in one place,
/// which is what every Material control is built on.
///
/// # Its whole content is when *not* to call back
///
/// The two callbacks -- `onShowFocusHighlight` and `onShowHoverHighlight` --
/// are not "you were focused" and "you were hovered". They are "the answer to
/// *should a highlight be drawn* changed", and upstream computes that answer
/// before and after every state change and fires only on the difference. A
/// control that acted on the raw events would light up while disabled, and
/// light up on a touchscreen where a highlight means nothing.
///
/// The two answers, verbatim from upstream's `_mayTriggerCallback`:
///
/// * **hover**: hovering **and** enabled **and** highlights are allowed at all;
/// * **focus**: focused **and** highlights are allowed **and** focus can be
///   requested -- which under directional navigation is always true, because a
///   d-pad user needs to see where they are even on a disabled control they are
///   passing over.
///
/// "Highlights are allowed at all" is [`highlight_mode`], and it is why this
/// class needed that machinery first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetectorState {
    pub hovering: bool,
    pub focused: bool,
    pub enabled: bool,
}

/// Upstream `NavigationMode`, which decides the focus half.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NavigationMode {
    /// Keyboard and mouse. A disabled control cannot take focus.
    #[default]
    Traditional,
    /// A d-pad or remote. **Everything can take focus**, including disabled
    /// controls -- upstream's reason is that a reader moving through a screen
    /// with a directional pad has no other way to know a disabled control is
    /// there, and skipping it silently loses them.
    Directional,
}

/// Upstream `FocusableActionDetector`, as the decision it makes.
///
/// The widget assembly around it -- the `Focus`, the `MouseRegion`, the
/// `Shortcuts` and `Actions` -- is composition this crate's caller does with
/// [`Focus`] and the pointer handlers directly. What has no counterpart
/// elsewhere is the gating, so that is what this is.
pub struct FocusableActionDetector {
    state: DetectorState,
    /// Upstream's `_canShowHighlight`, and it is **cached rather than read
    /// live** for a reason that only shows up when the mode itself changes:
    /// `update` compares the answer before and after a change, and a live read
    /// would see the new mode on both sides and conclude nothing moved.
    /// Upstream's `_updateHighlightMode` passes the cache update *as* the task
    /// for exactly this.
    can_show_highlight: bool,
    navigation_mode: NavigationMode,
    on_show_focus_highlight: Option<Rc<dyn Fn(bool)>>,
    on_show_hover_highlight: Option<Rc<dyn Fn(bool)>>,
}

impl FocusableActionDetector {
    pub fn new(enabled: bool) -> FocusableActionDetector {
        FocusableActionDetector {
            state: DetectorState {
                enabled,
                ..DetectorState::default()
            },
            can_show_highlight: highlight_mode() == FocusHighlightMode::Traditional,
            navigation_mode: NavigationMode::Traditional,
            on_show_focus_highlight: None,
            on_show_hover_highlight: None,
        }
    }

    pub fn with_navigation_mode(mut self, mode: NavigationMode) -> Self {
        self.navigation_mode = mode;
        self
    }

    pub fn with_on_show_focus_highlight(mut self, on: impl Fn(bool) + 'static) -> Self {
        self.on_show_focus_highlight = Some(Rc::new(on));
        self
    }

    pub fn with_on_show_hover_highlight(mut self, on: impl Fn(bool) + 'static) -> Self {
        self.on_show_hover_highlight = Some(Rc::new(on));
        self
    }

    pub fn state(&self) -> DetectorState {
        self.state
    }

    /// Upstream's `shouldShowHoverHighlight`.
    pub fn should_show_hover_highlight(&self) -> bool {
        self.state.hovering && self.state.enabled && self.highlights_allowed()
    }

    /// Upstream's `shouldShowFocusHighlight`.
    pub fn should_show_focus_highlight(&self) -> bool {
        self.state.focused && self.highlights_allowed() && self.can_request_focus()
    }

    /// Upstream's `canRequestFocus`.
    fn can_request_focus(&self) -> bool {
        match self.navigation_mode {
            NavigationMode::Traditional => self.state.enabled,
            NavigationMode::Directional => true,
        }
    }

    fn highlights_allowed(&self) -> bool {
        self.can_show_highlight
    }

    /// Upstream's `_mayTriggerCallback`: make the change, and fire only the
    /// callbacks whose answer moved.
    pub fn update(&mut self, change: impl FnOnce(&mut DetectorState)) {
        let hover_before = self.should_show_hover_highlight();
        let focus_before = self.should_show_focus_highlight();
        change(&mut self.state);
        let hover_now = self.should_show_hover_highlight();
        let focus_now = self.should_show_focus_highlight();
        // Upstream fires focus first, then hover.
        if focus_before != focus_now {
            if let Some(on) = &self.on_show_focus_highlight {
                on(focus_now);
            }
        }
        if hover_before != hover_now {
            if let Some(on) = &self.on_show_hover_highlight {
                on(hover_now);
            }
        }
    }

    /// The pointer arrived. Upstream's `_handleMouseEnter`, including its
    /// guard.
    ///
    /// The guard saves the walk, **not the outcome**: [`Detector::update`]
    /// already fires only the callbacks whose derived answer moved, and
    /// setting `hovering` to the value it already has moves nothing. So
    /// deleting the guard here changes what runs and not what happens, which
    /// is why a screen for guards the suite cannot make matter finds this one
    /// and the two below it. They stay because they are upstream's shape and
    /// they are free; the earlier version of this comment said the enter "does
    /// not go through the callback machinery at all", which is true of the
    /// control flow and reads like a claim about the result.
    pub fn hover(&mut self, hovering: bool) {
        if self.state.hovering == hovering {
            return;
        }
        self.update(|state| state.hovering = hovering);
    }

    /// Upstream's `_handleFocusChange`, with the same guard.
    pub fn focus(&mut self, focused: bool) {
        if self.state.focused == focused {
            return;
        }
        self.update(|state| state.focused = focused);
    }

    /// Upstream's `didUpdateWidget` path, which runs the same comparison
    /// against the *old* widget when `enabled` changes.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.state.enabled == enabled {
            return;
        }
        self.update(|state| state.enabled = enabled);
    }

    /// The highlight mode changed under it. Upstream's `_updateHighlightMode`,
    /// which is registered as a listener on the focus manager and whose task is
    /// the cache update -- see [`FocusableActionDetector::can_show_highlight`].
    pub fn highlight_mode_changed(&mut self) {
        let now = highlight_mode() == FocusHighlightMode::Traditional;
        let hover_before = self.should_show_hover_highlight();
        let focus_before = self.should_show_focus_highlight();
        self.can_show_highlight = now;
        let hover_now = self.should_show_hover_highlight();
        let focus_now = self.should_show_focus_highlight();
        if focus_before != focus_now {
            if let Some(on) = &self.on_show_focus_highlight {
                on(focus_now);
            }
        }
        if hover_before != hover_now {
            if let Some(on) = &self.on_show_hover_highlight {
                on(hover_now);
            }
        }
    }
}

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
        // Nor is one inside a subtree that has said its descendants may not be
        // focused. Upstream's `requestFocus` consults `canRequestFocus`, which
        // is the same walk.
        if !can_take_focus(&manager, id) {
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
    if changed {
        remember_focus(id);
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

/// Upstream `TraversalEdgeBehavior`: what a focus scope does when traversal
/// runs off its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TraversalEdgeBehavior {
    /// Wrap to the other end, so the scope is a ring. Upstream's default and
    /// what this crate's [`next`] and [`previous`] do today.
    #[default]
    ClosedLoop,
    /// Drop focus and let the embedder have the keystroke -- on the web, the
    /// browser gets the tab and the user can reach the address bar.
    LeaveFlutterView,
    /// Hand the move to the enclosing scope.
    ParentScope,
    /// Refuse the move and leave focus where it is.
    Stop,
}

/// What actually happens at the edge, once the scope's behaviour has been
/// resolved against whether there is a parent scope to hand off to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeOutcome {
    /// Focus the node at the far end.
    Wrap,
    /// Unfocus, and report the move as not handled.
    Unfocus,
    /// Unfocus and ask the enclosing scope to move instead.
    DelegateToParent,
    /// Report the move as not handled, and leave the focus alone.
    Stay,
}

impl TraversalEdgeBehavior {
    pub const ALL: [TraversalEdgeBehavior; 4] = [
        TraversalEdgeBehavior::ClosedLoop,
        TraversalEdgeBehavior::LeaveFlutterView,
        TraversalEdgeBehavior::ParentScope,
        TraversalEdgeBehavior::Stop,
    ];

    /// Resolve the behaviour at an edge.
    ///
    /// `has_parent_scope` is upstream's `parentScope != null && parentScope !=
    /// FocusManager.instance.rootScope`. **The root scope does not count as a
    /// parent**, so a top-level `ParentScope` has nowhere to go.
    ///
    /// And what it does then is the part worth pinning: upstream's comment
    /// says "No valid parent scope. Fallback to closed loop behavior." It
    /// **wraps** -- it does not stop. A scope that asked to defer outward and
    /// finds nothing outward behaves like a ring, not like a wall.
    pub fn at_edge(self, has_parent_scope: bool) -> EdgeOutcome {
        match self {
            TraversalEdgeBehavior::ClosedLoop => EdgeOutcome::Wrap,
            TraversalEdgeBehavior::LeaveFlutterView => EdgeOutcome::Unfocus,
            TraversalEdgeBehavior::ParentScope if has_parent_scope => EdgeOutcome::DelegateToParent,
            TraversalEdgeBehavior::ParentScope => EdgeOutcome::Wrap,
            TraversalEdgeBehavior::Stop => EdgeOutcome::Stay,
        }
    }
}

impl EdgeOutcome {
    /// Whether the focused node is cleared.
    ///
    /// This is the **whole difference** between `LeaveFlutterView` and `Stop`:
    /// both report the move as not handled, and only one of them lets go of
    /// the focus first. Collapsing them would leave a focus ring sitting on a
    /// widget while the browser took the next tab.
    pub fn unfocuses(self) -> bool {
        matches!(self, EdgeOutcome::Unfocus | EdgeOutcome::DelegateToParent)
    }

    /// Whether the traversal reports that it moved focus itself.
    ///
    /// `Unfocus` and `Stay` both answer no -- the first because it deliberately
    /// gave the keystroke away, the second because it refused it.
    /// `DelegateToParent` answers whatever the parent's move answered, which
    /// upstream checks rather than assumes: `return
    /// focusedChild.enclosingScope?.focusedChild != focusedChild`.
    pub fn definitely_handled(self) -> bool {
        matches!(self, EdgeOutcome::Wrap)
    }
}

/// The scroll alignment a tab traversal asks for.
///
/// Keyed to the **direction of travel, not to the destination**: upstream
/// writes `forward ? keepVisibleAtEnd : keepVisibleAtStart` in the ordinary
/// step and passes the very same thing through the wrap. So wrapping forward
/// -- to the *first* node -- still asks for `keepVisibleAtEnd`, which reads
/// backwards until you notice the policy is about which way the user is
/// moving through the list rather than about where they landed.
pub fn tab_alignment(forward: bool) -> crate::directional_traversal::ScrollPositionAlignmentPolicy {
    use crate::directional_traversal::ScrollPositionAlignmentPolicy;
    if forward {
        ScrollPositionAlignmentPolicy::KeepVisibleAtEnd
    } else {
        ScrollPositionAlignmentPolicy::KeepVisibleAtStart
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

/// The innermost group on an ancestor chain, if the chain runs through one.
fn enclosing_group(ancestors: &[u64]) -> Option<u64> {
    MANAGER.with(|manager| {
        let manager = manager.borrow();
        ancestors.iter().rev().copied().find(|id| {
            manager
                .entries
                .iter()
                .any(|entry| entry.id == *id && entry.is_group)
        })
    })
}

/// Upstream `ExcludeFocus`: a subtree that cannot be reached by the keyboard.
///
/// Upstream builds it as a `Focus` with four flags off at once, and each one
/// closes a different way in:
///
/// * `canRequestFocus: false` -- the node itself is not a stop.
/// * `skipTraversal: true` -- nor is it visited on the way past.
/// * `descendantsAreFocusable: !excluding` -- and neither is anything under it.
/// * `includeSemantics: false` -- and it adds no semantics node of its own,
///   which matters wherever something above is already merging them.
///
/// Upstream is careful to say what it does **not** do: it "does not affect the
/// value of `FocusNode.canRequestFocus` on the descendants". The descendants
/// keep their own answer and simply cannot be reached while this is excluding,
/// so turning it off again leaves them able to take focus -- though, upstream
/// notes, anything that was focused when it came on is unfocused and **not
/// refocused** when it goes off. Exclusion is not a pause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExcludeFocus {
    /// Upstream's `excluding`, true by default.
    pub excluding: bool,
}

impl Default for ExcludeFocus {
    fn default() -> ExcludeFocus {
        ExcludeFocus::new()
    }
}

impl ExcludeFocus {
    pub fn new() -> ExcludeFocus {
        ExcludeFocus { excluding: true }
    }

    pub fn excluding(excluding: bool) -> ExcludeFocus {
        ExcludeFocus { excluding }
    }

    /// Upstream's `canRequestFocus: false`. Constant.
    pub fn can_request_focus(&self) -> bool {
        false
    }

    /// Upstream's `skipTraversal: true`. Constant.
    pub fn skips_traversal(&self) -> bool {
        true
    }

    /// Upstream's `includeSemantics: false`. Constant.
    pub fn includes_semantics(&self) -> bool {
        false
    }

    /// Upstream's `descendantsAreFocusable: !excluding` -- **the only one of
    /// the four the flag decides.** An `ExcludeFocus` that is not excluding is
    /// still not itself a focus stop; it has simply stopped blocking what is
    /// under it.
    pub fn descendants_are_focusable(&self) -> bool {
        !self.excluding
    }
}

// The traversal order: every stop, in the order Tab visits them.
//
// Upstream's `FocusTraversalPolicy._sortAllDescendants`, in the shape this
// registry has. Each group's members are sorted among themselves by
// [`OrderedTraversalPolicy`], and a group node stands in its parent's list
// for its whole subtree -- which is what keeps a group's stops together and
// keeps an order inside one group from jumping a node past another group's.
thread_local! {
    /// The focus traps in force, innermost last.
    ///
    /// Upstream a `ModalRoute` installs a `FocusScopeNode` and traversal is
    /// confined to it; there is no scope node here, so the confinement is said
    /// directly. A trap is a node id, and while one is in force only that node
    /// and its descendants are reachable.
    ///
    /// A stack rather than a flag because modals nest: a dialog over a dialog
    /// confines to the inner one, and closing it returns to the outer.
    static TRAPS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Confines focus to `root` and its descendants until [`release_trap`].
///
/// Upstream's `ModalRoute` focus scope. What it prevents is a Tab out of a
/// dialog and into the page behind it -- which a reader cannot see is there,
/// and which is not supposed to be reachable while the dialog is up.
pub fn trap_focus(root: u64) {
    TRAPS.with(|traps| traps.borrow_mut().push(root));
}

/// Lifts the trap `root` installed. Lifting one that is not the innermost
/// removes it anyway: a modal may be dismissed out of order, and the
/// alternative is a trap that outlives what it was protecting.
pub fn release_trap(root: u64) {
    TRAPS.with(|traps| {
        let mut traps = traps.borrow_mut();
        if let Some(at) = traps.iter().rposition(|id| *id == root) {
            traps.remove(at);
        }
    });
}

/// The trap in force, if any.
pub fn active_trap() -> Option<u64> {
    TRAPS.with(|traps| traps.borrow().last().copied())
}

thread_local! {
    /// Traps that have asked for the focus and have not had it yet.
    ///
    /// Upstream's `FocusManager._pendingAutofocuses`, and it is a *pending*
    /// list for the reason upstream's is: a `FocusScope(autofocus: true)` says
    /// what it wants while it is being built, and at that moment none of the
    /// nodes it would focus have registered yet. Focusing there would find an
    /// empty tree and do nothing at all.
    static PENDING_AUTOFOCUS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    /// Nodes that asked for the focus themselves, in the order they asked.
    static PENDING_NODE_AUTOFOCUS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Asks for the focus to be put inside `trap` once its nodes exist.
///
/// Upstream's `FocusScope(autofocus: true)`, which every route carries
/// (`_ModalScope`). Without it a dialog comes up with the keyboard still
/// wherever it was -- and since [`dispatch_key`] starts from the focused node,
/// **none of the dialog's key handlers run** until the reader presses Tab.
pub fn autofocus_in(trap: u64) {
    PENDING_AUTOFOCUS.with(|pending| pending.borrow_mut().push(trap));
}

/// Asks for this node to take the focus once it exists.
///
/// Upstream's `Focus(autofocus: true)`, which `TextField.autofocus` and
/// `SelectableText.autofocus` pass down. Distinct from [`autofocus_in`],
/// which is a *scope* asking for the focus to land somewhere inside it: this
/// one names the node.
///
/// **Asked once, not every frame.** Upstream registers it in `initState`, and
/// the caller here is expected to do the same -- a field that asked again on
/// every build would take the focus back from wherever the reader had moved
/// it, which is worse than never having asked.
pub fn autofocus_node(id: u64) {
    PENDING_NODE_AUTOFOCUS.with(|pending| pending.borrow_mut().push(id));
}

/// Grants the pending autofocus requests. Called once per frame, **after** the
/// build that registers the nodes.
///
/// Upstream's `applyFocusChangesIfNeeded` does the same at the same point in
/// the frame, and for the same reason.
///
/// Returns whether the focus moved.
pub fn apply_pending_autofocus() -> bool {
    let pending: Vec<u64> =
        PENDING_AUTOFOCUS.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
    let nodes: Vec<u64> =
        PENDING_NODE_AUTOFOCUS.with(|pending| std::mem::take(&mut *pending.borrow_mut()));

    // Nodes first, and this order is the rule rather than an accident: a
    // scope's request is "put the focus *somewhere* in here", a node's is
    // "put it on me". Granting the scope first would satisfy it with whatever
    // stop came first and leave the node that actually asked unfocused --
    // upstream's `FocusScopeNode.autofocus` behaves the same way, holding a
    // node's request and preferring it over its own first stop.
    for node in nodes {
        // Taken in the order asked, and only the first one that can have it:
        // two fields both asking is a mistake in the tree, and upstream
        // asserts on it. Here the first wins, which is at least stable.
        if focused().is_some() {
            break;
        }
        // A node belonging to a scope that is no longer the one in force --
        // a dialog dismissed between the build that asked and this pass --
        // must not pull the keyboard into a page the reader cannot see.
        //
        // There is deliberately no "does this node exist" check beside it:
        // `focus` already refuses an unregistered id, and a second copy here
        // would be the same rule in two places with no way to tell them
        // apart. A mutation removing it survived every test, which is what
        // that looks like from the outside.
        if !within_active_trap(node) {
            continue;
        }
        if focus(node) {
            return true;
        }
    }

    if pending.is_empty() {
        return false;
    }
    // Only the trap actually in force may claim the focus. An outer modal's
    // request is superseded by an inner one, and a modal dismissed before the
    // frame arrived must not pull the focus into a dialog that has gone --
    // without this, its request would be granted against the *page's* stops,
    // because that is what `traversal_order` answers with no trap.
    let Some(active) = active_trap() else {
        return false;
    };
    // Belt and braces, and said so rather than left to look like coverage:
    // a mutation dropping this line survives every test here, because the only
    // way the trap in force is one that never asked is with an *older* modal
    // still up -- and then the focus is already inside it, so `already_inside`
    // below refuses anyway. Kept because the two say different things.
    if !pending.contains(&active) {
        return false;
    }
    let (already_inside, first) = MANAGER.with(|manager| {
        let manager = manager.borrow();
        let stops = traversal_order(&manager);
        let inside = manager.focused.is_some_and(|id| stops.contains(&id));
        (inside, stops.first().copied())
    });
    // Upstream's autofocus "does not fight a scope that already chose": a
    // dialog whose field asked for the focus itself keeps it.
    if already_inside {
        return false;
    }
    match first {
        Some(id) => focus(id),
        None => false,
    }
}

/// Forgets the pending requests. For tests, and for a view being torn down.
pub fn reset_pending_autofocus() {
    PENDING_AUTOFOCUS.with(|pending| pending.borrow_mut().clear());
    PENDING_NODE_AUTOFOCUS.with(|pending| pending.borrow_mut().clear());
}

/// Whether the node is reachable under whatever trap is in force. `true` when
/// there is no trap, which is the ordinary page.
fn within_active_trap(id: u64) -> bool {
    MANAGER.with(|manager| within_trap(&manager.borrow(), id))
}

/// Whether a node is reachable under the trap in force.
fn within_trap(manager: &FocusManager, id: u64) -> bool {
    let Some(trap) = active_trap() else {
        return true;
    };
    if id == trap {
        return true;
    }
    manager
        .entries
        .iter()
        .find(|entry| entry.id == id)
        .is_some_and(|entry| entry.ancestors.contains(&trap))
}

fn traversal_order(manager: &FocusManager) -> Vec<u64> {
    fn expand(manager: &FocusManager, group: Option<u64>, out: &mut Vec<u64>) {
        let mut members: Vec<(usize, &FocusEntry)> = manager
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.group == group && (!skips_traversal(manager, entry) || entry.is_group)
            })
            .collect();
        // Ordered first, in their order; the rest keep the order they
        // registered in, which `sort_by` preserves because it is stable.
        members.sort_by(|a, b| OrderedTraversalPolicy::compare(&a.1.order, &b.1.order));
        for (_, entry) in members {
            if entry.is_group {
                expand(manager, Some(entry.id), out);
            }
            if !skips_traversal(manager, entry) {
                out.push(entry.id);
            }
        }
    }

    let mut order = Vec::new();
    expand(manager, None, &mut order);
    // A trap removes the stops outside it rather than reordering them: Tab has
    // to *stay* in the dialog, so the page's fields are not later in the cycle,
    // they are not in the cycle.
    order.retain(|id| within_trap(manager, *id));
    order
}

fn step(direction: isize) -> bool {
    let target = MANAGER.with(|manager| {
        let manager = manager.borrow();
        let stops = traversal_order(&manager);
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
    // Upstream registers a global key handler for exactly this; here the one
    // place every key already passes through is this function.
    note_key_interaction();
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

/// Draws the focus highlight over its child while the control has the
/// keyboard.
///
/// Upstream this is one of `InkResponse`'s ink features, faded in and out with
/// the hover and press highlights it shares a stack with. Here it is a widget
/// of its own, because the controls that need it -- a chip, a button, the
/// three toggleables -- are not built on `InkResponse` in this crate and
/// wrapping each of them in one for the highlight alone would be a splash and
/// a hover nobody asked for.
struct FocusHighlight {
    shape: FocusShape,
    /// Where this publishes its handle, so the focus node above can tell it
    /// the keyboard arrived.
    sink: Rc<RefCell<Option<crate::framework::StateHandle<FocusHighlightState>>>>,
    child: RefCell<Option<AnyWidget>>,
}

#[derive(Default)]
struct FocusHighlightState {
    focused: bool,
}

impl crate::framework::StatefulComponent for FocusHighlight {
    type State = FocusHighlightState;

    fn initial_state(&self) -> FocusHighlightState {
        FocusHighlightState::default()
    }

    fn build(
        &self,
        state: &FocusHighlightState,
        handle: crate::framework::StateHandle<FocusHighlightState>,
        context: &mut crate::framework::BuildContext,
    ) -> AnyWidget {
        *self.sink.borrow_mut() = Some(handle);
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        if !state.focused {
            return child;
        }
        // Upstream's `focusColor`, which defaults to the theme's own overlay
        // rather than to a colour of its own: `ThemeData.focusColor`, the
        // primary at 12%. A ring would be Cupertino's answer; Material fills.
        let theme = crate::components::theme_of(context);
        let colour = theme.primary.with_alpha(0x1f);
        let shape = self.shape;
        crate::framework::single(child, move |inner| {
            Box::new(
                crate::render::RenderStack::new()
                    .push(inner)
                    .push(crate::render::RenderRef::new(match shape {
                        FocusShape::Box { corner_radius } => crate::widgets::Container::new()
                            .with_color(colour)
                            .with_corner_radius(corner_radius),
                        FocusShape::Circle { radius } => crate::widgets::Container::new()
                            .with_color(colour)
                            .with_size(radius * 2.0, radius * 2.0)
                            .with_corner_radius(radius),
                    })),
            )
        })
    }
}

/// Where a control's focus highlight is drawn.
///
/// Upstream keeps two things apart and so does this: `highlightShape` says
/// box or circle, and the rounding of a box is `borderRadius`, a separate
/// parameter. There is no third "stadium" shape -- a stadium is a box whose
/// corner radius is half its height, which is what a chip and a button pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FocusShape {
    /// Upstream's `BoxShape.rectangle` with a `borderRadius`. The highlight
    /// fills the control.
    Box { corner_radius: f32 },
    /// Upstream's `BoxShape.circle`: a disc of `radius`, centred on the
    /// control. The toggleables use it because the box they draw is far
    /// smaller than the area a finger is allowed to hit, and a highlight the
    /// size of the tick would look like a second, smaller control.
    Circle { radius: f32 },
}

impl FocusShape {
    /// `kRadialReactionRadius`, upstream's constant for the disc a checkbox,
    /// radio or switch reacts inside.
    pub const RADIAL_REACTION_RADIUS: f32 = 20.0;

    /// The shape the toggleables share.
    pub fn radial() -> FocusShape {
        FocusShape::Circle {
            radius: FocusShape::RADIAL_REACTION_RADIUS,
        }
    }
}

/// Makes `child` a keyboard stop that Enter and Space press.
///
/// The three things every operable control needs and none of them needs
/// differently: a node so the traversal can reach it, `autofocus` so it can
/// ask for the keyboard on arrival, and an activation bound to the same
/// handler the pointer calls.
///
/// It takes the pointer's own callback rather than a second one, because the
/// two must not be able to disagree about what pressing this does -- the same
/// rule the semantics annotation already follows by passing `on_tap` through.
///
/// A control with no handler is returned untouched. Upstream's
/// `canRequestFocus` is `isEnabled`, and a stop that answers nothing is
/// somewhere the reader lands for no reason and has to leave again.
pub fn operable(
    id: u64,
    autofocus: bool,
    on_tap: Option<Rc<dyn Fn(crate::gestures::TapEvent)>>,
    shape: FocusShape,
    child: AnyWidget,
) -> AnyWidget {
    let Some(on_tap) = on_tap else {
        return child;
    };
    // The highlight sits inside the focus node rather than outside it, so
    // that what gains the focus and what shows it are the same subtree.
    //
    // It is told when the focus moves rather than asking at every build:
    // `has_focus` would answer correctly whenever a build happened to run,
    // and nothing makes one run when the keyboard moves from one control to
    // another. A control that only repainted when something else rebuilt it
    // would show the highlight late, or keep showing it.
    let sink: Rc<RefCell<Option<crate::framework::StateHandle<FocusHighlightState>>>> =
        Rc::default();
    let child = crate::framework::stateful(FocusHighlight {
        shape,
        sink: Rc::clone(&sink),
        child: RefCell::new(Some(child)),
    });
    component(
        Focus::new(id, child)
            .with_on_focus_change(move |focused| {
                if let Some(handle) = sink.borrow().clone() {
                    handle.set_state(move |state| state.focused = focused);
                }
            })
            .with_autofocus(autofocus)
            .with_on_activate(move || {
                // A key has no position. Upstream's `ActivateAction` calls
                // `onPressed` rather than `onTap` for the same reason; here
                // the origin and a pointer id no real pointer has say it.
                on_tap(crate::gestures::TapEvent {
                    local_position: crate::render::Offset::ZERO,
                    position: crate::render::Offset::ZERO,
                    pointer_id: -1,
                });
            }),
    )
}

/// Whether this key is the one that presses the focused control.
///
/// Asked of the same table the traversal asks, so Enter and Space mean here
/// what they mean everywhere else, and a platform that ever spelled them
/// differently would be answered from one place. Upstream's
/// `WidgetsApp.defaultShortcuts` binds both to `ActivateIntent`, and its
/// `ButtonActivateIntent` is the same key for a narrower audience.
fn is_activation(event: &KeyEvent) -> bool {
    let table =
        crate::shortcuts::default_shortcuts(crate::editable_text::TargetPlatform::host(), false);
    let intent =
        crate::keyboard::with_keyboard(|keyboard| table.intent_for(event, keyboard).cloned());
    matches!(
        intent,
        Some(crate::actions::Intent::Activate) | Some(crate::actions::Intent::ButtonActivate)
    )
}

/// Handles Tab, if nothing else did.
///
/// Upstream this is a `Shortcuts` widget installed by `WidgetsApp` mapping Tab
/// to `NextFocusIntent` and Shift+Tab to `PreviousFocusIntent`. It is a
/// default rather than a rule -- an application that wants Tab for something
/// else handles it first, and this never sees it.
pub fn handle_traversal_key(event: &KeyEvent, keyboard: &Keyboard) -> bool {
    // No `is_down` test here: every `ShortcutActivator` begins with one, so a
    // release never matches a row and a guard here would be the same question
    // asked twice. The mutation sweep is what settled it -- removing this
    // guard changed nothing, which is the definition of a line not worth
    // keeping.
    //
    // Which key traverses is `WidgetsApp.defaultShortcuts`' business, not
    // this function's: it used to test for Tab itself and read `shift` off the
    // keyboard, which is the same rule written a second time -- and a second
    // copy that knew nothing of the web table, where Tab is the same but the
    // arrows are not.
    //
    // Shift still comes from the pressed set rather than from the event, and
    // the activator is what reads it: an event says what changed, and whether
    // another key is held is a question about something else.
    let table =
        crate::shortcuts::default_shortcuts(crate::editable_text::TargetPlatform::host(), false);
    match table.intent_for(event, keyboard) {
        Some(crate::actions::Intent::NextFocus) => next(),
        Some(crate::actions::Intent::PreviousFocus) => previous(),
        _ => false,
    }
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
    /// Upstream's `descendantsAreFocusable` and `descendantsAreTraversable`,
    /// both true by default: a plain `Focus` gets in nobody's way.
    descendants_focusable: bool,
    descendants_traversable: bool,
    id: u64,
    child: RefCell<Option<AnyWidget>>,
    on_key: Option<KeyHandler>,
    on_focus_change: Option<Rc<dyn Fn(bool)>>,
    traversable: bool,
    /// Whether tapping this region focuses it. On by default, because a
    /// reader who taps something expects to be typing into it.
    focus_on_tap: bool,
    /// Whether this node is a [`FocusTraversalGroup`] boundary.
    group: bool,
    /// What Enter or Space does to this node. Upstream's `ActivateIntent`,
    /// which a control registers an `ActivateAction` for.
    ///
    /// # Why it lives on `Focus` rather than in an `Actions` map
    ///
    /// Upstream's controls are wrapped in `FocusableActionDetector`, which is
    /// a `Focus`, a `Shortcuts` and an `Actions` in one. This crate already
    /// makes the same consolidation for the other key every control shares:
    /// [`handle_traversal_key`] asks [`crate::shortcuts::default_shortcuts`]
    /// directly rather than requiring an app-level `Shortcuts` scope above
    /// every button. Activation follows it, and for the same reason -- a
    /// control that needed a scope installed above it to answer Enter would
    /// answer it in some apps and not others.
    ///
    /// The `Shortcuts` widget is still what a caller uses for *their own*
    /// bindings. This is only the one binding every operable control has.
    on_activate: Option<Rc<dyn Fn()>>,
    /// Upstream's `autofocus`: take the keyboard when this node first appears.
    ///
    /// Asked for on the build that *registers* the node and not on the ones
    /// after it -- see the build, where "is there already an entry with this
    /// id" is what tells the two apart. Upstream draws the same line by
    /// asking in `initState` rather than in `build`.
    autofocus: bool,
}

impl Focus {
    pub fn new(id: u64, child: AnyWidget) -> Focus {
        Focus {
            id,
            child: RefCell::new(Some(child)),
            on_key: None,
            on_focus_change: None,
            traversable: true,
            descendants_focusable: true,
            descendants_traversable: true,
            focus_on_tap: true,
            group: false,
            autofocus: false,
            on_activate: None,
        }
    }

    /// What Enter or Space does here. See the field.
    pub fn with_on_activate(mut self, activate: impl Fn() + 'static) -> Self {
        self.on_activate = Some(Rc::new(activate));
        self
    }

    /// The handler this node registers: the caller's own, and the activation
    /// binding behind it.
    ///
    /// The caller's runs **first**, so a control that wants Enter for
    /// something else can take it -- a text field's Enter submits, and it
    /// must not also press the button the field happens to be inside.
    fn key_handler(&self) -> Option<KeyHandler> {
        let Some(activate) = self.on_activate.clone() else {
            return self.on_key.clone();
        };
        let own = self.on_key.clone();
        Some(Rc::new(move |event: &KeyEvent| {
            if let Some(own) = &own {
                if own(event) == KeyResult::Handled {
                    return KeyResult::Handled;
                }
            }
            if !is_activation(event) {
                return KeyResult::Ignored;
            }
            activate();
            KeyResult::Handled
        }))
    }

    /// Upstream's `autofocus`. See the field.
    pub fn with_autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    /// Marks this node as a traversal group boundary -- what
    /// [`FocusTraversalGroup`] builds.
    fn as_group(mut self) -> Self {
        self.group = true;
        self
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

    /// Upstream's `descendantsAreFocusable`, which is what `ExcludeFocus` is
    /// built out of.
    pub fn with_descendants_focusable(mut self, focusable: bool) -> Self {
        self.descendants_focusable = focusable;
        self
    }

    /// Upstream's `descendantsAreTraversable`.
    pub fn with_descendants_traversable(mut self, traversable: bool) -> Self {
        self.descendants_traversable = traversable;
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
            .inherited::<FocusAncestors>()
            .map(|s| s.0.clone())
            .unwrap_or_default();
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        // Anything focusable below this is inside it, and says so by reading
        // what this publishes.
        let mut inner = scope.clone();
        inner.push(self.id);
        let built = crate::framework::provide(FocusAncestors(inner), child);

        // Registered from the build, which is where upstream registers too
        // (`_FocusState.initState` and `didUpdateWidget`). It survives the
        // frames this widget does not rebuild, because the registry is not
        // rebuilt per frame -- see `prune`.
        // The explicit order, if an enclosing `FocusTraversalOrder` published
        // one, and the innermost enclosing group. Both are read here because
        // this is where the ancestor chain is still in hand.
        let order = context
            .inherited::<FocusOrder>()
            .map(|order| (*order).clone());
        let group = enclosing_group(&scope);
        // The first build of this node, as opposed to a rebuild: an entry
        // exists only once `register` below has run for it, and `prune` takes
        // it away again when the element goes. So a node that is remounted
        // asks again, which is what upstream does too -- `initState` runs
        // again for a new state object.
        if self.autofocus
            && !MANAGER.with(|manager| {
                manager
                    .borrow()
                    .entries
                    .iter()
                    .any(|entry| entry.id == self.id)
            })
        {
            autofocus_node(self.id);
        }
        register(FocusEntry {
            id: self.id,
            element: context.element_ref(),
            ancestors: scope,
            on_key: self.key_handler(),
            traversable: self.traversable,
            descendants_focusable: self.descendants_focusable,
            descendants_traversable: self.descendants_traversable,
            on_focus_change: self.on_focus_change.clone(),
            order,
            group,
            is_group: self.group,
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
            Box::new(
                crate::render::RenderPointerRegion::new(id, child)
                    .with_handlers(handlers)
                    // Upstream's `Focus` defers to its child: it never claims a
                    // hit where nothing under it did. That matters for modal
                    // barriers, which sit *under* a full-screen focus group in
                    // the overlay -- an opaque focus region there would swallow
                    // every outside tap before the barrier saw it.
                    .with_behavior(crate::render::HitTestBehavior::DeferToChild),
            )
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

    /// Mounts an operable control, paints, and hands back what was drawn.
    fn operable_drawn(
        shape: FocusShape,
        focus_first: bool,
    ) -> Vec<crate::engine_test_stubs::Drawn> {
        use crate::engine_test_stubs::{drawn, reset_drawn};
        use crate::render::RenderBox;

        reset();
        reset_pending_autofocus();
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            operable(
                95,
                false,
                Some(std::rc::Rc::new(|_| {})),
                shape,
                leaf(|| SizedBox::new(40.0, 40.0)),
            ),
        ));
        let _ = tree.build_render_tree();
        if focus_first {
            assert!(focus(95), "the control took the keyboard");
        }
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("a render tree");
        root.layout(crate::render::BoxConstraints::tight(200.0, 100.0));
        let mut layers = crate::engine::LayerTree::new(200, 100);
        reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(200.0, 100.0),
            );
            root.paint(&mut context, crate::render::Offset::ZERO);
        }
        drawn()
    }

    /// The rounded rectangles drawn, as (width, height, radius).
    fn rounded(calls: &[crate::engine_test_stubs::Drawn]) -> Vec<(f32, f32, f32)> {
        use crate::engine_test_stubs::Drawn;
        calls
            .iter()
            .filter_map(|call| match call {
                Drawn::RRect {
                    left,
                    top,
                    right,
                    bottom,
                    radius,
                    ..
                } => Some((right - left, bottom - top, *radius)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_control_shows_where_the_keyboard_is_and_only_while_it_is_there() {
        // Five controls became keyboard stops over ticks 525-528 and none of
        // them showed it: a reader tabbing through a form could operate every
        // control and could not see which one they were on.
        let unfocused = rounded(&operable_drawn(
            FocusShape::Box {
                corner_radius: 16.0,
            },
            false,
        ));
        assert!(
            unfocused.is_empty(),
            "nothing is drawn until the keyboard arrives: {unfocused:?}"
        );

        let focused = rounded(&operable_drawn(
            FocusShape::Box {
                corner_radius: 16.0,
            },
            true,
        ));
        assert_eq!(focused.len(), 1, "one highlight: {focused:?}");
        assert!(
            (focused[0].2 - 16.0).abs() < 0.01,
            "rounded as the control is: {focused:?}"
        );
    }

    #[test]
    fn a_toggleable_is_highlighted_by_a_disc_and_not_by_its_box() {
        // `kRadialReactionRadius`. A checkbox draws a tick in about eighteen
        // pixels and reacts inside forty, so a highlight the size of the tick
        // would look like a second, smaller control sitting inside the first.
        let marks = rounded(&operable_drawn(FocusShape::radial(), true));
        assert_eq!(marks.len(), 1, "{marks:?}");
        let (width, height, radius) = marks[0];
        let diameter = FocusShape::RADIAL_REACTION_RADIUS * 2.0;
        assert!(
            (width - diameter).abs() < 0.01 && (height - diameter).abs() < 0.01,
            "a disc forty across: {marks:?}"
        );
        assert!(
            (radius - FocusShape::RADIAL_REACTION_RADIUS).abs() < 0.01,
            "and rounded by its own radius, which is what makes it round"
        );
    }

    #[test]
    fn enter_and_space_press_the_focused_control() {
        // Before this, nothing in the crate acted on `Intent::Activate`: the
        // shortcut tables named it, the focus found the node, and no widget
        // did anything. Every control was unpressable from the keyboard.
        use std::cell::Cell;
        use std::rc::Rc;

        reset();
        reset_pending_autofocus();
        let presses = Rc::new(Cell::new(0));
        let counter = Rc::clone(&presses);

        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::component(
            Focus::new(91, leaf(|| SizedBox::new(10.0, 10.0)))
                .with_on_activate(move || counter.set(counter.get() + 1)),
        ));
        let _root = tree.build_render_tree().expect("mounted");
        assert!(focus(91));

        assert!(dispatch_key(&key(LogicalKey::ENTER)));
        assert_eq!(presses.get(), 1);
        assert!(dispatch_key(&key(LogicalKey::SPACE)));
        assert_eq!(presses.get(), 2, "space presses it too");

        // A key that is not an activation is not one.
        assert!(!dispatch_key(&key(LogicalKey::KEY_A)));
        assert_eq!(presses.get(), 2);
    }

    #[test]
    fn a_control_that_wants_enter_for_itself_keeps_it() {
        // A text field's Enter submits, and it must not also press whatever
        // the field happens to sit inside. The caller's own handler runs
        // first and can take the key.
        use std::cell::Cell;
        use std::rc::Rc;

        reset();
        reset_pending_autofocus();
        let pressed = Rc::new(Cell::new(false));
        let flag = Rc::clone(&pressed);

        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::component(
            Focus::new(92, leaf(|| SizedBox::new(10.0, 10.0)))
                .with_on_key(|_| KeyResult::Handled)
                .with_on_activate(move || flag.set(true)),
        ));
        let _root = tree.build_render_tree().expect("mounted");
        assert!(focus(92));

        assert!(dispatch_key(&key(LogicalKey::ENTER)), "the key was used");
        assert!(!pressed.get(), "but not to press the control");
    }

    #[test]
    fn a_focus_asking_for_the_keyboard_gets_it_on_the_frame_it_appears() {
        // Upstream's `Focus(autofocus: true)`. Any widget built on `Focus`
        // gets this, which is why it lives here and not in each of them.
        reset();
        reset_pending_autofocus();

        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::component(
            Focus::new(81, leaf(|| SizedBox::new(10.0, 10.0))).with_autofocus(true),
        ));
        let _root = tree.build_render_tree().expect("mounted");
        apply_pending_autofocus();
        assert_eq!(focused(), Some(81));
    }

    #[test]
    fn a_granted_autofocus_is_not_granted_a_second_time() {
        // Once given, the request is spent: the reader moves on and stays
        // moved on.
        //
        // Note what this does *not* see. Nothing here marks the node dirty,
        // so `Focus::build` never runs again -- a mutation asking on every
        // build survives this test, and did. The case where a rebuild really
        // happens is in cupertino.rs, where a switch rebuilds whenever its
        // focus changes; that is the test the "first build only" rule
        // actually rests on.
        reset();
        reset_pending_autofocus();

        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                crate::framework::component(
                    Focus::new(82, leaf(|| SizedBox::new(10.0, 10.0))).with_autofocus(true),
                ),
                focusable(83, leaf(|| SizedBox::new(10.0, 10.0))),
            ],
            |children| {
                let mut column = Column::new();
                for child in children {
                    column = column.push(child);
                }
                column
            },
        ));
        let _root = tree.build_render_tree().expect("mounted");
        apply_pending_autofocus();
        assert_eq!(focused(), Some(82), "it asked once, and was given it");

        assert!(focus(83), "the reader moves on");
        for _ in 0..3 {
            tree.rebuild_dirty();
            let _ = tree.build_render_tree();
            apply_pending_autofocus();
        }
        assert_eq!(focused(), Some(83), "and is not dragged back");
    }

    #[test]
    fn a_modifier_the_table_did_not_ask_for_is_not_traversal() {
        // The difference the table makes over a hand-written `logical == TAB`:
        // `SingleActivator` demands its modifiers **exactly**, so Ctrl+Tab is
        // not the Tab row. A test for the key alone, reading `shift` off the
        // keyboard, traverses on every combination -- and Ctrl+Tab belongs to
        // whatever the application binds it to, usually a tab strip.
        let _tree = two_fields();
        let mut control = Keyboard::new();
        let mut down = KeyEvent {
            physical: PhysicalKey::CONTROL_LEFT,
            logical: LogicalKey::CONTROL_LEFT,
            ..key(LogicalKey::TAB)
        };
        control.record(&mut down);

        assert!(!handle_traversal_key(&key(LogicalKey::TAB), &control));
        assert_eq!(focused(), None, "nothing moved");
    }

    #[test]
    fn traversal_answers_a_key_going_down_and_not_one_coming_up() {
        // A release is not a press: acting on both would move the focus twice
        // for one Tab.
        let _tree = two_fields();
        let plain = Keyboard::new();
        let up = KeyEvent {
            change: KeyChange::Up,
            ..key(LogicalKey::TAB)
        };
        assert!(!handle_traversal_key(&up, &plain));
        assert_eq!(focused(), None);

        assert!(handle_traversal_key(&key(LogicalKey::TAB), &plain));
        assert_eq!(focused(), Some(1));
    }

    #[test]
    fn shift_and_tab_really_go_the_other_way() {
        // Three nodes, because with two the wrap makes forwards and backwards
        // land in the same place and a test cannot tell them apart -- which is
        // what let a `PreviousFocus => next()` mutation through.
        reset();
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                focusable(1, leaf(|| SizedBox::new(10.0, 10.0))),
                focusable(2, leaf(|| SizedBox::new(10.0, 10.0))),
                focusable(3, leaf(|| SizedBox::new(10.0, 10.0))),
            ],
            |children| {
                let mut column = Column::new();
                for child in children {
                    column = column.push(child);
                }
                column
            },
        ));
        let _root = tree.build_render_tree().expect("mounted");

        let plain = Keyboard::new();
        assert!(handle_traversal_key(&key(LogicalKey::TAB), &plain));
        assert!(handle_traversal_key(&key(LogicalKey::TAB), &plain));
        assert_eq!(focused(), Some(2), "two forwards from nothing");
        assert!(handle_traversal_key(&key(LogicalKey::TAB), &with_shift()));
        assert_eq!(focused(), Some(1), "and one back is the one before it");
    }

    #[test]
    fn the_traversal_key_is_whatever_the_default_table_says_it_is() {
        // The rule used to be written twice: this function tested for Tab
        // itself while `WidgetsApp.defaultShortcuts` also said Tab. It reads
        // the table now, so a table that stopped binding Tab would stop this
        // -- and, more to the point, so the two cannot disagree.
        let table = crate::shortcuts::default_shortcuts(
            crate::editable_text::TargetPlatform::host(),
            false,
        );
        let plain = Keyboard::new();
        let names = |event: &KeyEvent, keyboard: &Keyboard| {
            table
                .intent_for(event, keyboard)
                .map(crate::actions::Intent::action_name)
        };
        assert_eq!(names(&key(LogicalKey::TAB), &plain), Some("NextFocus"));
        assert_eq!(
            names(&key(LogicalKey::TAB), &with_shift()),
            Some("PreviousFocus")
        );

        // And a key the table gives another meaning is not traversal: escape
        // dismisses, and this function leaves it alone rather than swallowing
        // it.
        assert_eq!(names(&key(LogicalKey::ESCAPE), &plain), Some("Dismiss"));
        let _tree = two_fields();
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

    // -- Explicit order and groups (upstream `focus_traversal.dart`) ----------

    /// Mounts `children` as a column and builds the render tree, so that
    /// every focus node in it has registered.
    fn mounted(children: Vec<AnyWidget>) -> ElementTree {
        reset();
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(children, |built| {
            let mut column = Column::new();
            for child in built {
                column = column.push(child);
            }
            Box::new(column)
        }));
        let _ = tree.build_render_tree();
        tree
    }

    fn field(id: u64) -> AnyWidget {
        focusable(id, leaf(|| SizedBox::new(10.0, 10.0)))
    }

    #[test]
    fn a_subtree_may_be_shut_out_of_tab_and_still_be_focusable() {
        // Upstream's two group flags, and the reason there are two. A page
        // under a dialog is untraversable -- Tab must stay in the dialog --
        // and still focusable, because the application may address a field on
        // it deliberately.
        let _tree = mounted(vec![
            field(1),
            FocusTraversalGroup::untraversable(90, field(2)),
            field(3),
        ]);

        // Tab walks past the whole group.
        assert!(next());
        assert_eq!(focused(), Some(1));
        assert!(next());
        assert_eq!(focused(), Some(3), "and not 2, which is inside the group");

        // But the node inside is not out of reach.
        assert!(focus(2), "something may still put the keyboard there");
        assert_eq!(focused(), Some(2));
    }

    #[test]
    fn and_one_that_is_shut_out_of_focus_is_out_of_reach_entirely() {
        // The other flag. Nothing inside may take the keyboard, however it is
        // asked -- which is what `ExcludeFocus` is built out of, and why it is
        // a group flag: a page says it once and every control on it stops
        // answering.
        let _tree = mounted(vec![
            field(1),
            FocusTraversalGroup::unfocusable(90, field(2)),
            field(3),
        ]);

        assert!(!focus(2), "asking directly is refused too");
        assert_ne!(focused(), Some(2));

        assert!(next());
        assert_eq!(focused(), Some(1));
        assert!(next());
        assert_eq!(focused(), Some(3), "and Tab does not go there either");
    }

    #[test]
    fn the_implication_runs_one_way_only() {
        // Upstream's `traversalDescendants` filters on `canRequestFocus` as
        // well as `skipTraversal`, so unfocusable **is** untraversable -- Tab
        // landing on a node that then refuses the keyboard reads as a dead key
        // press. The reverse does not hold, and the pair of tests above is
        // what says so.
        //
        // Written as one test because the two halves only mean something
        // together: either alone is satisfied by making both flags do the same
        // thing.
        let _tree = mounted(vec![
            FocusTraversalGroup::unfocusable(90, field(1)),
            FocusTraversalGroup::untraversable(91, field(2)),
            field(3),
        ]);
        // The traversal half first, from a clean state: neither group is a
        // stop, so the only one is 3.
        assert!(next());
        assert_eq!(focused(), Some(3), "the only stop there is");

        // Then the reachability half, which is where the two part company.
        assert!(!focus(1), "unfocusable: out of reach");
        assert!(focus(2), "untraversable: still reachable");
    }

    #[test]
    fn one_ancestor_saying_no_is_enough_however_far_up_it_is() {
        // `canRequestFocus` is `every ancestor allows it`, not "the nearest
        // one". A group nested inside an unfocusable one cannot re-admit its
        // children by saying nothing.
        let _tree = mounted(vec![
            field(1),
            FocusTraversalGroup::unfocusable(90, FocusTraversalGroup::new(91, field(2))),
        ]);
        assert!(!focus(2), "the outer group still refuses");
        assert!(focus(1), "and a node outside it is unaffected");
    }

    #[test]
    fn a_plain_group_gets_in_nobodys_way() {
        // Both flags default to true, so grouping alone changes neither
        // question. Without this the two tests above would hold just as well
        // if *every* group were shut, and they would be about grouping rather
        // than about the flags.
        let _tree = mounted(vec![
            field(1),
            FocusTraversalGroup::new(90, field(2)),
            field(3),
        ]);
        assert!(focus(2));
        assert_eq!(focused(), Some(2));
        assert!(next());
        assert_eq!(focused(), Some(3), "and Tab passes through it");
    }

    #[test]
    fn an_explicit_order_beats_the_order_things_were_built_in() {
        // Built 1, 2, 3; ordered 3, 1, 2.
        let _tree = mounted(vec![
            FocusTraversalOrder::new(NumericFocusOrder::new(2.0), field(1)),
            FocusTraversalOrder::new(NumericFocusOrder::new(3.0), field(2)),
            FocusTraversalOrder::new(NumericFocusOrder::new(1.0), field(3)),
        ]);
        assert!(next());
        assert_eq!(focused(), Some(3));
        assert!(next());
        assert_eq!(focused(), Some(1));
        assert!(next());
        assert_eq!(focused(), Some(2));
        assert!(next());
        assert_eq!(focused(), Some(3), "and it wraps");
    }

    #[test]
    fn a_lexical_order_sorts_by_the_string() {
        let _tree = mounted(vec![
            FocusTraversalOrder::new(LexicalFocusOrder::new("b"), field(1)),
            FocusTraversalOrder::new(LexicalFocusOrder::new("a"), field(2)),
        ]);
        assert!(next());
        assert_eq!(focused(), Some(2));
        assert!(next());
        assert_eq!(focused(), Some(1));
    }

    #[test]
    fn nodes_with_no_order_follow_the_ordered_ones() {
        // Upstream's `sortDescendants`: the ordered ones first, then the
        // rest as they came.
        let _tree = mounted(vec![
            field(1),
            FocusTraversalOrder::new(NumericFocusOrder::new(1.0), field(2)),
            field(3),
        ]);
        assert!(next());
        assert_eq!(focused(), Some(2), "the only ordered node leads");
        assert!(next());
        assert_eq!(focused(), Some(1));
        assert!(next());
        assert_eq!(focused(), Some(3));
    }

    #[test]
    fn a_traversal_group_keeps_its_members_together() {
        // A group holding 20 and 21, built between 1 and 2. Tab should walk
        // 1, then the group's two, then 2 -- and it would do that here from
        // build order alone, so the interesting case is the one below.
        let _tree = mounted(vec![
            field(1),
            FocusTraversalGroup::new(
                100,
                crate::framework::many(vec![field(20), field(21)], |built| {
                    let mut column = Column::new();
                    for child in built {
                        column = column.push(child);
                    }
                    Box::new(column)
                }),
            ),
            field(2),
        ]);
        let mut walk = Vec::new();
        for _ in 0..4 {
            next();
            walk.push(focused());
        }
        assert_eq!(walk, vec![Some(1), Some(20), Some(21), Some(2)]);
    }

    #[test]
    fn an_order_inside_a_group_does_not_reach_outside_it() {
        // 1 has no order and is outside the group; 20 and 21 are inside it
        // and ordered backwards. The group's members stay together and sort
        // among themselves -- an order in one group cannot jump a node past
        // the nodes of another, which is the whole point of grouping.
        let _tree = mounted(vec![
            field(1),
            FocusTraversalGroup::new(
                100,
                crate::framework::many(
                    vec![
                        FocusTraversalOrder::new(NumericFocusOrder::new(2.0), field(20)),
                        FocusTraversalOrder::new(NumericFocusOrder::new(1.0), field(21)),
                    ],
                    |built| {
                        let mut column = Column::new();
                        for child in built {
                            column = column.push(child);
                        }
                        Box::new(column)
                    },
                ),
            ),
        ]);
        let mut walk = Vec::new();
        for _ in 0..3 {
            next();
            walk.push(focused());
        }
        assert_eq!(walk, vec![Some(1), Some(21), Some(20)]);
    }

    #[test]
    fn the_focus_actions_move_the_keyboard() {
        use crate::actions::{
            ActionDispatcher, Intent, NextFocusAction, PreviousFocusAction, RequestFocusAction,
        };

        let _tree = mounted(vec![field(1), field(2), field(3)]);
        let dispatcher = ActionDispatcher::new()
            .with_action("RequestFocus", RequestFocusAction::new())
            .with_action("NextFocus", NextFocusAction::new())
            .with_action("PreviousFocus", PreviousFocusAction::new());

        dispatcher.invoke_action(&Intent::RequestFocus { id: 2 });
        assert_eq!(focused(), Some(2));
        dispatcher.invoke_action(&Intent::NextFocus);
        assert_eq!(focused(), Some(3));
        dispatcher.invoke_action(&Intent::PreviousFocus);
        assert_eq!(focused(), Some(2));

        // Upstream's next/previous actions do not consume the key: with
        // nowhere left to go the shell should get its chance at it.
        assert_eq!(
            dispatcher.maybe_invoke(&Intent::NextFocus, &key(LogicalKey::TAB)),
            KeyResult::Ignored
        );
    }

    // -- Scopes ---------------------------------------------------------------------

    /// A scope with `count` focusable children under it, mounted and built.
    ///
    /// The ids are the scope's own and then `scope + 1 ..= scope + count`.
    fn scoped(scope: u64, count: u64) -> ElementTree {
        let mut children = Vec::new();
        for n in 1..=count {
            children.push(focusable(scope + n, leaf(|| Empty)));
        }
        let mut tree = ElementTree::new();
        tree.rebuild(focus_scope_widget(
            scope,
            crate::framework::many(children, |rendered| {
                let mut column = Column::new();
                for child in rendered {
                    column = column.push(child);
                }
                Box::new(column)
            }),
        ));
        tree.build_render_tree();
        tree
    }

    #[test]
    fn a_scope_with_no_history_lands_on_its_first_stop() {
        reset();
        reset_scopes();
        let tree = scoped(100, 3);

        assert_eq!(focused_child(100), None, "nothing has been focused yet");
        assert!(focus_scope(100));
        assert_eq!(
            focused(),
            Some(101),
            "the first traversable node inside it, which is where Tab would go too"
        );
        drop(tree);
        reset_scopes();
    }

    #[test]
    fn a_scope_returns_the_reader_to_where_they_were() {
        // The whole point. Focus the third child, leave, come back.
        reset();
        reset_scopes();
        let tree = scoped(100, 3);

        focus(103);
        assert_eq!(focused_child(100), Some(103), "the scope noticed");

        unfocus();
        assert_eq!(focused(), None);

        assert!(focus_scope(100));
        assert_eq!(focused(), Some(103), "not the first one");
        drop(tree);
        reset_scopes();
    }

    #[test]
    fn the_scope_remembers_the_most_recent_and_not_the_first() {
        reset();
        reset_scopes();
        let tree = scoped(100, 3);

        focus(101);
        focus(102);
        focus(103);
        assert_eq!(focused_child(100), Some(103));
        drop(tree);
        reset_scopes();
    }

    #[test]
    fn focusing_a_scope_that_already_has_the_right_node_changes_nothing() {
        reset();
        reset_scopes();
        let tree = scoped(100, 2);

        focus(102);
        assert!(
            !focus_scope(100),
            "already there, so the keyboard did not move"
        );
        assert_eq!(focused(), Some(102));
        drop(tree);
        reset_scopes();
    }

    #[test]
    fn a_remembered_node_that_has_gone_falls_back_to_the_first() {
        // A row scrolled out of the registry, a dismissed dialog's field. The
        // scope's memory outlives the node, and `focus` refuses an id nothing
        // owns -- so the fallback has to run.
        reset();
        reset_scopes();
        let tree = scoped(100, 2);
        focus(102);
        assert_eq!(focused_child(100), Some(102));
        drop(tree);

        // Dropping the tree does not empty the registry -- `prune` does, once
        // per frame, and that is what really takes a departed node out. Saying
        // so here rather than relying on the drop is the difference between
        // testing the fallback and testing nothing: with 102 still registered
        // and still focused, `focus_scope` correctly has nothing to do.
        prune(|_| false);
        assert_eq!(focused(), None, "focus followed the node that went");

        let mut tree = ElementTree::new();
        tree.rebuild(focus_scope_widget(100, focusable(101, leaf(|| Empty))));
        tree.build_render_tree();

        assert_eq!(focused_child(100), Some(102), "the memory is still there");
        assert!(focus_scope(100));
        assert_eq!(focused(), Some(101), "but it landed on what exists");
        drop(tree);
        reset_scopes();
    }

    #[test]
    fn an_id_that_is_not_a_scope_remembers_nothing() {
        reset();
        reset_scopes();
        let tree = scoped(100, 2);
        focus(101);

        assert!(is_scope(100));
        assert!(!is_scope(101), "an ordinary node is not a scope");
        assert_eq!(focused_child(101), None);
        assert!(!focus_scope(101), "and cannot be entered as one");
        drop(tree);
        reset_scopes();
    }

    #[test]
    fn every_enclosing_scope_is_told_and_not_only_the_nearest() {
        // A reader leaving the outer scope and coming back expects the same
        // place as one leaving the inner scope -- and the outer scope has no
        // way to ask the inner one later.
        reset();
        reset_scopes();
        let mut tree = ElementTree::new();
        tree.rebuild(focus_scope_widget(
            100,
            focus_scope_widget(200, focusable(201, leaf(|| Empty))),
        ));
        tree.build_render_tree();

        focus(201);
        assert_eq!(focused_child(200), Some(201), "the inner scope");
        assert_eq!(focused_child(100), Some(201), "and the outer one");
        drop(tree);
        reset_scopes();
    }

    #[test]
    fn a_scope_is_not_itself_a_tab_stop() {
        // It is the thing containing the stops. A scope that Tab landed on
        // would be a stop the reader cannot see or type into.
        reset();
        reset_scopes();
        let tree = scoped(100, 2);

        let stops = MANAGER.with(|manager| traversal_order(&manager.borrow()));
        assert!(!stops.contains(&100), "{stops:?}");
        assert!(stops.contains(&101) && stops.contains(&102));
        drop(tree);
        reset_scopes();
    }

    #[test]
    fn a_scopes_first_stop_is_the_one_tab_would_reach_first() {
        // The two have to agree, or entering a scope and tabbing into it land
        // in different places.
        reset();
        reset_scopes();
        let tree = scoped(100, 3);

        let first_by_tab = MANAGER
            .with(|manager| traversal_order(&manager.borrow()))
            .into_iter()
            .find(|id| *id != 100);
        assert_eq!(first_focusable_in(100), first_by_tab);
        drop(tree);
        reset_scopes();
    }

    // -- The highlight mode -----------------------------------------------------------

    #[test]
    fn typing_says_keyboard_and_touching_says_touch() {
        reset_highlight();
        assert_eq!(
            highlight_mode(),
            FocusHighlightMode::Traditional,
            "the opening guess, before anything has happened"
        );

        note_touch_interaction();
        assert_eq!(highlight_mode(), FocusHighlightMode::Touch);
        note_key_interaction();
        assert_eq!(highlight_mode(), FocusHighlightMode::Traditional);
        reset_highlight();
    }

    #[test]
    fn a_screen_reader_action_counts_as_touch() {
        // A reader driving the interface through an assistive technology is not
        // looking for a focus ring.
        reset_highlight();
        note_key_interaction();
        note_semantics_interaction();
        assert_eq!(highlight_mode(), FocusHighlightMode::Touch);
        reset_highlight();
    }

    #[test]
    fn a_pinned_strategy_ignores_what_the_reader_is_doing() {
        reset_highlight();
        set_highlight_strategy(FocusHighlightStrategy::AlwaysTraditional);
        note_touch_interaction();
        assert_eq!(highlight_mode(), FocusHighlightMode::Traditional);

        set_highlight_strategy(FocusHighlightStrategy::AlwaysTouch);
        note_key_interaction();
        assert_eq!(highlight_mode(), FocusHighlightMode::Touch);

        // And going back to automatic reveals what was recorded meanwhile.
        set_highlight_strategy(FocusHighlightStrategy::Automatic);
        assert_eq!(highlight_mode(), FocusHighlightMode::Traditional);
        reset_highlight();
    }

    #[test]
    fn the_opening_guess_only_holds_until_something_happens() {
        reset_highlight();
        set_default_highlight_mode(FocusHighlightMode::Touch);
        assert_eq!(highlight_mode(), FocusHighlightMode::Touch);

        note_key_interaction();
        assert_eq!(highlight_mode(), FocusHighlightMode::Traditional);
        set_default_highlight_mode(FocusHighlightMode::Touch);
        assert_eq!(
            highlight_mode(),
            FocusHighlightMode::Traditional,
            "the guess is past its usefulness once there is a real answer"
        );
        reset_highlight();
    }

    #[test]
    fn a_highlight_listener_hears_the_edges_and_not_every_keystroke() {
        reset_highlight();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let recorder = Rc::clone(&heard);
        let token = add_highlight_listener(move |mode| recorder.borrow_mut().push(mode));

        note_key_interaction();
        note_key_interaction();
        note_key_interaction();
        assert!(heard.borrow().is_empty(), "already traditional");

        note_touch_interaction();
        note_touch_interaction();
        assert_eq!(*heard.borrow(), vec![FocusHighlightMode::Touch], "one edge");

        remove_highlight_listener(token);
        note_key_interaction();
        assert_eq!(heard.borrow().len(), 1, "and a removed listener is quiet");
        reset_highlight();
    }

    #[test]
    fn dispatching_a_key_is_what_records_the_keyboard() {
        // Upstream hooks the global key handler; here the one place every key
        // already passes through is `dispatch_key`.
        reset();
        reset_highlight();
        note_touch_interaction();
        assert_eq!(highlight_mode(), FocusHighlightMode::Touch);

        dispatch_key(&key(LogicalKey::TAB));
        assert_eq!(highlight_mode(), FocusHighlightMode::Traditional);
        reset_highlight();
    }

    // -- FocusableActionDetector ---------------------------------------------------------

    fn detector(heard: &Rc<RefCell<Vec<(&'static str, bool)>>>) -> FocusableActionDetector {
        let focus = Rc::clone(heard);
        let hover = Rc::clone(heard);
        FocusableActionDetector::new(true)
            .with_on_show_focus_highlight(move |on| focus.borrow_mut().push(("focus", on)))
            .with_on_show_hover_highlight(move |on| hover.borrow_mut().push(("hover", on)))
    }

    #[test]
    fn a_disabled_control_does_not_light_up_under_the_pointer() {
        reset_highlight();
        note_key_interaction();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let mut d = FocusableActionDetector::new(false).with_on_show_hover_highlight({
            let heard = Rc::clone(&heard);
            move |on| heard.borrow_mut().push(("hover", on))
        });
        d.hover(true);
        assert!(d.state().hovering, "it knows the pointer is there");
        assert!(!d.should_show_hover_highlight(), "and says not to draw it");
        assert!(heard.borrow().is_empty(), "so nothing was announced");
        reset_highlight();
    }

    #[test]
    fn nothing_lights_up_on_a_touchscreen() {
        // The other half of "should a highlight be drawn": a ring on the
        // last-tapped button is noise, and looks like something is selected.
        reset_highlight();
        note_touch_interaction();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let mut d = detector(&heard);
        d.hover(true);
        d.focus(true);
        assert!(!d.should_show_hover_highlight());
        assert!(!d.should_show_focus_highlight());
        assert!(heard.borrow().is_empty());
        reset_highlight();
    }

    #[test]
    fn the_callbacks_fire_on_the_answer_changing_and_not_on_the_event() {
        reset_highlight();
        note_key_interaction();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let mut d = detector(&heard);

        d.hover(true);
        assert_eq!(*heard.borrow(), vec![("hover", true)]);

        // A second enter is not a change and never reaches the machinery.
        d.hover(true);
        assert_eq!(heard.borrow().len(), 1);

        d.focus(true);
        assert_eq!(heard.borrow()[1], ("focus", true));
        reset_highlight();
    }

    #[test]
    fn disabling_a_hovered_control_puts_its_highlight_out() {
        // The state did not change -- the pointer is still there -- but the
        // *answer* did, which is what the callback is about.
        reset_highlight();
        note_key_interaction();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let mut d = detector(&heard);
        d.hover(true);
        heard.borrow_mut().clear();

        d.set_enabled(false);
        assert_eq!(*heard.borrow(), vec![("hover", false)]);
        assert!(d.state().hovering, "and the pointer is still over it");
        reset_highlight();
    }

    #[test]
    fn a_focused_control_lights_up_when_the_reader_reaches_for_the_keyboard() {
        // Nothing about the control changed; the mode did.
        reset_highlight();
        note_touch_interaction();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let mut d = detector(&heard);
        d.focus(true);
        assert!(heard.borrow().is_empty(), "touch: no ring");

        note_key_interaction();
        d.highlight_mode_changed();
        assert_eq!(*heard.borrow(), vec![("focus", true)]);
        reset_highlight();
    }

    #[test]
    fn directional_navigation_shows_the_ring_even_on_a_disabled_control() {
        // Upstream's reason: a reader moving through a screen with a d-pad has
        // no other way to know a disabled control is there, and skipping it
        // silently loses them.
        reset_highlight();
        note_key_interaction();

        let mut traditional = FocusableActionDetector::new(false);
        traditional.focus(true);
        assert!(!traditional.should_show_focus_highlight());

        let mut directional =
            FocusableActionDetector::new(false).with_navigation_mode(NavigationMode::Directional);
        directional.focus(true);
        assert!(directional.should_show_focus_highlight());

        // Hover is *not* excused the same way -- it stays gated on enabled.
        directional.hover(true);
        assert!(!directional.should_show_hover_highlight());
        reset_highlight();
    }

    #[test]
    fn focus_is_announced_before_hover() {
        // Upstream's order. Both change at once when a control is enabled while
        // hovered and focused.
        reset_highlight();
        note_key_interaction();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let mut d = FocusableActionDetector::new(false)
            .with_on_show_focus_highlight({
                let heard = Rc::clone(&heard);
                move |on| heard.borrow_mut().push(("focus", on))
            })
            .with_on_show_hover_highlight({
                let heard = Rc::clone(&heard);
                move |on| heard.borrow_mut().push(("hover", on))
            });
        d.hover(true);
        d.focus(true);
        heard.borrow_mut().clear();

        d.set_enabled(true);
        assert_eq!(
            *heard.borrow(),
            vec![("focus", true), ("hover", true)],
            "focus first"
        );
        reset_highlight();
    }

    // -- ActionListener --------------------------------------------------------------------

    #[test]
    fn every_listener_hears_an_invocation() {
        let heard = Rc::new(RefCell::new(0));
        let mut listeners = ActionListener::new();
        for _ in 0..3 {
            let counter = Rc::clone(&heard);
            listeners.add(move |_| *counter.borrow_mut() += 1);
        }
        listeners.notify(&crate::actions::Intent::DoNothing);
        assert_eq!(*heard.borrow(), 3);
    }

    #[test]
    fn a_removed_listener_stops_hearing_and_the_tokens_after_it_still_work() {
        let heard = Rc::new(RefCell::new(Vec::new()));
        let mut listeners = ActionListener::new();
        let a = {
            let heard = Rc::clone(&heard);
            listeners.add(move |_| heard.borrow_mut().push("a"))
        };
        let b = {
            let heard = Rc::clone(&heard);
            listeners.add(move |_| heard.borrow_mut().push("b"))
        };
        let c = {
            let heard = Rc::clone(&heard);
            listeners.add(move |_| heard.borrow_mut().push("c"))
        };

        assert!(listeners.remove(a));
        assert!(listeners.remove(c), "c's token still finds c");
        assert!(!listeners.remove(a), "and removing one twice finds nothing");

        listeners.notify(&crate::actions::Intent::DoNothing);
        assert_eq!(*heard.borrow(), vec!["b"]);
        assert_eq!(listeners.len(), 1);
        let _ = b;
    }

    #[test]
    fn an_empty_listener_list_is_not_an_error() {
        let listeners = ActionListener::new();
        assert!(listeners.is_empty());
        listeners.notify(&crate::actions::Intent::DoNothing);
    }
}

#[cfg(test)]
mod traversal_edge_tests {
    use super::{EdgeOutcome, TraversalEdgeBehavior, tab_alignment};
    use crate::directional_traversal::ScrollPositionAlignmentPolicy;

    #[test]
    fn leaving_the_view_and_stopping_both_refuse_the_move() {
        // They agree on the answer they report, which is why it is easy to
        // treat them as one.
        let leave = TraversalEdgeBehavior::LeaveFlutterView.at_edge(false);
        let stop = TraversalEdgeBehavior::Stop.at_edge(false);
        assert!(!leave.definitely_handled());
        assert!(!stop.definitely_handled());
    }

    #[test]
    fn but_only_one_of_them_lets_go_of_the_focus() {
        // The whole difference. Collapsing them leaves a focus ring sitting on
        // a widget while the browser takes the next tab.
        assert!(
            TraversalEdgeBehavior::LeaveFlutterView
                .at_edge(false)
                .unfocuses()
        );
        assert!(!TraversalEdgeBehavior::Stop.at_edge(false).unfocuses());
        assert_ne!(
            TraversalEdgeBehavior::LeaveFlutterView.at_edge(false),
            TraversalEdgeBehavior::Stop.at_edge(false)
        );
    }

    #[test]
    fn a_parent_scope_with_nowhere_to_go_wraps_rather_than_stops() {
        // Upstream: "No valid parent scope. Fallback to closed loop behavior."
        assert_eq!(
            TraversalEdgeBehavior::ParentScope.at_edge(false),
            EdgeOutcome::Wrap
        );
        assert_eq!(
            TraversalEdgeBehavior::ParentScope.at_edge(false),
            TraversalEdgeBehavior::ClosedLoop.at_edge(false),
            "at the top level the two are the same behaviour"
        );
        assert_ne!(
            TraversalEdgeBehavior::ParentScope.at_edge(false),
            TraversalEdgeBehavior::Stop.at_edge(false),
            "a scope that meant to defer outward is a ring, not a wall"
        );
    }

    #[test]
    fn and_with_somewhere_to_go_it_defers() {
        assert_eq!(
            TraversalEdgeBehavior::ParentScope.at_edge(true),
            EdgeOutcome::DelegateToParent
        );
        // ParentScope is the only behaviour the parent's existence changes.
        for behavior in TraversalEdgeBehavior::ALL {
            if behavior == TraversalEdgeBehavior::ParentScope {
                assert_ne!(behavior.at_edge(true), behavior.at_edge(false));
            } else {
                assert_eq!(
                    behavior.at_edge(true),
                    behavior.at_edge(false),
                    "{behavior:?}"
                );
            }
        }
    }

    #[test]
    fn delegating_lets_go_but_does_not_claim_to_have_moved_anything() {
        // Upstream verifies rather than assumes: it re-reads the focused child
        // and compares. So delegation is an unfocus whose outcome is the
        // parent's to report.
        let delegate = TraversalEdgeBehavior::ParentScope.at_edge(true);
        assert!(delegate.unfocuses());
        assert!(!delegate.definitely_handled());
    }

    #[test]
    fn only_wrapping_reports_the_move_as_its_own() {
        let outcomes = [
            EdgeOutcome::Wrap,
            EdgeOutcome::Unfocus,
            EdgeOutcome::DelegateToParent,
            EdgeOutcome::Stay,
        ];
        let handled: Vec<EdgeOutcome> = outcomes
            .into_iter()
            .filter(|o| o.definitely_handled())
            .collect();
        assert_eq!(handled, vec![EdgeOutcome::Wrap]);
        // And wrapping is the one that keeps a focus.
        assert!(!EdgeOutcome::Wrap.unfocuses());
    }

    #[test]
    fn the_scroll_alignment_follows_the_direction_and_not_the_destination() {
        // Wrapping forward lands on the *first* node and still asks to keep it
        // visible at the end, because the policy is about which way the user
        // is moving. The naive reading -- first node, so align at the start --
        // is what upstream does not do.
        assert_eq!(
            tab_alignment(true),
            ScrollPositionAlignmentPolicy::KeepVisibleAtEnd
        );
        assert_eq!(
            tab_alignment(false),
            ScrollPositionAlignmentPolicy::KeepVisibleAtStart
        );
        assert_ne!(tab_alignment(true), tab_alignment(false));
    }

    #[test]
    fn a_scope_is_a_ring_unless_it_says_otherwise() {
        assert_eq!(
            TraversalEdgeBehavior::default(),
            TraversalEdgeBehavior::ClosedLoop
        );
        // Which is what this crate's `next` and `previous` do today: they wrap
        // and have no scope to defer to.
        assert_eq!(
            TraversalEdgeBehavior::default().at_edge(false),
            EdgeOutcome::Wrap
        );
    }
}
