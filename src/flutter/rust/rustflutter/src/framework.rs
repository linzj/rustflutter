// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Widgets, elements and state.
//!
//! The render layer is a tree of objects that do work. This layer is the tree
//! of *descriptions* above it, plus the tree of *instances* in between that
//! remembers things between frames. Upstream those three are `Widget`,
//! `Element` and `RenderObject`; the names and the roles are the same here.
//!
//! ```text
//!   Widget      cheap, immutable, thrown away and rebuilt every frame
//!   Element     persistent: holds state, and decides what to reuse
//!   RenderObject  does layout, paint and hit testing
//! ```
//!
//! Without the middle layer there is nowhere for a counter to live: a widget
//! that is rebuilt each frame cannot remember anything, and a render object
//! does not know it is the same one as last frame. The element tree is what
//! makes `set_state` mean "this subtree changed" rather than "everything did".
//!
//! # Why an arena
//!
//! Upstream an `Element` holds a parent pointer, a child list, and a reference
//! to its `RenderObject`, all mutable, all cyclic. That shape does not survive
//! contact with Rust's borrow checker -- `Rc<RefCell<Element>>` everywhere
//! would compile and would then panic at run time the first time a build
//! touched its own ancestor.
//!
//! So elements live in a slab, keyed by [`ElementId`], and every link is an
//! index. Nothing is borrowed for longer than one operation, cycles are not
//! representable, and a stale handle is a lookup that returns `None` rather
//! than a dangling pointer.
//!
//! # What reuse means
//!
//! On rebuild, a new widget is matched against the element that is already
//! there. They match if the widget has the same concrete type and the same
//! [`Key`]. A match updates the element in place, so its state survives; a
//! mismatch unmounts the old subtree and mounts a new one, so its state does
//! not. That single rule is what a `Key` is for: it lets the caller say "this
//! is still the same thing" when position alone would say otherwise.
//!
//! An element that is updated in place does not make a second render object
//! either. It hands the new description to the one it has, which takes what
//! differs and says whether anything it takes has to be measured or drawn
//! again -- upstream's `updateRenderObject`, and the reason a screen that
//! rebuilt is not a screen that has to be re-measured. See
//! [`crate::render::RenderBox::update_from`].

use std::any::{Any, TypeId, type_name};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::render::BoxedRender;

// -- Identity -----------------------------------------------------------------

/// Distinguishes two widgets of the same type in the same position.
///
/// Without one, a list that reorders its children re-associates every element
/// with a different item, and any state they held goes with the position
/// rather than the item.
pub type Key = Option<u64>;

/// Where [`GlobalKey::new`] gets the next id.
static NEXT_GLOBAL_KEY: AtomicU64 = AtomicU64::new(0);

/// A key that is unique across the whole tree, not just across one child list.
///
/// A [`Key`] tells reconciliation "these two widgets are the same thing" *when
/// they meet in the same list*. A global key says it from anywhere: an element
/// dropped by one parent this frame can be picked up by another, with its
/// state and its render objects intact, because the key is how the new parent
/// finds it -- position is not.
///
/// That is upstream's `GlobalKey`, and the whole of what it is for here: a
/// move between parents is otherwise indistinguishable from a drop followed
/// by a mount, and a drop followed by a mount is exactly what loses state.
///
/// The registry that makes the key global is per tree (see
/// [`ElementTree::current_element`]), so the same `GlobalKey` in two trees is
/// two unrelated keys -- the same restraint the rest of this file works
/// under, one tree per owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlobalKey(u64);

impl GlobalKey {
    /// A key that has never been handed out before.
    ///
    /// Make one once -- in the state that owns the subtree being moved, not in
    /// the build that mentions it -- the way upstream's documentation asks for
    /// the same thing.
    pub fn new() -> GlobalKey {
        GlobalKey(NEXT_GLOBAL_KEY.fetch_add(1, Ordering::Relaxed))
    }
}

/// Index into the element arena. Cheap to copy; may be stale, in which case
/// every lookup returns `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(usize);

impl ElementId {
    pub fn index(self) -> usize {
        self.0
    }
}

// -- Widgets ------------------------------------------------------------------

/// A widget that describes itself in terms of other widgets, and holds no
/// state of its own.
pub trait Component: 'static {
    fn key(&self) -> Key {
        None
    }

    fn build(&self, context: &mut BuildContext) -> AnyWidget;
}

/// A widget with state that outlives its own rebuilds.
///
/// The widget is still thrown away every frame; `State` is what persists, and
/// it is reached through the [`StateHandle`] that `build` is given.
pub trait StatefulComponent: 'static {
    type State: Default + 'static;

    fn key(&self) -> Key {
        None
    }

    /// Advances time-dependent state, once per frame, before anything is
    /// built.
    ///
    /// This is where a transition moves and a ticker ticks. It exists because
    /// `build` gets the state by shared reference -- deliberately, so that a
    /// build cannot change what it is drawing halfway through -- and something
    /// has to be allowed to move the clock forward.
    ///
    /// Return true to ask for another frame. Frames are on demand, so an
    /// animation that stops asking stops running.
    fn advance(&self, _state: &mut Self::State, _frame_time_micros: i64) -> bool {
        false
    }

    /// Called when the element is rebuilt with a new widget of the same type
    /// and key, with the widget it replaces, before `build` runs.
    ///
    /// Upstream's `State.didUpdateWidget`, with the receiver flipped: there it
    /// is a method on the state that reads the new widget off itself, and here
    /// the state is plain data so the new widget is the receiver and the old
    /// one the argument. This is where an implicit animation notices its
    /// target moved; see [`crate::implicit::Animated`].
    ///
    /// `build` follows immediately and draws whatever this wrote, which is why
    /// there is nothing to return: unlike `advance` there is no frame to ask
    /// for, because one is already on its way.
    fn did_update_widget(&self, _old: &Self, _state: &mut Self::State) {}

    /// Called once, when the element is taken out of the tree for good.
    ///
    /// Upstream's `State.dispose`, and it exists for upstream's reason: state
    /// can own things the tree does not -- a timer, a listener, an entry in an
    /// overlay that lives somewhere else -- and dropping the state is not the
    /// same as letting those go. Rust frees the memory either way; what it
    /// cannot do is know that an `Rc` handed to an overlay above the navigator
    /// was meant to be taken back out.
    ///
    /// That is not hypothetical. The gallery's snackbar put its bar in the
    /// root overlay and its clock in a component inside the route, so popping
    /// the route dropped the clock and left the bar on screen for ever, with
    /// nothing left that could reach it.
    ///
    /// Not called for an element merely deactivated -- a [`GlobalKey`] may
    /// still claim it -- only when it is released.
    fn dispose(&self, _state: &mut Self::State) {}

    /// The state this widget starts with, used once when its element is
    /// mounted. Defaults to `State::default()`.
    ///
    /// Override it when the starting state depends on the widget rather than
    /// on nothing -- a screen that should open on a particular tab, a form
    /// pre-filled from what it was given. The alternative is a `set_state` from
    /// inside the first build, which works and takes an extra frame to become
    /// visible.
    fn initial_state(&self) -> Self::State {
        Self::State::default()
    }

    fn build(
        &self,
        state: &Self::State,
        handle: StateHandle<Self::State>,
        context: &mut BuildContext,
    ) -> AnyWidget;
}

/// A widget that is a render object, assembled from its children's.
///
/// The three combinators [`leaf`], [`single`] and [`many`] cover almost every
/// use; this trait is what they are built on and what a custom render widget
/// implements directly.
pub trait RenderWidget: 'static {
    fn key(&self) -> Key {
        None
    }

    /// The child widgets, in paint order.
    fn children(&self) -> Vec<AnyWidget>;

    /// Builds the render object, given the children's already-built ones in
    /// the same order.
    ///
    /// Upstream's `createRenderObject`, except that upstream deliberately says
    /// "this method should not do anything with the children" and attaches them
    /// separately through the element's slots. Here they are arguments, because
    /// a parent owns its children directly and there is nothing to attach them
    /// to before the parent exists.
    fn create_render(&self, children: Vec<BoxedRender>) -> BoxedRender;
}

/// A widget with its concrete type erased.
///
/// Cheap to clone, and that is load-bearing rather than a convenience. An
/// element is rebuilt by running `build` on the widget it is already holding
/// -- that is what a `set_state` or an inherited value changing does, and it is
/// what upstream's `Element.rebuild` does too. Upstream can afford it because a
/// Dart widget holds `final Widget child` and reading a field is free; a
/// component here has to hand its child *out* of a `&self` build, so if that
/// were a move the second build would find nothing and replace the subtree with
/// `Empty`. Sharing the widget makes handing it out repeatable, which is the
/// property the element machinery was written against.
#[derive(Clone)]
pub struct AnyWidget {
    inner: WidgetKind,
    type_id: TypeId,
    key: Key,
    /// Set only by [`with_global_key`]. Rides the widget the same way
    /// [`AnyWidget::provided`] does, so an element can be found by it from
    /// anywhere in the tree; see [`GlobalKey`].
    global_key: Option<GlobalKey>,
    /// Set only by [`provide`]. The element registers it so descendants can
    /// find it with [`BuildContext::inherited`].
    provided: Option<Provided>,
    /// Set only by [`notification_listener`]. The element registers it so
    /// descendants' notifications reach the callback; see
    /// [`Shared::dispatch_notification`].
    listener: Option<ListenerRegistration>,
}

#[derive(Clone)]
enum WidgetKind {
    Component(Rc<dyn ComponentObject>),
    Render(Rc<dyn RenderWidgetObject>),
}

impl AnyWidget {
    pub fn key(&self) -> Key {
        self.key
    }

    /// Whether `self` can be updated in place to become `other`, which is the
    /// question the whole reconciliation turns on.
    fn can_update(&self, other: &AnyWidget) -> bool {
        self.type_id == other.type_id && self.key == other.key
    }
}

// The object-safe shapes behind the two widget traits. StatefulComponent has
// an associated type, so it cannot be a trait object directly; these erase it.

trait ComponentObject {
    fn create_state(&self) -> Option<Box<dyn Any>>;
    /// Runs the widget's per-frame advance. Returns whether another frame is
    /// wanted.
    fn advance(&self, state: Option<&mut dyn Any>, frame_time_micros: i64) -> bool;
    /// This widget as its own concrete type, so the widget an element replaced
    /// can be handed back to it typed, in `did_update_widget`.
    fn as_any(&self) -> &dyn Any;
    /// Runs the widget's `did_update_widget`, with the widget it replaces.
    fn did_update_widget(&self, old: &dyn Any, state: Option<&mut dyn Any>);
    /// Runs the widget's `dispose`, as its element is released.
    fn dispose(&self, state: Option<&mut dyn Any>);
    fn build(
        &self,
        state: Option<&mut dyn Any>,
        element: ElementId,
        context: &mut BuildContext,
    ) -> AnyWidget;
}

trait RenderWidgetObject {
    fn children(&self) -> Vec<AnyWidget>;
    fn create_render(&self, children: Vec<BoxedRender>) -> BoxedRender;
    /// Gives `target` this widget's configuration instead of making a second
    /// render object, and says whether it took it.
    ///
    /// Upstream's `updateRenderObject`, which every `RenderObjectWidget` writes
    /// by hand: `Padding` assigns its `padding` onto the `RenderPadding` the
    /// element already has. There is no widget class here holding those fields
    /// separately -- the configuration lives inside the closure a combinator
    /// captured -- so the new values are reached the only way they can be, by
    /// building the object the closure would have built and letting the old one
    /// take what it needs from it. The comparison that decides what changed is
    /// then the render object's own, which is where upstream puts it too.
    fn reconfigure(&self, target: &BoxedRender, children: Vec<BoxedRender>) -> bool;
}

struct StatelessObject<C: Component>(C);

impl<C: Component> ComponentObject for StatelessObject<C> {
    fn create_state(&self) -> Option<Box<dyn Any>> {
        None
    }

    fn advance(&self, _state: Option<&mut dyn Any>, _frame_time_micros: i64) -> bool {
        // Nothing to advance: a stateless widget has no clock to move.
        false
    }

    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn did_update_widget(&self, _old: &dyn Any, _state: Option<&mut dyn Any>) {
        // A stateless widget has no state to tell that it changed.
    }

    fn dispose(&self, _state: Option<&mut dyn Any>) {
        // Nor any to let go of.
    }

    fn build(
        &self,
        _state: Option<&mut dyn Any>,
        _element: ElementId,
        context: &mut BuildContext,
    ) -> AnyWidget {
        self.0.build(context)
    }
}

struct StatefulObject<C: StatefulComponent>(C);

impl<C: StatefulComponent> ComponentObject for StatefulObject<C> {
    fn create_state(&self) -> Option<Box<dyn Any>> {
        Some(Box::new(self.0.initial_state()))
    }

    fn advance(&self, state: Option<&mut dyn Any>, frame_time_micros: i64) -> bool {
        match state.and_then(|s| s.downcast_mut::<C::State>()) {
            Some(state) => self.0.advance(state, frame_time_micros),
            None => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn did_update_widget(&self, old: &dyn Any, state: Option<&mut dyn Any>) {
        let (Some(old), Some(state)) = (
            old.downcast_ref::<C>(),
            state.and_then(|s| s.downcast_mut::<C::State>()),
        ) else {
            return;
        };
        self.0.did_update_widget(old, state);
    }

    fn dispose(&self, state: Option<&mut dyn Any>) {
        if let Some(state) = state.and_then(|s| s.downcast_mut::<C::State>()) {
            self.0.dispose(state);
        }
    }

    fn build(
        &self,
        state: Option<&mut dyn Any>,
        element: ElementId,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let handle = StateHandle::<C::State>::new(element, context.shared());
        match state.and_then(|s| s.downcast_mut::<C::State>()) {
            Some(state) => self.0.build(state, handle, context),
            // Only reachable if the element's state was taken and not put
            // back, which would be a bug in this file rather than in the
            // caller. Building against a default keeps the frame going.
            None => {
                let fallback = C::State::default();
                self.0.build(&fallback, handle, context)
            }
        }
    }
}

struct RenderObjectWidget<W: RenderWidget>(W);

impl<W: RenderWidget> RenderWidgetObject for RenderObjectWidget<W> {
    fn children(&self) -> Vec<AnyWidget> {
        self.0.children()
    }

    fn create_render(&self, children: Vec<BoxedRender>) -> BoxedRender {
        self.0.create_render(children)
    }

    fn reconfigure(&self, target: &BoxedRender, children: Vec<BoxedRender>) -> bool {
        target.reconfigure(self.0.create_render(children))
    }
}

/// Wraps a [`Component`] as an [`AnyWidget`].
pub fn component<C: Component>(widget: C) -> AnyWidget {
    AnyWidget {
        type_id: TypeId::of::<C>(),
        key: widget.key(),
        inner: WidgetKind::Component(Rc::new(StatelessObject(widget))),
        global_key: None,
        provided: None,
        listener: None,
    }
}

/// Wraps a [`StatefulComponent`] as an [`AnyWidget`].
pub fn stateful<C: StatefulComponent>(widget: C) -> AnyWidget {
    AnyWidget {
        type_id: TypeId::of::<C>(),
        key: widget.key(),
        inner: WidgetKind::Component(Rc::new(StatefulObject(widget))),
        global_key: None,
        provided: None,
        listener: None,
    }
}

/// Wraps a [`RenderWidget`] as an [`AnyWidget`].
pub fn render_widget<W: RenderWidget>(widget: W) -> AnyWidget {
    AnyWidget {
        type_id: TypeId::of::<W>(),
        key: widget.key(),
        inner: WidgetKind::Render(Rc::new(RenderObjectWidget(widget))),
        global_key: None,
        provided: None,
        listener: None,
    }
}

/// Upstream `KeyedSubtree`: an already-built widget given a [`Key`].
///
/// A key is normally set by the widget that has one (`Component::key`), which
/// works when the widget is yours. This is for when it is not -- a list of
/// children built by somebody else, each of which has to keep its element
/// across a reorder. Upstream's is a widget that wraps; here the key is
/// written onto the widget itself, since an `AnyWidget` carries its own.
pub fn keyed_subtree(key: u64, mut child: AnyWidget) -> AnyWidget {
    child.key = Some(key);
    child
}

/// Upstream `KeyedSubtree.ensureUniqueKeysForList`: every item keyed by its
/// own key if it has one and by its position if it does not, so that a list
/// built from unkeyed children still reorders by identity.
///
/// Upstream wraps each item in a `KeyedSubtree` whose key is
/// `ValueKey(child.key ?? index)`; a key here is a number, so the item's own
/// key is already that value and the index stands in when there is none.
pub fn ensure_unique_keys_for_list(items: Vec<AnyWidget>, base_index: u64) -> Vec<AnyWidget> {
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let key = item.key.unwrap_or(base_index + index as u64);
            keyed_subtree(key, item)
        })
        .collect()
}

/// The state a [`stateful_builder`] holds: none of its own.
///
/// Upstream's `StatefulBuilder` holds no state either -- the point of it is
/// the `setState` it hands to the builder, which belongs to an element the
/// caller did not have to declare a widget for.
#[derive(Default)]
pub struct StatefulBuilderState;

/// Upstream `StatefulBuilder`: a builder given a way to rebuild itself.
pub struct StatefulBuilder<F> {
    builder: F,
    key: Key,
}

impl<F> StatefulBuilder<F>
where
    F: Fn(StateHandle<StatefulBuilderState>) -> AnyWidget + 'static,
{
    pub fn new(builder: F) -> StatefulBuilder<F> {
        StatefulBuilder { builder, key: None }
    }

    pub fn with_key(mut self, key: u64) -> Self {
        self.key = Some(key);
        self
    }
}

impl<F> StatefulComponent for StatefulBuilder<F>
where
    F: Fn(StateHandle<StatefulBuilderState>) -> AnyWidget + 'static,
{
    type State = StatefulBuilderState;

    fn key(&self) -> Key {
        self.key
    }

    fn build(
        &self,
        _state: &Self::State,
        handle: StateHandle<Self::State>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        (self.builder)(handle)
    }
}

/// [`StatefulBuilder`] as a widget.
pub fn stateful_builder<F>(builder: F) -> AnyWidget
where
    F: Fn(StateHandle<StatefulBuilderState>) -> AnyWidget + 'static,
{
    stateful(StatefulBuilder::new(builder))
}

/// Gives an already-built widget a [`GlobalKey`].
///
/// The key is not a [`Key`]: it does not take part in
/// [`AnyWidget::can_update`], because its whole job is to match elements that
/// *cannot* be matched by position -- one parent dropped it, another wants it.
/// See [`GlobalKey`].
pub fn with_global_key(key: GlobalKey, mut widget: AnyWidget) -> AnyWidget {
    widget.global_key = Some(key);
    widget
}

// -- The three combinators ----------------------------------------------------

struct LeafWidget<F> {
    key: Key,
    build: F,
}

impl<F: Fn() -> BoxedRender + 'static> RenderWidget for LeafWidget<F> {
    fn key(&self) -> Key {
        self.key
    }
    fn children(&self) -> Vec<AnyWidget> {
        Vec::new()
    }
    fn create_render(&self, _children: Vec<BoxedRender>) -> BoxedRender {
        (self.build)()
    }
}

/// A render widget with no children.
///
/// ```ignore
/// leaf(|| Box::new(Text::new("hello").with_size(20.0)))
/// ```
pub fn leaf<F, R>(build: F) -> AnyWidget
where
    F: Fn() -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    render_widget(LeafWidget {
        key: None,
        build: move || crate::render::RenderRef::new(build()),
    })
}

/// [`leaf`] with an explicit key.
pub fn keyed_leaf<F, R>(key: u64, build: F) -> AnyWidget
where
    F: Fn() -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    render_widget(LeafWidget {
        key: Some(key),
        build: move || crate::render::RenderRef::new(build()),
    })
}

struct SingleWidget<F> {
    key: Key,
    child: RefCell<Option<AnyWidget>>,
    wrap: F,
}

impl<F, R> RenderWidget for SingleWidget<F>
where
    F: Fn(BoxedRender) -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    fn key(&self) -> Key {
        self.key
    }

    fn children(&self) -> Vec<AnyWidget> {
        // Cloned rather than taken: an element rebuilds by running this again
        // on the widget it already holds, and a taken child is gone by then.
        // See AnyWidget.
        self.child.borrow().clone().into_iter().collect()
    }

    fn create_render(&self, mut children: Vec<BoxedRender>) -> BoxedRender {
        match children.pop() {
            Some(child) => crate::render::RenderRef::new((self.wrap)(child)),
            // A child that failed to build leaves nothing to wrap. Producing an
            // empty box keeps the frame going rather than dropping the parent.
            None => crate::render::RenderRef::new(crate::widgets::Empty),
        }
    }
}

/// A render widget with one child, given a way to wrap the child's render
/// object.
///
/// ```ignore
/// single(child, |c| Box::new(RenderPadding::new(EdgeInsets::all(8.0), c)))
/// ```
pub fn single<F, R>(child: AnyWidget, wrap: F) -> AnyWidget
where
    F: Fn(BoxedRender) -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    render_widget(SingleWidget {
        key: None,
        child: RefCell::new(Some(child)),
        wrap,
    })
}

/// [`single`] with an explicit key.
pub fn keyed_single<F, R>(key: u64, child: AnyWidget, wrap: F) -> AnyWidget
where
    F: Fn(BoxedRender) -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    render_widget(SingleWidget {
        key: Some(key),
        child: RefCell::new(Some(child)),
        wrap,
    })
}

struct ManyWidget<F> {
    key: Key,
    children: RefCell<Vec<AnyWidget>>,
    assemble: F,
}

impl<F, R> RenderWidget for ManyWidget<F>
where
    F: Fn(Vec<BoxedRender>) -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    fn key(&self) -> Key {
        self.key
    }

    fn children(&self) -> Vec<AnyWidget> {
        self.children.borrow().clone()
    }

    fn create_render(&self, children: Vec<BoxedRender>) -> BoxedRender {
        crate::render::RenderRef::new((self.assemble)(children))
    }
}

/// A render widget with any number of children, given a way to assemble their
/// render objects.
///
/// ```ignore
/// many(rows, |children| {
///     let mut flex = RenderFlex::column();
///     for child in children { flex = flex.push(child); }
///     Box::new(flex)
/// })
/// ```
pub fn many<F, R>(children: Vec<AnyWidget>, assemble: F) -> AnyWidget
where
    F: Fn(Vec<BoxedRender>) -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    render_widget(ManyWidget {
        key: None,
        children: RefCell::new(children),
        assemble,
    })
}

/// [`many`] with an explicit key.
pub fn keyed_many<F, R>(key: u64, children: Vec<AnyWidget>, assemble: F) -> AnyWidget
where
    F: Fn(Vec<BoxedRender>) -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    render_widget(ManyWidget {
        key: Some(key),
        children: RefCell::new(children),
        assemble,
    })
}

// -- The build error placeholder ----------------------------------------------

/// The leaf a component whose `build` panicked leaves behind: upstream's
/// `ErrorWidget`, which paints a gray box where the subtree would have been.
///
/// The exception itself is reported to the log when it is caught; the
/// placeholder is only a marker, so it paints a flat gray box and contributes
/// no semantics -- it is decoration standing in for content, not content.
pub struct ErrorPlaceholder;

impl RenderWidget for ErrorPlaceholder {
    fn children(&self) -> Vec<AnyWidget> {
        Vec::new()
    }

    fn create_render(&self, _children: Vec<BoxedRender>) -> BoxedRender {
        crate::render::RenderRef::new(crate::render::RenderDecoratedBox::new().with_color(
            // Upstream's ErrorWidget background is `Color(0xF0900000)`, and
            // a release build shows plain gray; gray is what a marker box
            // reads as either way.
            crate::engine::Color::argb(0xF0, 0x90, 0x90, 0x90),
        ))
    }
}

/// The subtree-sized gray box a panicked build is replaced by.
///
/// The element that panicked keeps its state and its place in the tree; only
/// what it built is swapped out. The next time something marks that element
/// dirty its build runs again, for real.
pub fn error_placeholder() -> AnyWidget {
    render_widget(ErrorPlaceholder)
}

/// The message a panic carried, whether it was raised with a formatted
/// string, a plain string, or anything else.
fn panic_message(panic: &(dyn Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

// -- Provider -----------------------------------------------------------------

/// Publishes a value to everything below it.
///
/// ```ignore
/// provide(Theme::dark(), component(Page))
/// ```
///
/// A descendant reads it with [`BuildContext::inherited`]. The value is shared
/// rather than cloned, so publishing something large costs a pointer.
pub struct Provider<T: 'static> {
    value: Rc<T>,
    child: RefCell<Option<AnyWidget>>,
}

impl<T: 'static> RenderWidget for Provider<T> {
    fn children(&self) -> Vec<AnyWidget> {
        self.child.borrow().clone().into_iter().collect()
    }

    fn create_render(&self, mut children: Vec<BoxedRender>) -> BoxedRender {
        // A provider is not a box: it adds nothing to layout and simply passes
        // its child through.
        match children.pop() {
            Some(child) => child,
            None => crate::render::RenderRef::new(crate::widgets::Empty),
        }
    }
}

/// A published value, plus what the element needs in order to decide whether
/// republishing it means anything.
///
/// `same` is this port of upstream's `updateShouldNotify`. There it is a method
/// each `InheritedWidget` implements -- `MediaQuery`'s is `data != old.data` --
/// and here it is a function pointer that the generic `provide` fills in, for
/// the same reason: by the time the element tree holds the value it has
/// forgotten the type, and a value that cannot be compared cannot say whether
/// anything changed, which makes every rebuild look like a change.
///
/// `aspect_stale` is the finer question, upstream's
/// `InheritedModel.updateShouldNotifyDependent`: whether one named part of the
/// value changed, so that a reader which said it reads only that part can be
/// left alone. `None` is the plain `InheritedWidget` answer -- there are no
/// parts, every change is every reader's news -- and is what [`provide`] fills
/// in; [`provide_model`] fills in the other.
///
/// `theme_type` is upstream's `InheritedTheme`, which there is a class a widget
/// extends and here is a mark on the value, since a value is what this port
/// publishes. `Some` means the value is carried across a subtree boundary by
/// [`BuildContext::capture_themes`]; `None`, the usual answer, means it is not
/// -- a `MediaQuery` that followed a menu into the overlay would tell it the
/// size of the button.
#[derive(Clone)]
struct Provided {
    type_id: TypeId,
    value: Rc<dyn Any>,
    same: fn(&dyn Any, &dyn Any) -> bool,
    aspect_stale: Option<fn(&dyn Any, &dyn Any, &str) -> bool>,
    theme_type: Option<&'static str>,
}

/// A published value whose readers can depend on one *part* of it.
///
/// Upstream's `InheritedModel`: a reader that qualifies its dependence with an
/// aspect -- see [`BuildContext::inherited_aspect`] -- is rebuilt when the
/// value changes *and* the aspect it named is among the parts that changed.
/// [`DependentNotify::is_aspect_stale`] is the port of
/// `updateShouldNotifyDependent`, minus the set: the aspects are asked about
/// one at a time, and which aspects a reader cares about is the dependency
/// record's business, not the value's.
pub trait DependentNotify: PartialEq {
    /// Whether `aspect` of the value differs between `old` and `new`.
    fn is_aspect_stale(old: &Self, new: &Self, aspect: &str) -> bool;
}

/// The value a [`Provider`] publishes, kept out of the widget so the element
/// can register it without knowing `T`.
trait ProvidedValue {
    fn provided(&self) -> Provided;
}

impl<T: PartialEq + 'static> ProvidedValue for Provider<T> {
    fn provided(&self) -> Provided {
        Provided::of(Rc::clone(&self.value))
    }
}

impl Provided {
    /// Wraps a value with the comparison for its own type.
    fn of<T: PartialEq + 'static>(value: Rc<T>) -> Provided {
        Provided {
            type_id: TypeId::of::<T>(),
            value: value as Rc<dyn Any>,
            same: |a, b| match (a.downcast_ref::<T>(), b.downcast_ref::<T>()) {
                (Some(a), Some(b)) => a == b,
                // Different types cannot be the same value. Not reachable:
                // the type id is checked before this is called.
                _ => false,
            },
            aspect_stale: None,
            theme_type: None,
        }
    }

    /// [`Provided::of`], for a value that can also say which part of it
    /// changed. Upstream's `InheritedModel.updateShouldNotifyDependent`.
    fn of_model<T: DependentNotify + 'static>(value: Rc<T>) -> Provided {
        Provided {
            aspect_stale: Some(|a, b, aspect| {
                match (a.downcast_ref::<T>(), b.downcast_ref::<T>()) {
                    (Some(old), Some(new)) => T::is_aspect_stale(old, new, aspect),
                    // Not reachable: the type id is checked before this is called.
                    _ => true,
                }
            }),
            ..Provided::of(value)
        }
    }
}

/// Publishes `value` to `child` and everything below it.
///
/// `T` has to be comparable, because publishing the same value again must not
/// count as a change -- a provider rebuilt every frame would otherwise rebuild
/// everything that reads it every frame, which is the thing dependency
/// tracking exists to avoid.
pub fn provide<T: PartialEq + 'static>(value: T, child: AnyWidget) -> AnyWidget {
    // A `Theme` is an inherited theme wherever it is published. Upstream says
    // so once, on the `Theme` widget; this port has no `Theme` widget to say it
    // on -- an application publishes the value itself, with this function -- so
    // it is said here instead. Every other theme is a `provide_theme` call.
    let theme_type =
        (TypeId::of::<T>() == TypeId::of::<crate::components::Theme>()).then(type_name::<T>);
    published(value, child, theme_type)
}

/// [`provide`], for a value that is an **inherited theme**: one a subtree built
/// somewhere else should keep.
///
/// Upstream's `InheritedTheme`, and the same short list of things: a theme, a
/// component theme, a default text style. What it buys is
/// [`BuildContext::capture_themes`] -- a menu or a dialog put up in an overlay
/// is not below the theme that opened it, and without a capture it would be
/// drawn in whatever theme happens to be above the overlay instead.
pub fn provide_theme<T: PartialEq + 'static>(value: T, child: AnyWidget) -> AnyWidget {
    published(value, child, Some(type_name::<T>()))
}

/// [`provide`] and [`provide_theme`], which differ only in the mark.
fn published<T: PartialEq + 'static>(
    value: T,
    child: AnyWidget,
    theme_type: Option<&'static str>,
) -> AnyWidget {
    let widget = Provider {
        value: Rc::new(value),
        child: RefCell::new(Some(child)),
    };
    let provided = Provided {
        theme_type,
        ..widget.provided()
    };
    let mut any = render_widget(widget);
    any.provided = Some(provided);
    any
}

/// [`provide`], for a value whose readers can depend on part of it.
///
/// The value's type implements [`DependentNotify`], which is upstream's
/// arrangement of an `InheritedModel` where an `InheritedWidget` would do: a
/// reader that names an aspect -- see [`BuildContext::inherited_aspect`] -- is
/// rebuilt only when that part changed, and every other reader, one that read
/// the value whole, is rebuilt for any change exactly as [`provide`] would
/// have it.
pub fn provide_model<T: DependentNotify + 'static>(value: T, child: AnyWidget) -> AnyWidget {
    let widget = Provider {
        value: Rc::new(value),
        child: RefCell::new(Some(child)),
    };
    let provided = Provided::of_model(Rc::clone(&widget.value));
    let mut any = render_widget(widget);
    any.provided = Some(provided);
    any
}

/// The themes a widget was built under, ready to be put back around something
/// built somewhere else.
///
/// Upstream's `CapturedThemes`, which holds widgets where this holds the values
/// they publish, and frozen for the same reason it says twice: what is wrapped
/// sees the themes **as they were at the capture**, not as they are now. A menu
/// lives for as long as a press, so that is the whole of the difference.
///
/// Taken with [`BuildContext::capture_themes`] and spent with
/// [`ThemeCapture::wrap`]. A capture from a context with no themes above it is
/// empty, and wrapping with an empty capture returns the child unchanged.
#[derive(Clone, Default)]
pub struct ThemeCapture {
    /// Nearest theme first, which is the order upstream's `_CaptureAll` wraps
    /// in: each one goes outside the last, so the nearest ends up outermost
    /// and shadows the rest -- the arrangement the capture was taken under.
    themes: Vec<Provided>,
}

impl ThemeCapture {
    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }

    /// How many themes were captured. The types are not exposed: a caller can
    /// spend a capture, not read it.
    pub fn len(&self) -> usize {
        self.themes.len()
    }

    /// Upstream's `CapturedThemes.wrap`: rebuild the captured themes around
    /// `child`, wherever `child` is about to be built.
    pub fn wrap(&self, child: AnyWidget) -> AnyWidget {
        let mut wrapped = child;
        for provided in &self.themes {
            let mut any = render_widget(CapturedTheme {
                child: RefCell::new(Some(wrapped)),
            });
            any.provided = Some(provided.clone());
            wrapped = any;
        }
        wrapped
    }
}

/// What a [`ThemeCapture`] wraps with: a [`Provider`] whose value was published
/// by somebody else, so it can be republished without knowing its type.
struct CapturedTheme {
    child: RefCell<Option<AnyWidget>>,
}

impl RenderWidget for CapturedTheme {
    fn children(&self) -> Vec<AnyWidget> {
        self.child.borrow().clone().into_iter().collect()
    }

    fn create_render(&self, mut children: Vec<BoxedRender>) -> BoxedRender {
        match children.pop() {
            Some(child) => child,
            None => crate::render::RenderRef::new(crate::widgets::Empty),
        }
    }
}

// -- Notifications ------------------------------------------------------------

/// A value that can bubble up the element tree.
///
/// Upstream's `Notification`. A widget below dispatches one and every
/// [`notification_listener`] above it -- up to the root -- is offered it,
/// nearest first. What the listener does with it is the listener's business;
/// what the *notification* carries is a snapshot, taken where it was
/// dispatched, of whatever the kind of event it is needs to say.
///
/// Notifications are read, never written: nothing in the tree is asked to
/// change because one went by, and a listener that wants a rebuild says so the
/// way anything else does, with [`StateHandle::set_state`]. That is why
/// upstream scroll notifications are described as "primarily useful for paint
/// effects" -- they arrive between frames, which is too late for layout and
/// fine for paint.
pub trait Notification: 'static {
    /// This notification as its own concrete type.
    ///
    /// Upstream asks `notification is T` to decide whether a listener is
    /// interested; the same question here is a downcast, and this is the
    /// reference to downcast. See [`notification_listener`].
    fn as_any(&self) -> &dyn Any;
}

/// A listener as the element tree holds it: one callback, already wrapped so
/// it answers the only question the walk has.
///
/// What it answers is upstream's `NotificationListener.onNotification` return
/// value: `true` means this notification stops here, `false` means it keeps
/// bubbling. The type check is inside rather than beside it because upstream
/// puts it there too -- `_NotificationElement.onNotification` calls back only
/// when `notification is T`, and returns false otherwise.
#[derive(Clone)]
struct ListenerRegistration {
    call: Rc<dyn Fn(&dyn Notification) -> bool>,
}

/// A widget that keeps its child and listens for notifications from below it.
///
/// Upstream's `NotificationListener<T>`: a proxy widget, adding nothing to
/// layout or painting, holding one callback. The type parameter there picks
/// which notifications arrive; here the callback is generic over [`Notification`]
/// and downcasts itself, which keeps the wiring to one function and the choice
/// of what to catch inside the closure where the reaction is.
///
/// ```ignore
/// notification_listener(
///     |notification: &ScrollNotification| { /* ... */ false },
///     list,
/// )
/// ```
///
/// # Why one enum beats five subtypes
///
/// Upstream's scroll notifications are a class hierarchy, and
/// `NotificationListener<ScrollNotification>` catches all of them because `is`
/// is a subtype test. Rust has no runtime subtyping, so an exact-type match on
/// five sibling structs would catch one each and a listener for "scrolling,
/// whatever kind" -- the common case, and what the scrollbar is -- could not be
/// written. [`crate::scrolling::ScrollNotification`] is therefore one type with
/// a variant per kind: one name to listen for, the variant where upstream would
/// have put the runtimeType, and `match` where upstream puts a switch.
pub fn notification_listener<N: Notification>(
    on_notification: impl Fn(&N) -> bool + 'static,
    child: AnyWidget,
) -> AnyWidget {
    let call: Rc<dyn Fn(&dyn Notification) -> bool> = Rc::new(move |notification| {
        match notification.as_any().downcast_ref::<N>() {
            Some(notification) => on_notification(notification),
            // Not the type this listener is for. Say so, and the walk goes on
            // to the ancestors -- upstream's `notification is T` failing.
            None => false,
        }
    });
    let mut any = render_widget(ListenerWidget {
        child: RefCell::new(Some(child)),
    });
    any.listener = Some(ListenerRegistration { call });
    any
}

/// The proxy widget behind [`notification_listener`].
///
/// The registration the element needs is not on this struct: it rides the
/// [`AnyWidget`] beside it (see [`AnyWidget::listener`]), the same way a
/// [`Provider`]'s value does, so the element can take it without knowing `N`.
struct ListenerWidget {
    child: RefCell<Option<AnyWidget>>,
}

impl RenderWidget for ListenerWidget {
    fn children(&self) -> Vec<AnyWidget> {
        self.child.borrow().clone().into_iter().collect()
    }

    fn create_render(&self, mut children: Vec<BoxedRender>) -> BoxedRender {
        // A listener is not a box: it adds nothing to layout, exactly as
        // upstream's NotificationListener is a ProxyWidget whose render object
        // is its child's.
        match children.pop() {
            Some(child) => child,
            None => crate::render::RenderRef::new(crate::widgets::Empty),
        }
    }
}

/// Where to start a notification bubbling, from somewhere other than a build.
///
/// Upstream's dispatching widget keeps its `BuildContext` -- the `Scrollable`
/// calls it the `notificationContext` and dispatches through it long after the
/// build that made it -- and this is that, made once during a build and held
/// with the state that will want it. A sink whose element has since gone away
/// dispatches nothing, which is also upstream's behaviour for a defunct
/// context.
#[derive(Clone)]
pub struct NotificationSink {
    shared: Weak<Shared>,
    element: ElementRef,
}

impl NotificationSink {
    /// Starts `notification` bubbling from the element this sink was made for.
    ///
    /// Upstream's `Notification.dispatch(target)`. The notification is offered
    /// to the listener at that element (if it is one) and then to each
    /// listener above it, until one returns `true` or the root runs out.
    pub fn dispatch(&self, notification: &dyn Notification) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        if shared.generation(self.element.id) != self.element.generation {
            return;
        }
        shared.dispatch_notification(self.element.id, notification);
    }
}

// -- State --------------------------------------------------------------------

type Mutation = Box<dyn FnOnce(&mut dyn Any)>;

/// One reader of one provider, and which parts of the value it reads.
///
/// The reader's half of upstream's `InheritedElement._dependents`, with the
/// aspect set `InheritedModelElementMixin.updateDependencies` collects for each
/// dependent kept beside it. An empty `aspects` is upstream's empty set: the
/// reader did not qualify its dependence, and is rebuilt for every change.
struct Dependent {
    reader: ElementId,
    aspects: Vec<&'static str>,
}

impl Clone for Dependent {
    fn clone(&self) -> Dependent {
        Dependent {
            reader: self.reader,
            aspects: self.aspects.clone(),
        }
    }
}

/// What a [`StateHandle`] needs to reach from outside the tree.
struct Shared {
    states: RefCell<HashMap<ElementId, Box<dyn Any>>>,
    dirty: RefCell<Vec<ElementId>>,
    /// Mutations that arrived while their state was checked out for a build.
    pending: RefCell<Vec<(ElementId, Mutation)>>,
    needs_frame: Cell<bool>,
    /// Bumped when an element id is recycled, so a handle to the old occupant
    /// stops resolving.
    generations: RefCell<HashMap<ElementId, u64>>,
    /// Parent of each mounted element, so a build can walk up. Kept here rather
    /// than read off the nodes because a BuildContext must not borrow the arena
    /// it is being built inside.
    parents: RefCell<HashMap<ElementId, Option<ElementId>>>,
    /// Values a [`Provider`] has published, by the type it publishes.
    provided: RefCell<HashMap<ElementId, Provided>>,
    /// The notification listeners mounted at each element, by the element the
    /// listening widget became. Upstream keeps these as a linked list threaded
    /// through the elements themselves (`_notificationTree`), built at mount
    /// from the parent's; here the parent map this struct already keeps *is*
    /// that list, walked on demand, and this table says which elements are on
    /// it.
    listeners: RefCell<HashMap<ElementId, ListenerRegistration>>,
    /// Who reads each provider, and what each reader reads. Two maps of the
    /// same relation: the first is what a change has to rebuild, the second is
    /// what an unmounted element has to be removed from. Upstream keeps the
    /// same pair, as `InheritedElement._dependents` and
    /// `Element._dependencies`; the first also remembers *which parts* of the
    /// value each reader asked about, upstream's aspect set.
    dependents: RefCell<HashMap<ElementId, Vec<Dependent>>>,
    dependencies: RefCell<HashMap<ElementId, Vec<ElementId>>>,
}

impl Shared {
    fn new() -> Rc<Shared> {
        Rc::new(Shared {
            states: RefCell::new(HashMap::new()),
            dirty: RefCell::new(Vec::new()),
            pending: RefCell::new(Vec::new()),
            needs_frame: Cell::new(false),
            generations: RefCell::new(HashMap::new()),
            parents: RefCell::new(HashMap::new()),
            provided: RefCell::new(HashMap::new()),
            listeners: RefCell::new(HashMap::new()),
            dependents: RefCell::new(HashMap::new()),
            dependencies: RefCell::new(HashMap::new()),
        })
    }

    fn generation(&self, id: ElementId) -> u64 {
        self.generations.borrow().get(&id).copied().unwrap_or(0)
    }

    fn bump_generation(&self, id: ElementId) {
        let mut generations = self.generations.borrow_mut();
        *generations.entry(id).or_insert(0) += 1;
    }

    /// The nearest value of type `T` published at or above `start`, and which
    /// element published it.
    fn lookup(&self, start: ElementId, wanted: TypeId) -> Option<(ElementId, Rc<dyn Any>)> {
        let parents = self.parents.borrow();
        let provided = self.provided.borrow();
        let mut current = Some(start);
        while let Some(id) = current {
            if let Some(entry) = provided.get(&id) {
                if entry.type_id == wanted {
                    return Some((id, Rc::clone(&entry.value)));
                }
            }
            current = parents.get(&id).copied().flatten();
        }
        None
    }

    /// Offers `notification` to the listener at `from` and then to each
    /// listener above it, nearest first, until one says it handled the
    /// notification or the root runs out.
    ///
    /// Upstream's `_NotificationNode.dispatchNotification`, which walks the
    /// chain of `NotifiableElementMixin`s built at mount. The walk starts at
    /// `from` itself rather than its parent because upstream's chain includes
    /// the dispatching element when that element is itself a listener.
    ///
    /// The registration is cloned out before the callback runs: a callback
    /// that calls `set_state` marks elements dirty, and nothing here may still
    /// be holding the table that would have to be borrowed for that.
    fn dispatch_notification(&self, from: ElementId, notification: &dyn Notification) {
        let mut current = Some(from);
        while let Some(id) = current {
            let listener = self.listeners.borrow().get(&id).cloned();
            if let Some(listener) = listener {
                if (listener.call)(notification) {
                    return;
                }
            }
            current = self.parents.borrow().get(&id).copied().flatten();
        }
    }

    /// Records that `reader` read what `provider` publishes.
    ///
    /// Upstream's `InheritedElement.updateDependencies`, called from
    /// `dependOnInheritedWidgetOfExactType` for the same reason: reading a
    /// value is what makes a widget care about it changing, and nothing else
    /// can tell. A read without an aspect is a dependence on the whole value.
    fn depend(&self, provider: ElementId, reader: ElementId) {
        let mut dependents = self.dependents.borrow_mut();
        let readers = dependents.entry(provider).or_default();
        match readers
            .iter_mut()
            .find(|dependent| dependent.reader == reader)
        {
            // Whatever aspects this reader asked about before, it reads
            // everything now. Upstream replaces the dependent's aspect set
            // with an empty one here, which means the same thing.
            Some(dependent) => dependent.aspects.clear(),
            None => readers.push(Dependent {
                reader,
                aspects: Vec::new(),
            }),
        }
        let mut dependencies = self.dependencies.borrow_mut();
        let providers = dependencies.entry(reader).or_default();
        if !providers.contains(&provider) {
            providers.push(provider);
        }
    }

    /// [`Shared::depend`], qualified by one aspect of the value.
    ///
    /// Upstream's `InheritedModelElement.updateDependencies`: the aspect
    /// joins the set the reader is accumulating over this build -- it is
    /// rebuilt when *any* aspect it reads changes -- unless it read the value
    /// whole, in which case one part of it is not news it can afford to miss.
    fn depend_on_aspect(&self, provider: ElementId, reader: ElementId, aspect: &'static str) {
        let mut dependents = self.dependents.borrow_mut();
        let readers = dependents.entry(provider).or_default();
        match readers
            .iter_mut()
            .find(|dependent| dependent.reader == reader)
        {
            Some(dependent) => {
                if !dependent.aspects.is_empty() && !dependent.aspects.contains(&aspect) {
                    dependent.aspects.push(aspect);
                }
            }
            None => readers.push(Dependent {
                reader,
                aspects: vec![aspect],
            }),
        }
        let mut dependencies = self.dependencies.borrow_mut();
        let providers = dependencies.entry(reader).or_default();
        if !providers.contains(&provider) {
            providers.push(provider);
        }
    }

    /// Forgets everything `reader` used to read, before it reads again.
    ///
    /// A build that no longer looks at the theme must stop being rebuilt when
    /// the theme changes, and the only place that is known is here, just
    /// before the build that will register whatever it does read.
    fn clear_dependencies(&self, reader: ElementId) {
        let providers = self.dependencies.borrow_mut().remove(&reader);
        if let Some(providers) = providers {
            let mut dependents = self.dependents.borrow_mut();
            for provider in providers {
                if let Some(readers) = dependents.get_mut(&provider) {
                    readers.retain(|dependent| dependent.reader != reader);
                }
            }
        }
    }

    /// Marks everything that reads `provider` for rebuilding, asking each
    /// reader whether what *it* reads changed.
    ///
    /// Upstream's `InheritedElement.notifyClients` walking into
    /// `InheritedModelElementMixin.notifyDependent`: a reader that did not
    /// qualify its dependence, or a value that cannot compare per aspect, is
    /// marked unconditionally; a reader that named aspects is marked only if
    /// one of them is among the parts that changed.
    fn notify_dependents(&self, provider: ElementId, old: &Provided, new: &Provided) -> usize {
        let readers = self
            .dependents
            .borrow()
            .get(&provider)
            .cloned()
            .unwrap_or_default();
        let mut notified = 0;
        for dependent in &readers {
            let stale = dependent.aspects.is_empty()
                || match old.aspect_stale {
                    None => true,
                    Some(is_aspect_stale) => dependent.aspects.iter().any(|aspect| {
                        is_aspect_stale(old.value.as_ref(), new.value.as_ref(), aspect)
                    }),
                };
            if stale {
                self.mark_dirty(dependent.reader);
                notified += 1;
            }
        }
        notified
    }

    fn mark_dirty(&self, id: ElementId) {
        let mut dirty = self.dirty.borrow_mut();
        if !dirty.contains(&id) {
            dirty.push(id);
        }
        self.needs_frame.set(true);
    }
}

/// A way to change one element's state from outside a build.
///
/// Hand it to a callback -- a tap handler, a timer, a finished request -- and
/// call [`StateHandle::set_state`] when the answer arrives. The element is
/// marked dirty and a frame is requested; the rebuild happens on that frame,
/// not inside `set_state`.
pub struct StateHandle<S> {
    pub(crate) id: ElementId,
    generation: u64,
    shared: Weak<Shared>,
    marker: PhantomData<fn(&mut S)>,
}

impl<S> Clone for StateHandle<S> {
    fn clone(&self) -> Self {
        StateHandle {
            id: self.id,
            generation: self.generation,
            shared: self.shared.clone(),
            marker: PhantomData,
        }
    }
}

impl<S: 'static> StateHandle<S> {
    fn new(id: ElementId, shared: &Rc<Shared>) -> StateHandle<S> {
        StateHandle {
            id,
            generation: shared.generation(id),
            shared: Rc::downgrade(shared),
            marker: PhantomData,
        }
    }

    pub fn element(&self) -> ElementId {
        self.id
    }

    /// A handle attached to nothing, for tests and for a component that is
    /// being constructed outside a build. Every `set_state` on it returns
    /// false.
    pub fn detached() -> StateHandle<S> {
        StateHandle {
            id: ElementId(usize::MAX),
            generation: 0,
            shared: Weak::new(),
            marker: PhantomData,
        }
    }

    /// Whether this handle still points at the element it was made for. A
    /// handle held past its element's unmounting goes stale rather than
    /// dangling, and [`StateHandle::set_state`] on it does nothing.
    pub fn is_valid(&self) -> bool {
        match self.shared.upgrade() {
            Some(shared) => shared.generation(self.id) == self.generation,
            None => false,
        }
    }

    /// Mutates the state and schedules a rebuild of this element's subtree.
    ///
    /// Returns whether the mutation was accepted; `false` means the handle is
    /// stale.
    pub fn set_state(&self, mutate: impl FnOnce(&mut S) + 'static) -> bool {
        let Some(shared) = self.shared.upgrade() else {
            return false;
        };
        if shared.generation(self.id) != self.generation {
            return false;
        }

        // Apply now if the state is not checked out for a build; otherwise
        // queue it. Applying immediately is what lets a handler read back what
        // it just wrote; queueing is what keeps a set_state from inside a build
        // from being a re-entrant borrow.
        let mut mutate = Some(mutate);
        if let Ok(mut states) = shared.states.try_borrow_mut() {
            if let Some(state) = states.get_mut(&self.id).and_then(|s| s.downcast_mut::<S>()) {
                if let Some(mutate) = mutate.take() {
                    mutate(state);
                }
            }
        }

        if let Some(mutate) = mutate {
            shared.pending.borrow_mut().push((
                self.id,
                Box::new(move |any: &mut dyn Any| {
                    if let Some(state) = any.downcast_mut::<S>() {
                        mutate(state);
                    }
                }),
            ));
        }

        shared.mark_dirty(self.id);
        true
    }
}

// -- Build context ------------------------------------------------------------

/// What a widget is given while it builds.
pub struct BuildContext {
    shared: Rc<Shared>,
    element: ElementId,
    depth: usize,
    frame_time_micros: i64,
}

impl BuildContext {
    fn shared(&self) -> &Rc<Shared> {
        &self.shared
    }

    /// The element being built.
    pub fn element(&self) -> ElementId {
        self.element
    }

    /// A way to ask, later, whether this element is still the one that was
    /// built here.
    ///
    /// Not just the id: an id is a slot in an arena, and a released slot is
    /// handed to the next element that needs one. The generation is what tells
    /// "still mounted" from "somebody else lives here now".
    pub fn element_ref(&self) -> ElementRef {
        ElementRef {
            id: self.element,
            generation: self.shared.generation(self.element),
        }
    }

    /// How deep in the element tree this build is. Useful for diagnostics.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The time this frame is targeted at, in microseconds since the epoch.
    ///
    /// An animation that is a pure function of time -- a spinner, a pulse,
    /// anything that never stops -- can be computed from this during the build
    /// and needs no controller and no state at all. One that starts, stops or
    /// reverses does need a controller; see [`crate::animation`].
    pub fn frame_time_micros(&self) -> i64 {
        self.frame_time_micros
    }

    /// Asks for another frame without changing any state -- what an animation
    /// that reads the clock rather than storing a value needs.
    pub fn request_frame(&self) {
        self.shared.needs_frame.set(true);
    }

    /// The nearest value of type `T` published by a [`Provider`] above this
    /// element, or `None` if there is none.
    ///
    /// Upstream this is `dependOnInheritedWidgetOfExactType`, and the *depend*
    /// is the point: reading the value registers this element as a reader, so
    /// that publishing a different one later rebuilds this widget and not the
    /// tree around it. See [`ElementTree::publish`].
    pub fn inherited<T: 'static>(&self) -> Option<Rc<T>> {
        let (provider, value) = self.shared.lookup(self.element, TypeId::of::<T>())?;
        self.shared.depend(provider, self.element);
        value.downcast::<T>().ok()
    }

    /// [`BuildContext::inherited`], or the type's default if nothing published
    /// one. What a theme lookup wants: a page that forgot to install a theme
    /// should look plain, not fail to build.
    pub fn inherited_or_default<T: Default + 'static>(&self) -> Rc<T> {
        self.inherited::<T>()
            .unwrap_or_else(|| Rc::new(T::default()))
    }

    /// [`BuildContext::inherited`], qualified by one aspect of the value.
    ///
    /// Upstream's `dependOnInheritedWidgetOfExactType` with an `aspect`, or
    /// `InheritedModel.inheritFrom`: the reader is rebuilt when the value
    /// changes *and* the aspect it named is one of the parts that changed, so
    /// a widget that reads only the padding is not rebuilt because the view
    /// got taller. A value published with [`provide`] rather than
    /// [`provide_model`] cannot answer the aspect question, and this is then
    /// the same as [`BuildContext::inherited`]; so is a build that also reads
    /// the value unqualified.
    pub fn inherited_aspect<T: 'static>(&self, aspect: &'static str) -> Option<Rc<T>> {
        let (provider, value) = self.shared.lookup(self.element, TypeId::of::<T>())?;
        self.shared.depend_on_aspect(provider, self.element, aspect);
        value.downcast::<T>().ok()
    }

    /// [`BuildContext::inherited_aspect`], or the type's default if nothing
    /// published one.
    pub fn inherited_aspect_or_default<T: Default + 'static>(&self, aspect: &'static str) -> Rc<T> {
        self.inherited_aspect::<T>(aspect)
            .unwrap_or_else(|| Rc::new(T::default()))
    }

    /// The inherited themes above this element, frozen, so a subtree built
    /// somewhere else can be built under them.
    ///
    /// Upstream's `InheritedTheme.capture(from: context, to: ...)`, and the
    /// case it exists for is exactly the one this port has: a menu is put up in
    /// an overlay at the root, which is not below the page that opened it, so
    /// the page's theme is not above the menu. The button captures on its way
    /// past and the overlay entry wraps the menu in what it caught.
    ///
    /// The rule is [`crate::inherited::capture_themes`]: walk up, keep the
    /// values marked as themes by [`provide_theme`], and keep only the first
    /// of each type, because a nearer theme shadows a farther one and wrapping
    /// in both would put the shadowed one on the outside where it can never be
    /// read.
    ///
    /// Divergence from upstream, and it is the `to` argument: upstream stops at
    /// the navigator's context, since the themes above *that* are still above
    /// the route and do not need copying. This walks to the root, because an
    /// overlay here is a handle rather than an element and there is no context
    /// to stop at. The extra themes are the ones the wrapped subtree would have
    /// seen anyway; the cost is that a change to one of them after the capture
    /// does not reach the menu that is already open.
    pub fn capture_themes(&self) -> ThemeCapture {
        let ancestors: Vec<crate::inherited::ThemeLink> = {
            let parents = self.shared.parents.borrow();
            let provided = self.shared.provided.borrow();
            let mut links = Vec::new();
            let mut current = Some(self.element);
            while let Some(id) = current {
                links.push(match provided.get(&id).and_then(|entry| entry.theme_type) {
                    Some(theme_type) => crate::inherited::ThemeLink::theme(id.0 as u64, theme_type),
                    None => crate::inherited::ThemeLink::plain(id.0 as u64),
                });
                current = parents.get(&id).copied().flatten();
            }
            links
        };
        // `to: None` -- the walk reaches the root, so this cannot be the
        // "`to` is not an ancestor" error.
        let captured = crate::inherited::capture_themes(&ancestors, self.element.0 as u64, None)
            .expect("a walk to the root always reaches its end");
        let provided = self.shared.provided.borrow();
        ThemeCapture {
            themes: captured
                .themes()
                .iter()
                .filter_map(|link| provided.get(&ElementId(link.element as usize)).cloned())
                .collect(),
        }
    }

    /// Starts `notification` bubbling up from this element.
    ///
    /// Upstream's `BuildContext.dispatchNotification`. The notification is
    /// offered to every [`notification_listener`] above this element, nearest
    /// first, until one returns `true` from its callback.
    pub fn dispatch_notification(&self, notification: &dyn Notification) {
        self.shared
            .dispatch_notification(self.element, notification);
    }

    /// A way to dispatch notifications from this element after the build.
    ///
    /// Some dispatchers are not widgets that build -- the scroll position
    /// logic here, upstream's `Scrollable` -- but state that outlives the
    /// build. Those hold one of these instead, which is upstream's arrangement
    /// exactly: the `Scrollable` keeps its context as the `notificationContext`
    /// and dispatches through it from wherever the scrolling ends up happening.
    pub fn notification_sink(&self) -> NotificationSink {
        NotificationSink {
            shared: Rc::downgrade(&self.shared),
            element: self.element_ref(),
        }
    }
}

// -- Elements -----------------------------------------------------------------

struct ElementNode {
    widget: AnyWidget,
    children: Vec<ElementId>,
    parent: Option<ElementId>,
    depth: usize,
    /// The render object this element owns, kept across frames.
    ///
    /// Upstream's `RenderObjectElement._renderObject`, and the reason it is
    /// here rather than only inside its parent is the same: an element that
    /// did not rebuild has nothing new to say, and the object it made last
    /// time is still the right one -- with its measurements, its shaped text
    /// and its scroll extent still in it.
    ///
    /// A component has none. Upstream says so with a separate element class;
    /// here it is simply never filled in, because a component's render object
    /// is its child's.
    render: Option<BoxedRender>,
    /// Whether the widget has changed since `render` was made.
    ///
    /// Set by `mount` and `update` -- that is, exactly when this element was
    /// given a new widget. A widget that was not replaced describes the same
    /// render object it described last frame.
    render_dirty: bool,
    /// Whether this element was dropped by its parent this frame but is being
    /// kept for a [`GlobalKey`] to claim, upstream's *inactive* lifecycle
    /// state. State, render object and children survive; the tree walks skip
    /// it; and if nothing claims the key before the frame ends,
    /// [`ElementTree::rebuild`] releases it for real.
    inactive: bool,
}

/// A reference to a particular element, one that stops being true when that
/// element goes away.
///
/// The generation is the whole point: element ids are arena slots and are
/// re-used, so an id on its own cannot tell "still there" from "replaced".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElementRef {
    id: ElementId,
    generation: u64,
}

/// The persistent tree between widgets and render objects.
///
/// One per view. [`ElementTree::rebuild`] takes the root widget for this frame
/// and reconciles it against what is already mounted;
/// [`ElementTree::build_render_tree`] then produces the render objects.
pub struct ElementTree {
    nodes: Vec<Option<ElementNode>>,
    free: Vec<usize>,
    root: Option<ElementId>,
    shared: Rc<Shared>,
    /// What every build this frame is told the time is.
    frame_time_micros: i64,
    /// Elements rebuilt during the last pass. Diagnostic, and what the tests
    /// assert on to show that a rebuild was partial.
    last_rebuilt: Vec<ElementId>,
    /// Which element each [`GlobalKey`] currently names, mounted or parked.
    ///
    /// Upstream's `BuildOwner._globalKeyRegistry`: written when a widget with
    /// a global key mounts, cleared when its element is released, and read by
    /// [`ElementTree::mount`] to claim an element another parent dropped this
    /// frame. It lives on the tree rather than on [`Shared`] because nothing
    /// outside the reconciliation reaches for it -- the read access is
    /// [`ElementTree::current_element`].
    global_keys: HashMap<GlobalKey, ElementId>,
    /// Roots of subtrees dropped this frame that a [`GlobalKey`] could still
    /// claim, released at the end of the rebuild if none does.
    ///
    /// Upstream's `BuildOwner._inactiveElements`, a list of roots: the
    /// descendants are inactive along with their root but are not listed
    /// separately, and `_unmountAll` walks each root down.
    inactive: Vec<ElementId>,
}

impl ElementTree {
    pub fn new() -> ElementTree {
        ElementTree {
            nodes: Vec::new(),
            free: Vec::new(),
            root: None,
            shared: Shared::new(),
            frame_time_micros: 0,
            last_rebuilt: Vec::new(),
            global_keys: HashMap::new(),
            inactive: Vec::new(),
        }
    }

    /// How many elements are mounted. A rebuild that reuses everything leaves
    /// this unchanged.
    ///
    /// An element parked for a [`GlobalKey`] to claim does not count: it is
    /// not in the tree, it is waiting to be.
    pub fn len(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| n.as_ref().is_some_and(|n| !n.inactive))
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Sets the clock every build this frame reads. Called by the host once
    /// per frame, before any rebuilding.
    pub fn set_frame_time(&mut self, frame_time_micros: i64) {
        self.frame_time_micros = frame_time_micros;
    }

    /// Runs every mounted widget's [`StatefulComponent::advance`], shallowest
    /// first, and returns whether any of them wants another frame.
    ///
    /// Called once per frame before building. Advancing before building rather
    /// than during it is what lets `build` take the state by shared reference:
    /// by the time anything is drawn, the clock has already moved.
    pub fn advance_frame(&mut self, frame_time_micros: i64) -> bool {
        self.frame_time_micros = frame_time_micros;

        // Collect first: advancing may set_state, which touches the same maps
        // this walk would otherwise be holding. An element parked for a
        // GlobalKey to claim does not advance: upstream's clock stops for the
        // inactive, and restarts if the element is reactivated.
        let ids: Vec<ElementId> = (0..self.nodes.len())
            .filter(|index| self.nodes[*index].as_ref().is_some_and(|n| !n.inactive))
            .map(ElementId)
            .collect();

        let mut wants_frame = false;
        for id in ids {
            let Some(node) = self.nodes[id.0].as_ref() else {
                continue;
            };
            let WidgetKind::Component(_) = &node.widget.inner else {
                continue;
            };

            // Check the state out for the duration, exactly as a build does, so
            // a set_state from inside advance queues instead of aliasing.
            let mut state = self.shared.states.borrow_mut().remove(&id);
            if state.is_none() {
                continue;
            }
            let advanced = {
                let node = self.nodes[id.0].as_ref().expect("checked above");
                let WidgetKind::Component(component) = &node.widget.inner else {
                    unreachable!("filtered above");
                };
                component.advance(state.as_deref_mut(), frame_time_micros)
            };
            if let Some(state) = state {
                self.shared.states.borrow_mut().insert(id, state);
            }
            if advanced {
                // Advancing changed something time-dependent, so what was built
                // from it is stale. Without this the clock moves and the screen
                // does not, which looks exactly like an animation that never
                // started.
                self.shared.mark_dirty(id);
            }
            wants_frame |= advanced;
        }
        wants_frame
    }

    /// Whether anything has asked for a frame since this was last cleared.
    pub fn needs_frame(&self) -> bool {
        self.shared.needs_frame.get()
    }

    pub fn clear_needs_frame(&self) {
        self.shared.needs_frame.set(false);
    }

    /// The elements rebuilt by the last [`ElementTree::rebuild`] or
    /// [`ElementTree::rebuild_dirty`].
    pub fn last_rebuilt(&self) -> &[ElementId] {
        &self.last_rebuilt
    }

    /// The root element, if anything is mounted.
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    /// An element's children, in build order.
    pub fn children_of(&self, id: ElementId) -> Vec<ElementId> {
        self.nodes[id.0]
            .as_ref()
            .map(|node| node.children.clone())
            .unwrap_or_default()
    }

    /// Whether an element is still in the tree.
    ///
    /// An element parked for a [`GlobalKey`] to claim is not: it left its
    /// parent and nothing reaches it until a claim puts it back.
    pub fn is_mounted(&self, id: ElementId) -> bool {
        self.nodes
            .get(id.0)
            .is_some_and(|slot| slot.as_ref().is_some_and(|n| !n.inactive))
    }

    /// Whether the element a [`ElementRef`] names is still that element.
    pub fn is_live(&self, element: ElementRef) -> bool {
        self.is_mounted(element.id) && self.shared.generation(element.id) == element.generation
    }

    /// Reads an element's state, for tests and diagnostics.
    pub fn state<S: 'static, R>(&self, id: ElementId, read: impl FnOnce(&S) -> R) -> Option<R> {
        let states = self.shared.states.borrow();
        states.get(&id)?.downcast_ref::<S>().map(read)
    }

    /// The element this [`GlobalKey`] currently names, mounted or parked.
    ///
    /// Upstream's `GlobalKey.currentElement`, reached the only way this
    /// engine can reach it: upstream's registry hangs off the one global
    /// `BuildOwner`, and this port has one [`ElementTree`] per owner instead,
    /// so the question is asked of the tree that mounted the key. A key
    /// claimed but not yet released still names its element -- an element
    /// parked for a claim is gone only once the frame ends without one.
    pub fn current_element(&self, key: &GlobalKey) -> Option<ElementId> {
        let id = self.global_keys.get(key).copied()?;
        self.nodes
            .get(id.0)
            .is_some_and(|slot| slot.is_some())
            .then_some(id)
    }

    /// Reads the state of the element this [`GlobalKey`] names.
    ///
    /// Upstream's `GlobalKey.currentState`: `None` when no element holds the
    /// key, or when it does and its state is not an `S` -- the same two ways
    /// upstream returns null. Read-only, like [`ElementTree::state`]: writing
    /// is what a [`StateHandle`] is for, and the handle the widget's own build
    /// was given is the one to keep.
    pub fn current_state<S: 'static, R>(
        &self,
        key: &GlobalKey,
        read: impl FnOnce(&S) -> R,
    ) -> Option<R> {
        let id = self.current_element(key)?;
        let states = self.shared.states.borrow();
        states.get(&id)?.downcast_ref::<S>().map(read)
    }

    fn allocate(&mut self, node: ElementNode) -> ElementId {
        match self.free.pop() {
            Some(index) => {
                self.nodes[index] = Some(node);
                let id = ElementId(index);
                // The slot had a previous occupant, so any handle to it must
                // stop resolving.
                self.shared.bump_generation(id);
                id
            }
            None => {
                self.nodes.push(Some(node));
                ElementId(self.nodes.len() - 1)
            }
        }
    }

    fn release(&mut self, id: ElementId) {
        // Upstream's `Element.unmount` -> `BuildOwner._unregisterGlobalKey`:
        // the key stops naming this element, unless it already names a newer
        // one -- which is the same identity check upstream makes.
        if let Some(global_key) = self.nodes[id.0].as_ref().and_then(|n| n.widget.global_key) {
            if self.global_keys.get(&global_key) == Some(&id) {
                self.global_keys.remove(&global_key);
            }
        }
        if let Some(node) = self.nodes[id.0].take() {
            // The widget says goodbye to its state before either is dropped --
            // upstream's `State.dispose`, called from `Element.unmount`. It
            // runs before the children are released so that a parent still
            // sees the tree it is letting go of, which is the order upstream
            // uses too.
            if let WidgetKind::Component(component) = &node.widget.inner {
                let mut states = self.shared.states.borrow_mut();
                let state = states.get_mut(&id).map(|state| &mut **state);
                component.dispose(state);
            }
            for child in node.children {
                self.release(child);
            }
        }
        // A handle to the element that was here is pointing at nothing now.
        // Without the bump it stayed "valid" until the slot happened to be
        // reused, and a caller asking `is_valid` -- a drawer's controls asking
        // whether it is still attached -- got the answer of an element that
        // no longer exists.
        self.shared.bump_generation(id);
        self.shared.states.borrow_mut().remove(&id);
        self.shared.parents.borrow_mut().remove(&id);
        self.shared.provided.borrow_mut().remove(&id);
        self.shared.listeners.borrow_mut().remove(&id);
        self.shared.clear_dependencies(id);
        self.shared.dependents.borrow_mut().remove(&id);
        self.shared.dirty.borrow_mut().retain(|d| *d != id);
        self.shared.pending.borrow_mut().retain(|(d, _)| *d != id);
        self.free.push(id.0);
    }

    /// Drops a child the way the reconciliation drops children: for keeps,
    /// unless a [`GlobalKey`] could still claim it.
    ///
    /// Upstream's `Element.deactivateChild`, which never releases anything
    /// outright -- the child goes to the owner's inactive list and stays
    /// there until the frame ends. Releasing immediately is this tree's
    /// equivalent for the child nobody can name; parking is the same half
    /// frame of grace upstream gives every dropped child, and only a global
    /// key can use it.
    fn deactivate_child(&mut self, id: ElementId) {
        if self.has_global_key_in_subtree(id) {
            self.deactivate_subtree(id);
        } else {
            self.release(id);
        }
    }

    /// Whether any element at or below `id` carries a [`GlobalKey`]. Only
    /// then is a dropped subtree claimable, so only then is parking it worth
    /// the frame's end.
    fn has_global_key_in_subtree(&self, id: ElementId) -> bool {
        match self.nodes[id.0].as_ref() {
            Some(node) => {
                node.widget.global_key.is_some()
                    || node
                        .children
                        .iter()
                        .any(|child| self.has_global_key_in_subtree(*child))
            }
            None => false,
        }
    }

    /// Parks a dropped subtree: off the tree, out of the walks, state and
    /// render objects and inner structure untouched.
    ///
    /// This is upstream's deactivate, from `deactivateChild`'s
    /// `_inactiveElements.add` down through `_deactivateRecursively`: the
    /// root loses its parent, every element in the subtree stops being a
    /// reader or a host of published values and notifications and is taken
    /// out of the build queue, and everything that makes the subtree *this*
    /// subtree -- the state, the render objects with what they measured, the
    /// children -- is kept for a claim to put back. `unmount`, the real
    /// release, happens at [`ElementTree::finalize_inactive`] if no claim
    /// comes.
    fn deactivate_subtree(&mut self, id: ElementId) {
        let Some(node) = self.nodes[id.0].as_mut() else {
            return;
        };
        if node.inactive {
            // Already parked under an earlier drop this frame; upstream's
            // inactive list holds each element once for the same reason.
            return;
        }
        node.inactive = true;
        // Only the root loses its parent. The descendants keep theirs -- the
        // claim of a deeper keyed element detaches it from *its* parent
        // inside this subtree, which only works if that link is still there.
        node.parent = None;
        let children = node.children.clone();
        self.inactive.push(id);
        self.detach_from_shared_maps(id);
        for child in children {
            self.deactivate_descendant(child);
        }
    }

    fn deactivate_descendant(&mut self, id: ElementId) {
        let Some(node) = self.nodes[id.0].as_mut() else {
            return;
        };
        if node.inactive {
            return;
        }
        node.inactive = true;
        let children = node.children.clone();
        self.detach_from_shared_maps(id);
        for child in children {
            self.deactivate_descendant(child);
        }
    }

    /// The half of deactivation that takes an element out of every walk:
    /// no longer a reader, no longer a host, no longer scheduled.
    fn detach_from_shared_maps(&self, id: ElementId) {
        self.shared.parents.borrow_mut().remove(&id);
        self.shared.provided.borrow_mut().remove(&id);
        self.shared.listeners.borrow_mut().remove(&id);
        self.shared.clear_dependencies(id);
        self.shared.dependents.borrow_mut().remove(&id);
        self.shared.dirty.borrow_mut().retain(|d| *d != id);
        self.shared.pending.borrow_mut().retain(|(d, _)| *d != id);
    }

    /// Releases whatever the frame parked and no [`GlobalKey`] claimed.
    ///
    /// Upstream's `BuildOwner.finalizeTree` -> `_InactiveElements._unmountAll`:
    /// the last thing a frame does is unmount the inactive, which is why an
    /// element can only spend half a frame off the tree. Called at the end of
    /// every rebuild, so a claim has to happen inside the same rebuild that
    /// dropped the element -- exactly upstream's "same animation frame".
    fn finalize_inactive(&mut self) {
        let parked = std::mem::take(&mut self.inactive);
        for id in parked {
            if self.nodes[id.0].as_ref().is_some_and(|n| n.inactive) {
                self.release(id);
            }
        }
    }

    /// Claims the element `key` names, for a widget about to mount.
    ///
    /// Upstream's `Element._retakeInactiveElement`: the registry is asked
    /// first, before a new element is ever made. Three answers come back.
    ///
    /// * The named element is parked, and the widget could update it -- the
    ///   claim. The element is reactivated in place; see [`Self::attach`].
    /// * The named element is still attached, to a *different* parent, and
    ///   the widget could update it -- taken anyway, with the old parent
    ///   left to notice the child is gone. Upstream's comment on this branch
    ///   is that the element's inactivity is "forward-looking": the old
    ///   parent has not reconciled yet, but this frame it will, and when it
    ///   does the child will not be in its list.
    /// * Anything else -- the key is already spoken for by a live element
    ///   that cannot be updated into this widget. That is two widgets using
    ///   one key, and it is an error, exactly as upstream's asserts are.
    ///
    /// `None` means no claim: mount a fresh element. The one parked-but-
    /// unsuitable case -- a different widget type under the same key -- is
    /// upstream's silent answer too: the new element takes the registry and
    /// the old one is released with the rest of the inactive.
    fn claim_global_key(
        &mut self,
        key: GlobalKey,
        widget: &AnyWidget,
        parent: Option<ElementId>,
    ) -> Option<ElementId> {
        let &existing = self.global_keys.get(&key)?;
        let (live, can_update, old_parent) = match self.nodes[existing.0].as_ref() {
            Some(node) => (!node.inactive, node.widget.can_update(widget), node.parent),
            None => return None,
        };
        if live && (old_parent == parent || !can_update) {
            panic!(
                "Multiple widgets used the same GlobalKey ({key:?}). \
                 A GlobalKey can only be specified on one widget at a time \
                 in the widget tree."
            );
        }
        if !can_update {
            return None;
        }
        // Detach from the parent it has now, live or parked: a live parent's
        // child list loses it here -- upstream's `forgetChild` -- so the
        // parent's own reconciliation later this frame does not find it and
        // drop it a second time. A parked one keeps its inner links, so a
        // deeper claim stays possible; this element just leaves them.
        if let Some(old) = old_parent {
            if let Some(node) = self.nodes[old.0].as_mut() {
                node.children.retain(|child| *child != existing);
            }
        }
        self.inactive.retain(|id| *id != existing);
        self.attach(existing, parent);
        Some(existing)
    }

    /// Reactivates a claimed subtree under a new parent.
    ///
    /// Upstream's `Element._activateWithParent` with `_activateRecursively`
    /// and `_updateDepth`: parent links are rewritten top down, depths
    /// follow, and every element in the subtree is active again. What it does
    /// *not* do is build -- that is the caller handing the element the new
    /// widget, the same `update` an in-place match gets, with the same
    /// state kept and the same `did_update_widget` told.
    fn attach(&mut self, id: ElementId, parent: Option<ElementId>) {
        let depth = parent
            .and_then(|parent| self.nodes[parent.0].as_ref().map(|node| node.depth))
            .map_or(0, |depth| depth + 1);
        self.attach_at_depth(id, parent, depth);
    }

    fn attach_at_depth(&mut self, id: ElementId, parent: Option<ElementId>, depth: usize) {
        let Some(node) = self.nodes[id.0].as_mut() else {
            return;
        };
        node.inactive = false;
        node.parent = parent;
        node.depth = depth;
        let children = node.children.clone();
        self.shared.parents.borrow_mut().insert(id, parent);
        for child in children {
            self.attach_at_depth(child, Some(id), depth + 1);
        }
    }

    /// Reconciles `widget` against the mounted tree, mounting it if nothing is
    /// there yet.
    pub fn rebuild(&mut self, widget: AnyWidget) {
        self.last_rebuilt.clear();
        self.drain_pending();
        // Clear before the work, not after: a full rebuild subsumes everything
        // that was already pending, but a set_state raised *during* it is a
        // request for the next frame and must survive.
        self.shared.dirty.borrow_mut().clear();
        match self.root {
            Some(root) => {
                let root = self.update(root, widget, None, 0);
                self.root = Some(root);
            }
            None => {
                let root = self.mount(widget, None, 0);
                self.root = Some(root);
            }
        }
        // The frame's last act, upstream's finalizeTree: whatever was parked
        // for a GlobalKey that never claimed it is released for real.
        self.finalize_inactive();
    }

    /// Rebuilds only the elements marked dirty by [`StateHandle::set_state`].
    ///
    /// Returns how many subtrees were rebuilt. This is the whole point of the
    /// element tree: a counter that changes rebuilds the counter, not the page
    /// around it.
    pub fn rebuild_dirty(&mut self) -> usize {
        self.last_rebuilt.clear();
        self.drain_pending();

        let mut dirty: Vec<ElementId> = self.shared.dirty.borrow_mut().drain(..).collect();
        if dirty.is_empty() {
            return 0;
        }
        // Shallowest first, so rebuilding an ancestor subsumes its descendants
        // instead of doing the work twice.
        dirty.sort_by_key(|id| self.nodes[id.0].as_ref().map_or(usize::MAX, |n| n.depth));

        let mut rebuilt = 0;
        let mut done: Vec<ElementId> = Vec::new();
        for id in dirty {
            let Some(node) = self.nodes[id.0].as_ref() else {
                continue;
            };
            if node.inactive {
                // Parked for a GlobalKey that has not claimed it; it has
                // nothing to contribute until one does.
                continue;
            }
            if done.iter().any(|ancestor| self.is_ancestor(*ancestor, id)) {
                continue;
            }
            self.rebuild_component(id);
            done.push(id);
            rebuilt += 1;
        }
        // A partial rebuild is still a frame: upstream's finalizeTree runs
        // after every build pass, not only after the full ones.
        self.finalize_inactive();
        rebuilt
    }

    /// Republishes a provided value, rebuilding only what reads it.
    ///
    /// Returns whether anything changed. This is the half of inherited widgets
    /// that a full rebuild cannot express: the view's metrics change many
    /// times a second while a keyboard opens, and the answer to that should be
    /// rebuilding the two widgets that asked about the padding, not the page.
    /// A reader that qualified its dependence with
    /// [`BuildContext::inherited_aspect`] counts as reading only the part it
    /// named, and is rebuilt only when that part is among what changed.
    ///
    /// Upstream reaches the same place along a different road -- the widget
    /// above is rebuilt with a new value and its child is the *same widget
    /// object*, which stops the reconciliation there and leaves
    /// `notifyClients` to mark the dependents. Widgets here are closures and
    /// cannot be compared, so the value is replaced on the element instead of
    /// being carried down to it. What is published is the element's, not the
    /// widget's: the next full rebuild will publish whatever the widget says
    /// again.
    pub fn publish<T: PartialEq + 'static>(&mut self, value: T) -> bool {
        let wanted = TypeId::of::<T>();
        // Shallowest first, so the root's theme wins over one published deeper
        // in for a subtree -- and there is no ambiguity about which is meant.
        let target = (0..self.nodes.len())
            .map(ElementId)
            .filter(|id| self.nodes[id.0].is_some())
            .filter(|id| {
                self.shared
                    .provided
                    .borrow()
                    .get(id)
                    .is_some_and(|entry| entry.type_id == wanted)
            })
            .min_by_key(|id| self.nodes[id.0].as_ref().map_or(usize::MAX, |n| n.depth));
        let Some(target) = target else { return false };

        // The old value is needed after the swap: whether a reader is told is
        // decided by comparing the old value and the new, so both have to
        // exist while the question is being asked. The comparison itself --
        // whole, and per aspect -- is carried over from the entry being
        // replaced rather than rebuilt from `T`, because it belongs to the
        // provider widget, not to each value it is given; upstream's is on the
        // widget too (`updateShouldNotify`, `updateShouldNotifyDependent`).
        let value = Rc::new(value);
        let (old, new) = {
            let mut provided = self.shared.provided.borrow_mut();
            let Some(current) = provided.get(&target) else {
                return false;
            };
            let replacement = Provided {
                type_id: current.type_id,
                value: Rc::clone(&value) as Rc<dyn Any>,
                same: current.same,
                aspect_stale: current.aspect_stale,
                // Carried over for the same reason as the comparisons: being a
                // theme is the provider's property, not the value's.
                theme_type: current.theme_type,
            };
            if (current.same)(current.value.as_ref(), replacement.value.as_ref()) {
                return false;
            }
            let old = current.clone();
            provided.insert(target, replacement.clone());
            (old, replacement)
        };
        self.shared.notify_dependents(target, &old, &new);
        true
    }

    /// How many elements read what `provider` publishes. For tests: the whole
    /// point of the dependency map is that this is smaller than the tree.
    pub fn dependent_count(&self, provider: ElementId) -> usize {
        self.shared
            .dependents
            .borrow()
            .get(&provider)
            .map_or(0, |readers| readers.len())
    }

    fn is_ancestor(&self, ancestor: ElementId, descendant: ElementId) -> bool {
        let mut current = self.nodes[descendant.0].as_ref().and_then(|n| n.parent);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.nodes[id.0].as_ref().and_then(|n| n.parent);
        }
        false
    }

    fn drain_pending(&mut self) {
        let pending: Vec<(ElementId, Mutation)> =
            self.shared.pending.borrow_mut().drain(..).collect();
        for (id, mutate) in pending {
            let mut states = self.shared.states.borrow_mut();
            if let Some(state) = states.get_mut(&id) {
                mutate(state.as_mut());
            }
        }
    }

    /// Re-runs one component element's build and reconciles its child.
    fn rebuild_component(&mut self, id: ElementId) {
        let Some(node) = self.nodes[id.0].as_ref() else {
            return;
        };
        let depth = node.depth;
        let WidgetKind::Component(_) = &node.widget.inner else {
            // A render element has nothing of its own to rebuild; its children
            // come from the widget that produced it.
            return;
        };

        let built = self.build_component(id, depth);
        let old_child = self.nodes[id.0]
            .as_ref()
            .and_then(|n| n.children.first().copied());
        let new_child = match old_child {
            Some(child) => self.update(child, built, Some(id), depth + 1),
            None => self.mount(built, Some(id), depth + 1),
        };
        if let Some(node) = self.nodes[id.0].as_mut() {
            node.children = vec![new_child];
        }
        self.last_rebuilt.push(id);
    }

    /// Runs a component's `build`, checking its state out for the duration so
    /// a `set_state` from inside it queues instead of aliasing.
    ///
    /// A build that panics does not take the frame with it. Upstream catches
    /// the exception in `ComponentElement.performRebuild`, builds an
    /// `ErrorWidget` in its place, and lets the frame finish -- and that is
    /// what happens here: the panic is caught around the build call, what it
    /// built is replaced by [`error_placeholder`], and the element is left
    /// clean, so it is not retried until something marks it dirty again. The
    /// element's state survives the panic, so the retry builds against what
    /// the failed build had.
    fn build_component(&mut self, id: ElementId, depth: usize) -> AnyWidget {
        // Whatever it read last time is forgotten now; this build registers
        // what it reads this time.
        self.shared.clear_dependencies(id);
        let mut state = self.shared.states.borrow_mut().remove(&id);
        let mut context = BuildContext {
            shared: Rc::clone(&self.shared),
            element: id,
            depth,
            frame_time_micros: self.frame_time_micros,
        };

        let built = {
            let node = self.nodes[id.0]
                .as_ref()
                .expect("element vanished mid-build");
            let WidgetKind::Component(component) = &node.widget.inner else {
                unreachable!("build_component on a render element");
            };
            // The closure reaches the checked-out state and the build context
            // through trait objects, so it is not UnwindSafe by inspection --
            // but everything it can touch belongs to this frame. The state is
            // put back into the map below whether the build returned or
            // panicked; the context is dropped here; and any RefCell the
            // build held was released by unwinding before `catch_unwind`
            // returned, so the shared maps are consistent at the catch. The
            // panic hook is untouched: it reports the panic while it unwinds,
            // and this only decides what the frame shows for it.
            match catch_unwind(AssertUnwindSafe(|| {
                component.build(state.as_deref_mut(), id, &mut context)
            })) {
                Ok(built) => built,
                Err(panic) => {
                    // Upstream reports through FlutterError.dumpErrorToConsole;
                    // the framework has no error sink yet, so the payload goes
                    // to the log the way app.rs reports its own diagnostics.
                    eprintln!(
                        "rustflutter: build of element {id:?} panicked: {}",
                        panic_message(panic.as_ref())
                    );
                    error_placeholder()
                }
            }
        };

        if let Some(state) = state {
            self.shared.states.borrow_mut().insert(id, state);
        }
        built
    }

    fn mount(&mut self, widget: AnyWidget, parent: Option<ElementId>, depth: usize) -> ElementId {
        // Upstream's `Element.inflateWidget`: before a new element is ever
        // made, a global key is asked whether an old one should be had
        // instead -- dropped by another parent this frame, or still attached
        // to one that has not reconciled yet. A claim returns the element
        // reactivated and handed the new widget, state and all; the `update`
        // an in-place match gets.
        if let Some(global_key) = widget.global_key {
            if let Some(existing) = self.claim_global_key(global_key, &widget, parent) {
                return self.update(existing, widget, parent, depth);
            }
        }
        let is_component = matches!(widget.inner, WidgetKind::Component(_));
        let state = match &widget.inner {
            WidgetKind::Component(component) => component.create_state(),
            WidgetKind::Render(_) => None,
        };
        let children_widgets = match &widget.inner {
            WidgetKind::Component(_) => Vec::new(),
            WidgetKind::Render(render) => render.children(),
        };

        let provided = widget.provided.clone();
        let listener = widget.listener.clone();
        let global_key = widget.global_key;
        let id = self.allocate(ElementNode {
            widget,
            children: Vec::new(),
            parent,
            depth,
            render: None,
            render_dirty: true,
            inactive: false,
        });
        if let Some(state) = state {
            self.shared.states.borrow_mut().insert(id, state);
        }
        self.shared.parents.borrow_mut().insert(id, parent);
        // Upstream's `Element.mount` -> `BuildOwner._registerGlobalKey`.
        if let Some(global_key) = global_key {
            self.global_keys.insert(global_key, id);
        }
        match provided {
            Some(provided) => {
                self.shared.provided.borrow_mut().insert(id, provided);
            }
            None => {
                self.shared.provided.borrow_mut().remove(&id);
            }
        }
        match listener {
            Some(listener) => {
                self.shared.listeners.borrow_mut().insert(id, listener);
            }
            None => {
                self.shared.listeners.borrow_mut().remove(&id);
            }
        }

        let children = if is_component {
            let built = self.build_component(id, depth);
            self.last_rebuilt.push(id);
            vec![self.mount(built, Some(id), depth + 1)]
        } else {
            children_widgets
                .into_iter()
                .map(|child| self.mount(child, Some(id), depth + 1))
                .collect()
        };

        if let Some(node) = self.nodes[id.0].as_mut() {
            node.children = children;
        }
        id
    }

    /// Updates `id` to `widget` if they match, otherwise replaces the subtree.
    /// Returns whichever element now occupies the slot.
    fn update(
        &mut self,
        id: ElementId,
        widget: AnyWidget,
        parent: Option<ElementId>,
        depth: usize,
    ) -> ElementId {
        // A child a GlobalKey claim took from this parent mid-walk is not
        // this parent's to update or to drop -- upstream's `forgottenChildren`:
        // the walk acts as if the child had never been in the list, and the
        // new widget mounts without touching the moved element.
        if !self.nodes[id.0]
            .as_ref()
            .is_some_and(|node| node.parent == parent)
        {
            return self.mount(widget, parent, depth);
        }
        let matches = self.nodes[id.0]
            .as_ref()
            .is_some_and(|node| node.widget.can_update(&widget));
        if !matches {
            self.deactivate_child(id);
            return self.mount(widget, parent, depth);
        }

        let is_component = matches!(widget.inner, WidgetKind::Component(_));
        let children_widgets = match &widget.inner {
            WidgetKind::Component(_) => Vec::new(),
            WidgetKind::Render(render) => render.children(),
        };

        let provided = widget.provided.clone();
        let listener = widget.listener.clone();
        // The widget being replaced is kept on its way out, so it can be
        // handed to `did_update_widget` below; upstream passes it to the state
        // the same way, in `StatefulElement.update`.
        let old_widget = self.nodes[id.0].as_mut().map(|node| {
            let old = std::mem::replace(&mut node.widget, widget);
            node.depth = depth;
            node.parent = parent;
            // A new widget describes the render object differently, so the
            // object has to be asked to take the difference. Upstream does that
            // here, in `RenderObjectElement.update`; it happens on the render
            // walk instead, because the children's objects are arguments to
            // this one's and they are not built until then.
            node.render_dirty = true;
            old
        });
        self.shared.parents.borrow_mut().insert(id, parent);
        match provided {
            Some(provided) => {
                self.shared.provided.borrow_mut().insert(id, provided);
            }
            None => {
                self.shared.provided.borrow_mut().remove(&id);
            }
        }
        match listener {
            Some(listener) => {
                self.shared.listeners.borrow_mut().insert(id, listener);
            }
            None => {
                self.shared.listeners.borrow_mut().remove(&id);
            }
        }

        if is_component {
            if let Some(old) = old_widget.as_ref() {
                self.did_update_widget(id, old);
            }
            let built = self.build_component(id, depth);
            self.last_rebuilt.push(id);
            let old_child = self.nodes[id.0]
                .as_ref()
                .and_then(|n| n.children.first().copied());
            let child = match old_child {
                Some(child) => self.update(child, built, Some(id), depth + 1),
                None => self.mount(built, Some(id), depth + 1),
            };
            if let Some(node) = self.nodes[id.0].as_mut() {
                node.children = vec![child];
            }
        } else {
            let old_children = self.nodes[id.0]
                .as_ref()
                .map(|n| n.children.clone())
                .unwrap_or_default();
            let children = self.update_children(old_children, children_widgets, id, depth + 1);
            if let Some(node) = self.nodes[id.0].as_mut() {
                node.children = children;
            }
        }
        id
    }

    /// Runs a stateful element's [`StatefulComponent::did_update_widget`],
    /// with the widget it just replaced.
    ///
    /// Upstream's `StatefulElement.update`, between taking the new widget and
    /// rebuilding: the state is told what changed before it is built again, so
    /// the build draws what the hook decided. The state is checked out exactly
    /// as a build checks it out, so a `set_state` from inside the hook queues
    /// instead of aliasing.
    fn did_update_widget(&mut self, id: ElementId, old: &AnyWidget) {
        // Nothing stateful is mounted here: a stateless element has no state
        // to tell, and upstream would have no `State` to call either.
        let Some(mut state) = self.shared.states.borrow_mut().remove(&id) else {
            return;
        };
        if let Some(node) = self.nodes[id.0].as_ref() {
            if let (WidgetKind::Component(new), WidgetKind::Component(previous)) =
                (&node.widget.inner, &old.inner)
            {
                new.did_update_widget(previous.as_any(), Some(state.as_mut()));
            }
        }
        self.shared.states.borrow_mut().insert(id, state);
    }

    /// Reconciles a child list.
    ///
    /// Upstream's `updateChildren`, step for step: sync the head while the
    /// children still match in place, scan the tail the same way from the end,
    /// and only the middle in between is unsynchronized -- keyed old children
    /// wait in a map for their key to come by, unkeyed ones are let go, and
    /// anything new in the middle mounts fresh.
    ///
    /// The two ends are why a list that grew or shrank at an end, or took an
    /// insertion in the middle, keeps the state of everything it did not
    /// actually replace: matching from the head alone would shift every child
    /// after an insertion by one position, and a shifted child is a child that
    /// lost its state. Only a middle child that *moved* needs a key to
    /// survive, which is the same trade upstream makes.
    fn update_children(
        &mut self,
        old: Vec<ElementId>,
        new: Vec<AnyWidget>,
        parent: ElementId,
        depth: usize,
    ) -> Vec<ElementId> {
        let mut new_widgets: Vec<Option<AnyWidget>> = new.into_iter().map(Some).collect();
        let mut new_children: Vec<Option<ElementId>> = vec![None; new_widgets.len()];

        let mut new_top: usize = 0;
        let mut old_top: usize = 0;
        // Bottoms sit one below the top when a list is empty, so they roam
        // below zero and are compared as signed.
        let mut new_bottom: isize = new_widgets.len() as isize - 1;
        let mut old_bottom: isize = old.len() as isize - 1;

        // Update the top of the list: everything still matching in place is
        // updated in place, up to the first mismatch.
        while old_top as isize <= old_bottom && new_top as isize <= new_bottom {
            let old_child = old[old_top];
            let widget = new_widgets[new_top]
                .take()
                .expect("each new child is placed once");
            let matched = self.nodes[old_child.0]
                .as_ref()
                .is_some_and(|node| node.widget.can_update(&widget));
            if !matched {
                new_widgets[new_top] = Some(widget);
                break;
            }
            new_children[new_top] = Some(self.update(old_child, widget, Some(parent), depth));
            new_top += 1;
            old_top += 1;
        }

        // Scan the bottom of the list. Nothing is updated yet -- the middle
        // has to be settled first -- but the tail that still matches is
        // remembered by narrowing both ends.
        while old_top as isize <= old_bottom && new_top as isize <= new_bottom {
            let old_child = old[old_bottom as usize];
            let matched = match new_widgets[new_bottom as usize].as_ref() {
                Some(widget) => self.nodes[old_child.0]
                    .as_ref()
                    .is_some_and(|node| node.widget.can_update(widget)),
                None => false,
            };
            if !matched {
                break;
            }
            old_bottom -= 1;
            new_bottom -= 1;
        }

        // Scan the old children in the middle: keyed ones wait in a map for
        // their key, unkeyed ones are dropped -- upstream deactivates them
        // here, because a child that neither stayed in place nor carried a key
        // cannot be told apart from a new one. A child a GlobalKey claim has
        // since taken is nobody's to drop (upstream's forgottenChildren).
        let have_old_middle = old_top as isize <= old_bottom;
        let mut old_keyed: HashMap<(TypeId, u64), ElementId> = HashMap::new();
        if have_old_middle {
            while old_top as isize <= old_bottom {
                let old_child = old[old_top];
                let still_here = self.nodes[old_child.0]
                    .as_ref()
                    .is_some_and(|node| node.parent == Some(parent));
                if still_here {
                    if let Some(node) = self.nodes[old_child.0].as_ref() {
                        match node.widget.key {
                            Some(key) => {
                                old_keyed.insert((node.widget.type_id, key), old_child);
                            }
                            None => self.deactivate_child(old_child),
                        }
                    }
                }
                old_top += 1;
            }
        }

        // Update the middle of the list. A keyed child claims the old element
        // its key is waiting on; the map is keyed by type as well as key, so a
        // claim always updates in place, which is upstream's canUpdate check
        // after the lookup, with the losers left for the cleanup below. An
        // unkeyed child here is new, and mounts.
        while new_top as isize <= new_bottom {
            let widget = new_widgets[new_top]
                .take()
                .expect("each new child is placed once");
            let reuse = widget
                .key
                .and_then(|key| old_keyed.remove(&(widget.type_id, key)));
            let id = match reuse {
                Some(existing) => self.update(existing, widget, Some(parent), depth),
                None => self.mount(widget, Some(parent), depth),
            };
            new_children[new_top] = Some(id);
            new_top += 1;
        }

        // The whole list has been walked; the bottom that matched in the scan
        // is still there, one past the middle on both sides.
        old_bottom = old.len() as isize - 1;
        new_bottom = new_widgets.len() as isize - 1;

        // Update the bottom of the list.
        while old_top as isize <= old_bottom && new_top as isize <= new_bottom {
            let old_child = old[old_top];
            let widget = new_widgets[new_top]
                .take()
                .expect("each new child is placed once");
            new_children[new_top] = Some(self.update(old_child, widget, Some(parent), depth));
            new_top += 1;
            old_top += 1;
        }

        // Whatever keyed middle child never came by is gone.
        for (_, id) in old_keyed {
            if self.nodes[id.0]
                .as_ref()
                .is_some_and(|node| node.parent == Some(parent))
            {
                self.deactivate_child(id);
            }
        }

        new_children
            .into_iter()
            .map(|child| child.expect("every slot was filled by one of the walks"))
            .collect()
    }

    /// Walks the element tree and produces the render tree for this frame.
    ///
    /// "Produces" overstates it. Almost nothing is produced twice: an element
    /// whose widget did not change hands back the object it made before, and an
    /// element whose widget *did* change hands the difference to the object it
    /// made before. So a screen where one counter ticks keeps every render
    /// object it had -- including the counter's own -- along with everything
    /// they had measured, shaped and drawn.
    ///
    /// This is upstream's arrangement, reached the other way round. There the
    /// render object is what persists and the widget is the description, and an
    /// element's `update` calls `updateRenderObject` on the object it is
    /// already holding; here the description arrives first and the object it
    /// describes is looked up. Where the walk still differs is when: upstream
    /// updates the render object as the element tree is reconciled, and this
    /// does it here, because a render object is built from its children's and
    /// they are not built until now.
    pub fn build_render_tree(&mut self) -> Option<BoxedRender> {
        let root = self.root?;
        Some(self.build_render(root).0)
    }

    /// The render object an element is currently holding, if it has one.
    ///
    /// For tests: persistence is only observable as identity, and identity is
    /// only reachable from here.
    pub fn render_of(&self, id: ElementId) -> Option<BoxedRender> {
        self.nodes[id.0]
            .as_ref()
            .and_then(|node| node.render.clone())
    }

    /// Builds `id`'s render object, and its subtree's underneath it.
    ///
    /// A `MediaQuery` on the way down changes the text scale for everything
    /// below it, and a `directionality` the direction, so the walk carries
    /// both. Upstream a `Text` reads `MediaQuery.textScalerOf(context)` and
    /// `Directionality.of(context)` in its own `build`; a render object here
    /// is made inside a closure that has no context, so the two are pushed
    /// for the duration of the subtree instead and taken by whatever is
    /// constructed inside it. Same values, same place in the frame -- the
    /// paragraph ends up holding them either way, which is what matters,
    /// since shaping happens at layout when the walk is long over.
    /// Returns this element's render object, and whether it is a new one.
    fn build_render(&mut self, id: ElementId) -> (BoxedRender, bool) {
        let (scale, direction) = {
            let provided = self.shared.provided.borrow();
            let entry = provided.get(&id);
            (
                entry.and_then(|entry| {
                    entry
                        .value
                        .downcast_ref::<crate::media_query::MediaQueryData>()
                        .map(|data| data.text_scale_factor)
                }),
                entry
                    .and_then(|entry| {
                        entry
                            .value
                            .downcast_ref::<crate::direction::TextDirection>()
                    })
                    .copied(),
            )
        };
        match (scale, direction) {
            (Some(scale), Some(direction)) => crate::media_query::with_text_scale(scale, || {
                crate::direction::with_direction(direction, || self.build_render_node(id))
            }),
            (Some(scale), None) => {
                crate::media_query::with_text_scale(scale, || self.build_render_node(id))
            }
            (None, Some(direction)) => {
                crate::direction::with_direction(direction, || self.build_render_node(id))
            }
            (None, None) => self.build_render_node(id),
        }
    }

    fn build_render_node(&mut self, id: ElementId) -> (BoxedRender, bool) {
        let (is_component, children, dirty, cached) = {
            let node = self.nodes[id.0]
                .as_ref()
                .expect("render walk hit a freed element");
            (
                matches!(node.widget.inner, WidgetKind::Component(_)),
                node.children.clone(),
                node.render_dirty,
                node.render.clone(),
            )
        };

        if is_component {
            // A component's render object is its child's; it owns none itself.
            return match children.first() {
                Some(child) => self.build_render(*child),
                None => (crate::render::RenderRef::new(crate::widgets::Empty), true),
            };
        }

        // The children first, because whether this object can be re-used
        // depends on whether theirs were: a parent holds its children by
        // handle, and a child that was remade is not the one this parent is
        // holding.
        let mut child_renders = Vec::with_capacity(children.len());
        let mut a_child_was_remade = false;
        for child in &children {
            let (render, remade) = self.build_render(*child);
            a_child_was_remade |= remade;
            child_renders.push(render);
        }

        if !dirty && !a_child_was_remade {
            if let Some(cached) = cached.clone() {
                return (cached, false);
            }
        }

        // The widget changed, so the object it describes has to change with it
        // -- but not into a different object. Upstream's
        // `RenderObjectElement.update` hands the new configuration to the
        // render object that is already there, and everything that object had
        // worked out stays worked out: the text it shaped, the extent it
        // measured, the layer it drew. Just as much to the point, the parent is
        // still holding the child it was holding, so a leaf that changed does
        // not remake the spine above it.
        if let Some(cached) = cached {
            let took = {
                let node = self.nodes[id.0]
                    .as_ref()
                    .expect("element vanished mid-walk");
                let WidgetKind::Render(render) = &node.widget.inner else {
                    unreachable!("checked above");
                };
                render.reconfigure(&cached, child_renders.clone())
            };
            if took {
                if let Some(node) = self.nodes[id.0].as_mut() {
                    node.render_dirty = false;
                }
                return (cached, false);
            }
        }

        let built = {
            let node = self.nodes[id.0]
                .as_ref()
                .expect("element vanished mid-walk");
            let WidgetKind::Render(render) = &node.widget.inner else {
                unreachable!("checked above");
            };
            render.create_render(child_renders)
        };
        if let Some(node) = self.nodes[id.0].as_mut() {
            node.render = Some(built.clone());
            node.render_dirty = false;
        }
        (built, true)
    }
}

impl Default for ElementTree {
    fn default() -> Self {
        Self::new()
    }
}

// -- Tests --------------------------------------------------------------------

/// Upstream `UniqueWidget`: a widget with exactly one inflated instance.
///
/// The key is **required**, which is the whole design. An ordinary widget can
/// appear anywhere any number of times, and "the state of this widget" is then
/// not a question with one answer. A global key makes it one, and
/// `currentState` is the answer -- so a caller outside the tree can reach the
/// state directly rather than threading a callback down to it.
///
/// `currentState` is an `Option` and stays one: the widget may not be mounted,
/// and upstream returns null rather than asserting. A unique widget that is
/// not on screen is an ordinary state of affairs, not a mistake.
pub struct UniqueWidget<S> {
    key: u64,
    state: Option<S>,
}

impl<S> UniqueWidget<S> {
    pub fn new(key: u64) -> UniqueWidget<S> {
        UniqueWidget { key, state: None }
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    /// Upstream's `currentState`, read through the global key.
    pub fn current_state(&self) -> Option<&S> {
        self.state.as_ref()
    }

    /// The state being created as the one instance mounts.
    pub fn mount(&mut self, state: S) {
        debug_assert!(
            self.state.is_none(),
            "a UniqueWidget has exactly one inflated instance"
        );
        self.state = Some(state);
    }

    pub fn unmount(&mut self) {
        self.state = None;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_unique_widget_that_is_not_mounted_has_no_state_and_that_is_fine() {
        // Upstream returns null rather than asserting: a unique widget off
        // screen is an ordinary state of affairs.
        let mut widget: UniqueWidget<u32> = UniqueWidget::new(7);
        assert_eq!(widget.key(), 7);
        assert!(widget.current_state().is_none());

        widget.mount(42);
        assert_eq!(widget.current_state(), Some(&42));

        widget.unmount();
        assert!(
            widget.current_state().is_none(),
            "and it goes back to none rather than keeping a stale one"
        );
    }

    #[test]
    #[should_panic(expected = "exactly one inflated instance")]
    fn a_second_instance_of_a_unique_widget_is_the_mistake_the_key_prevents() {
        let mut widget: UniqueWidget<u32> = UniqueWidget::new(7);
        widget.mount(1);
        widget.mount(2);
    }

    use super::*;
    use crate::render::{BoxConstraints, RenderBox, RenderFlex, Size};
    use crate::widgets::Empty;
    use std::cell::RefCell;
    use std::rc::Rc;

    // Counts how many times each label was built, so a test can tell a partial
    // rebuild from a total one.
    thread_local! {
        static BUILDS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    fn record(label: &'static str) {
        BUILDS.with(|b| b.borrow_mut().push(label));
    }

    fn builds_of(label: &str) -> usize {
        BUILDS.with(|b| b.borrow().iter().filter(|l| **l == label).count())
    }

    fn reset_builds() {
        BUILDS.with(|b| b.borrow_mut().clear());
    }

    struct Sized(f32);

    impl RenderBox for Sized {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            constraints.constrain(Size::square(self.0))
        }
        fn size(&self) -> Size {
            Size::square(self.0)
        }
        fn paint(&self, _c: &mut crate::render::PaintContext, _o: crate::render::Offset) {}
    }

    fn column(children: Vec<AnyWidget>) -> AnyWidget {
        many(children, |children| {
            let mut flex = RenderFlex::column();
            for child in children {
                flex = flex.push(child);
            }
            Box::new(flex)
        })
    }

    #[derive(Default)]
    struct Counter {
        count: i32,
    }

    struct CounterWidget {
        label: &'static str,
        key: Key,
        /// Where the test picks up the handle so it can drive set_state.
        sink: Rc<RefCell<Option<StateHandle<Counter>>>>,
    }

    impl StatefulComponent for CounterWidget {
        type State = Counter;

        fn key(&self) -> Key {
            self.key
        }

        fn build(
            &self,
            state: &Counter,
            handle: StateHandle<Counter>,
            _context: &mut BuildContext,
        ) -> AnyWidget {
            record(self.label);
            *self.sink.borrow_mut() = Some(handle);
            let size = state.count as f32;
            leaf(move || Sized(size))
        }
    }

    struct Static(&'static str);

    impl Component for Static {
        fn build(&self, _context: &mut BuildContext) -> AnyWidget {
            record(self.0);
            leaf(|| Empty)
        }
    }

    #[test]
    fn mounting_builds_every_component_once() {
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("a")),
            stateful(CounterWidget {
                label: "counter",
                key: None,
                sink: sink.clone(),
            }),
        ]));
        assert_eq!(builds_of("a"), 1);
        assert_eq!(builds_of("counter"), 1);
        assert!(tree.build_render_tree().is_some());
    }

    #[test]
    fn state_survives_a_rebuild_of_the_whole_tree() {
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        let build = |sink: &Rc<RefCell<Option<StateHandle<Counter>>>>| {
            column(vec![stateful(CounterWidget {
                label: "counter",
                key: None,
                sink: sink.clone(),
            })])
        };

        tree.rebuild(build(&sink));
        let handle = sink.borrow().clone().unwrap();
        assert!(handle.set_state(|s| s.count = 7));

        tree.rebuild(build(&sink));
        let id = handle.element();
        assert_eq!(tree.state::<Counter, _>(id, |s| s.count), Some(7));
    }

    #[test]
    fn set_state_rebuilds_only_the_dirty_subtree() {
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("sibling")),
            stateful(CounterWidget {
                label: "counter",
                key: None,
                sink: sink.clone(),
            }),
        ]));
        assert_eq!(builds_of("sibling"), 1);
        assert_eq!(builds_of("counter"), 1);

        let handle = sink.borrow().clone().unwrap();
        handle.set_state(|s| s.count += 1);
        assert!(tree.needs_frame());

        assert_eq!(tree.rebuild_dirty(), 1);
        // The counter built again; its sibling did not.
        assert_eq!(builds_of("counter"), 2);
        assert_eq!(builds_of("sibling"), 1);
    }

    #[test]
    fn a_changed_widget_type_replaces_the_element_and_its_state() {
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![stateful(CounterWidget {
            label: "counter",
            key: None,
            sink: sink.clone(),
        })]));
        let handle = sink.borrow().clone().unwrap();
        handle.set_state(|s| s.count = 5);

        // A different widget type in the same slot cannot be updated in place.
        tree.rebuild(column(vec![component(Static("replacement"))]));
        assert!(
            !handle.is_valid()
                || tree
                    .state::<Counter, _>(handle.element(), |s| s.count)
                    .is_none()
        );
        assert_eq!(builds_of("replacement"), 1);
    }

    #[test]
    fn keys_carry_state_across_a_reorder() {
        reset_builds();
        let first = Rc::new(RefCell::new(None));
        let second = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();

        let widgets = |a: &Rc<RefCell<Option<StateHandle<Counter>>>>,
                       b: &Rc<RefCell<Option<StateHandle<Counter>>>>,
                       swap: bool| {
            let one = stateful(CounterWidget {
                label: "one",
                key: Some(1),
                sink: a.clone(),
            });
            let two = stateful(CounterWidget {
                label: "two",
                key: Some(2),
                sink: b.clone(),
            });
            if swap {
                column(vec![two, one])
            } else {
                column(vec![one, two])
            }
        };

        tree.rebuild(widgets(&first, &second, false));
        let handle_one = first.borrow().clone().unwrap();
        let handle_two = second.borrow().clone().unwrap();
        handle_one.set_state(|s| s.count = 11);
        handle_two.set_state(|s| s.count = 22);

        tree.rebuild(widgets(&first, &second, true));

        // Both kept their own counts even though their positions swapped.
        assert_eq!(
            tree.state::<Counter, _>(handle_one.element(), |s| s.count),
            Some(11)
        );
        assert_eq!(
            tree.state::<Counter, _>(handle_two.element(), |s| s.count),
            Some(22)
        );
    }

    #[test]
    fn without_keys_state_follows_the_position() {
        reset_builds();
        let first = Rc::new(RefCell::new(None));
        let second = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();

        let widgets = |a: &Rc<RefCell<Option<StateHandle<Counter>>>>,
                       b: &Rc<RefCell<Option<StateHandle<Counter>>>>| {
            column(vec![
                stateful(CounterWidget {
                    label: "one",
                    key: None,
                    sink: a.clone(),
                }),
                stateful(CounterWidget {
                    label: "two",
                    key: None,
                    sink: b.clone(),
                }),
            ])
        };

        tree.rebuild(widgets(&first, &second));
        let handle_one = first.borrow().clone().unwrap();
        handle_one.set_state(|s| s.count = 11);

        // Rebuild with the sinks swapped: the widgets are the same type with no
        // key, so position wins and the first slot keeps its state.
        tree.rebuild(widgets(&second, &first));
        assert_eq!(
            tree.state::<Counter, _>(handle_one.element(), |s| s.count),
            Some(11)
        );
    }

    #[test]
    fn inserting_into_the_middle_of_an_unkeyed_list_keeps_the_state_after_it() {
        // The case the two-ended sync exists for. Nothing is keyed, so nothing
        // that *moves* survives -- but nothing here moves: a different-type
        // child is inserted at the front of the second half, and every child
        // after the insertion is the same widget in the same order. Matching
        // from the head alone shifts each of those by one position, and a
        // shifted child is a child whose state was dropped; the tail scan
        // matches them where they are.
        reset_builds();
        let sinks: Vec<Rc<RefCell<Option<StateHandle<Counter>>>>> =
            (0..3).map(|_| Rc::new(RefCell::new(None))).collect();
        let counter = |label: &'static str, sink: &Rc<RefCell<Option<StateHandle<Counter>>>>| {
            stateful(CounterWidget {
                label,
                key: None,
                sink: sink.clone(),
            })
        };
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            counter("first", &sinks[0]),
            counter("second", &sinks[1]),
            counter("third", &sinks[2]),
        ]));
        let handles: Vec<StateHandle<Counter>> =
            sinks.iter().map(|s| s.borrow().clone().unwrap()).collect();
        handles[0].set_state(|s| s.count = 11);
        handles[1].set_state(|s| s.count = 22);
        handles[2].set_state(|s| s.count = 33);

        tree.rebuild(column(vec![
            counter("first", &sinks[0]),
            component(Static("inserted")),
            counter("second", &sinks[1]),
            counter("third", &sinks[2]),
        ]));

        assert_eq!(
            tree.state::<Counter, _>(handles[0].element(), |s| s.count),
            Some(11)
        );
        assert_eq!(
            tree.state::<Counter, _>(handles[1].element(), |s| s.count),
            Some(22),
            "the child after the insertion kept neither its element nor its state"
        );
        assert_eq!(
            tree.state::<Counter, _>(handles[2].element(), |s| s.count),
            Some(33)
        );
    }

    #[test]
    fn a_keyed_reorder_around_an_insertion_keeps_state() {
        // Keys still win wherever the children land: both keyed children move
        // past an insertion in the middle, and each keeps its own state.
        reset_builds();
        let first = Rc::new(RefCell::new(None));
        let second = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        let one = |sink: &Rc<RefCell<Option<StateHandle<Counter>>>>| {
            stateful(CounterWidget {
                label: "one",
                key: Some(1),
                sink: sink.clone(),
            })
        };
        let two = |sink: &Rc<RefCell<Option<StateHandle<Counter>>>>| {
            stateful(CounterWidget {
                label: "two",
                key: Some(2),
                sink: sink.clone(),
            })
        };
        tree.rebuild(column(vec![one(&first), two(&second)]));
        let handle_one = first.borrow().clone().unwrap();
        let handle_two = second.borrow().clone().unwrap();
        handle_one.set_state(|s| s.count = 11);
        handle_two.set_state(|s| s.count = 22);

        tree.rebuild(column(vec![
            two(&second),
            component(Static("inserted")),
            one(&first),
        ]));

        // Both kept their own counts through the reorder and the insertion.
        assert_eq!(
            tree.state::<Counter, _>(handle_one.element(), |s| s.count),
            Some(11)
        );
        assert_eq!(
            tree.state::<Counter, _>(handle_two.element(), |s| s.count),
            Some(22)
        );
    }

    #[test]
    fn did_update_widget_sees_the_old_widget_before_the_build() {
        // A stateful element rebuilt with the same type is told what it
        // replaced, and told it before it builds: whatever the hook wrote is
        // what this very build draws. Mounting runs no hook -- upstream's
        // initState is a different method -- and neither does a type change,
        // which replaces the element outright.
        reset_builds();
        thread_local! {
            static UPDATES_SEEN: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
        }

        struct Reconfigured(i32);

        #[derive(Default)]
        struct ReconfiguredState {
            updates: usize,
            replaced: Vec<i32>,
        }

        impl StatefulComponent for Reconfigured {
            type State = ReconfiguredState;

            fn did_update_widget(&self, old: &Self, state: &mut Self::State) {
                state.updates += 1;
                state.replaced.push(old.0);
            }

            fn build(
                &self,
                state: &Self::State,
                _handle: StateHandle<Self::State>,
                _context: &mut BuildContext,
            ) -> AnyWidget {
                record("reconfigured");
                UPDATES_SEEN.with(|u| u.borrow_mut().push(state.updates));
                let value = self.0;
                leaf(move || Sized(value as f32))
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Reconfigured(1)));
        tree.rebuild(stateful(Reconfigured(2)));
        tree.rebuild(stateful(Reconfigured(3)));

        let root = tree.root().expect("mounted");
        let (replaced, updates) = tree
            .state::<ReconfiguredState, _>(root, |s| (s.replaced.clone(), s.updates))
            .expect("the state survived the updates");
        // Each rebuild reports the widget it replaced; the mount reports none.
        assert_eq!(replaced, vec![1, 2]);
        assert_eq!(updates, 2);
        // Each build saw what the hook had already written, in the same pass:
        // zero on mount, then one more with every update.
        let seen = UPDATES_SEEN.with(|u| u.borrow().clone());
        assert_eq!(seen, vec![0, 1, 2]);
    }

    #[test]
    fn removing_a_child_frees_its_element_and_state() {
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("keep")),
            stateful(CounterWidget {
                label: "drop",
                key: None,
                sink: sink.clone(),
            }),
        ]));
        let mounted = tree.len();
        let handle = sink.borrow().clone().unwrap();
        handle.set_state(|s| s.count = 3);

        tree.rebuild(column(vec![component(Static("keep"))]));
        assert!(tree.len() < mounted);
        assert_eq!(
            tree.state::<Counter, _>(handle.element(), |s| s.count),
            None
        );
    }

    #[test]
    fn a_stale_handle_is_refused_rather_than_dangling() {
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![stateful(CounterWidget {
            label: "gone",
            key: None,
            sink: sink.clone(),
        })]));
        let handle = sink.borrow().clone().unwrap();

        tree.rebuild(column(vec![]));
        // The element is unmounted; a later set_state must not resurrect it.
        // It may still report success if the slot has not been recycled, but
        // it must never write into another element's state.
        handle.set_state(|s| s.count = 99);
        assert_eq!(
            tree.state::<Counter, _>(handle.element(), |s| s.count),
            None
        );
    }

    #[test]
    fn set_state_during_build_is_queued_rather_than_aliasing() {
        reset_builds();

        struct SelfDirtying(Rc<RefCell<Option<StateHandle<Counter>>>>);

        impl StatefulComponent for SelfDirtying {
            type State = Counter;

            fn build(
                &self,
                state: &Counter,
                handle: StateHandle<Counter>,
                _context: &mut BuildContext,
            ) -> AnyWidget {
                record("self");
                *self.0.borrow_mut() = Some(handle.clone());
                if state.count < 2 {
                    // Legal, and the reason set_state has to tolerate being
                    // called while the state is checked out.
                    handle.set_state(|s| s.count += 1);
                }
                leaf(|| Empty)
            }
        }

        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(SelfDirtying(sink.clone())));
        let handle = sink.borrow().clone().unwrap();
        assert_eq!(
            tree.state::<Counter, _>(handle.element(), |s| s.count),
            Some(0)
        );

        // The queued mutation lands at the start of the next pass.
        tree.rebuild_dirty();
        assert_eq!(
            tree.state::<Counter, _>(handle.element(), |s| s.count),
            Some(1)
        );
        tree.rebuild_dirty();
        assert_eq!(
            tree.state::<Counter, _>(handle.element(), |s| s.count),
            Some(2)
        );
    }

    #[test]
    fn advancing_marks_the_element_dirty_so_the_next_build_sees_it() {
        reset_builds();

        #[derive(Default)]
        struct Clock {
            ticks: u32,
        }

        struct Ticking;

        impl StatefulComponent for Ticking {
            type State = Clock;

            fn advance(&self, state: &mut Clock, _now: i64) -> bool {
                state.ticks += 1;
                // Stops after three, so the test does not describe a loop that
                // never ends.
                state.ticks < 3
            }

            fn build(
                &self,
                _state: &Clock,
                _handle: StateHandle<Clock>,
                _context: &mut BuildContext,
            ) -> AnyWidget {
                record("ticking");
                leaf(|| Empty)
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Ticking));
        assert_eq!(builds_of("ticking"), 1);

        // Each advance that reports a change dirties its element, so the
        // rebuild that follows actually rebuilds it.
        assert!(tree.advance_frame(16_000));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(builds_of("ticking"), 2);

        assert!(tree.advance_frame(32_000));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(builds_of("ticking"), 3);

        // The third advance reports no further change, and nothing rebuilds.
        assert!(!tree.advance_frame(48_000));
        assert_eq!(tree.rebuild_dirty(), 0);
        assert_eq!(builds_of("ticking"), 3);
    }

    #[test]
    fn rebuilding_an_ancestor_subsumes_its_dirty_descendants() {
        reset_builds();

        struct Outer(
            Rc<RefCell<Option<StateHandle<Counter>>>>,
            Rc<RefCell<Option<StateHandle<Counter>>>>,
        );

        impl StatefulComponent for Outer {
            type State = Counter;

            fn build(
                &self,
                _state: &Counter,
                handle: StateHandle<Counter>,
                _context: &mut BuildContext,
            ) -> AnyWidget {
                record("outer");
                *self.0.borrow_mut() = Some(handle);
                stateful(CounterWidget {
                    label: "inner",
                    key: None,
                    sink: self.1.clone(),
                })
            }
        }

        let outer_sink = Rc::new(RefCell::new(None));
        let inner_sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Outer(outer_sink.clone(), inner_sink.clone())));
        assert_eq!(builds_of("outer"), 1);
        assert_eq!(builds_of("inner"), 1);

        outer_sink
            .borrow()
            .clone()
            .unwrap()
            .set_state(|s| s.count += 1);
        inner_sink
            .borrow()
            .clone()
            .unwrap()
            .set_state(|s| s.count += 1);

        // Two dirty elements, one of which contains the other: one rebuild.
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(builds_of("outer"), 2);
        assert_eq!(builds_of("inner"), 2);
    }

    // -- Inherited values -----------------------------------------------------

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Published(i32);

    /// Reads the published value, and says what it read.
    struct Reader;

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let value = context.inherited::<Published>().map_or(0, |v| v.0);
            record("reader");
            leaf(move || Sized(value as f32))
        }
    }

    /// Sits in the same tree and reads nothing.
    struct Bystander;

    impl Component for Bystander {
        fn build(&self, _context: &mut BuildContext) -> AnyWidget {
            record("bystander");
            leaf(|| Sized(1.0))
        }
    }

    fn published_tree() -> ElementTree {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Published(1),
            column(vec![component(Reader), component(Bystander)]),
        ));
        tree
    }

    #[test]
    fn reading_a_value_registers_a_dependency() {
        reset_builds();
        let tree = published_tree();
        let provider = tree.root.expect("a mounted root");
        assert_eq!(tree.dependent_count(provider), 1, "one of the two read it");
    }

    #[test]
    fn a_new_value_rebuilds_its_readers_and_nothing_else() {
        reset_builds();
        let mut tree = published_tree();
        assert_eq!(builds_of("reader"), 1);
        assert_eq!(builds_of("bystander"), 1);

        assert!(tree.publish(Published(2)));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(
            builds_of("reader"),
            2,
            "the reader should have been rebuilt"
        );
        assert_eq!(
            builds_of("bystander"),
            1,
            "and nothing else should have been"
        );
    }

    #[test]
    fn the_new_value_is_the_one_that_gets_read() {
        reset_builds();
        let mut tree = published_tree();
        tree.publish(Published(7));
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("a mounted root");
        root.layout(BoxConstraints::new(0.0, 100.0, 0.0, 100.0));
        // The reader sizes itself to what it read.
        assert_eq!(root.size().width, 7.0);
    }

    #[test]
    fn publishing_the_same_value_is_not_a_change() {
        reset_builds();
        let mut tree = published_tree();
        assert!(!tree.publish(Published(1)), "the same value is not news");
        assert_eq!(tree.rebuild_dirty(), 0);
        assert_eq!(builds_of("reader"), 1);
    }

    // -- Captured themes ------------------------------------------------------

    /// A value published as an inherited theme, sized so a reader can be
    /// measured rather than interrogated.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Skin(f32);

    /// Takes the capture where it is built, and hands it back.
    struct Capturer(Rc<RefCell<Option<ThemeCapture>>>);

    impl Component for Capturer {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.0.borrow_mut() = Some(context.capture_themes());
            leaf(|| Sized(1.0))
        }
    }

    /// Sizes itself to the `Skin` it can see, or to nothing.
    struct SkinReader;

    impl Component for SkinReader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let width = context.inherited::<Skin>().map_or(0.0, |skin| skin.0);
            leaf(move || Sized(width))
        }
    }

    /// Mounts `under` and returns the capture its `Capturer` took.
    fn capture_under(under: impl FnOnce(AnyWidget) -> AnyWidget) -> ThemeCapture {
        let slot: Rc<RefCell<Option<ThemeCapture>>> = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(under(component(Capturer(Rc::clone(&slot)))));
        let capture = slot.borrow().clone().expect("the capturer built");
        capture
    }

    /// What a `SkinReader` measures when it is built in a tree of its own,
    /// wrapped in `capture` -- the overlay case, in miniature.
    fn skin_seen_through(capture: &ThemeCapture) -> f32 {
        let mut tree = ElementTree::new();
        tree.rebuild(capture.wrap(component(SkinReader)));
        let mut root = tree.build_render_tree().expect("a mounted root");
        root.layout(BoxConstraints::new(0.0, 100.0, 0.0, 100.0));
        root.size().width
    }

    #[test]
    fn a_captured_theme_reaches_a_tree_that_is_not_below_it() {
        // The whole point: the reader is in another tree entirely, which is
        // what an overlay is to the page that opened a menu in it.
        let capture = capture_under(|child| provide_theme(Skin(7.0), child));
        assert_eq!(capture.len(), 1);
        assert_eq!(skin_seen_through(&capture), 7.0);
    }

    #[test]
    fn a_plain_published_value_is_not_carried() {
        // Upstream captures `InheritedTheme`s and nothing else, and this is the
        // reason: a `MediaQuery` that followed a menu into the overlay would
        // tell it the size of the button it came from.
        let capture = capture_under(|child| provide(Skin(7.0), child));
        assert!(capture.is_empty());
        assert_eq!(skin_seen_through(&capture), 0.0, "nothing was carried");
    }

    #[test]
    fn only_the_nearest_theme_of_a_type_travels() {
        // Upstream: "inherited themes completely shadow ancestors of the same
        // type". Carrying both would wrap the child twice, with the shadowed
        // one on the outside where nothing can read it.
        let capture =
            capture_under(|child| provide_theme(Skin(3.0), provide_theme(Skin(9.0), child)));
        assert_eq!(capture.len(), 1);
        assert_eq!(skin_seen_through(&capture), 9.0, "the nearer one");
    }

    #[test]
    fn a_material_theme_is_a_theme_without_being_told() {
        // An application publishes a `Theme` with plain `provide` -- there is
        // no `Theme` widget in this port to mark it as upstream's does -- so
        // `provide` marks it, and this is the test that says so.
        struct ThemeReader(Rc<RefCell<Option<crate::components::Theme>>>);
        impl Component for ThemeReader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = Some((*crate::components::theme_of(context)).clone());
                leaf(|| Sized(1.0))
            }
        }

        let capture = capture_under(|child| provide(crate::components::Theme::light(), child));
        assert_eq!(capture.len(), 1);

        let seen = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(capture.wrap(component(ThemeReader(Rc::clone(&seen)))));
        assert_eq!(
            seen.borrow().as_ref().map(|theme| theme.background),
            Some(crate::components::Theme::light().background),
            "the light theme crossed, rather than the dark default"
        );
    }

    #[test]
    fn publishing_a_type_nobody_provides_does_nothing() {
        #[derive(PartialEq)]
        struct Unrelated(bool);
        let mut tree = published_tree();
        assert!(!tree.publish(Unrelated(true)));
    }

    #[test]
    fn a_widget_that_stops_reading_stops_being_rebuilt() {
        // The dependency is re-registered by each build, so one that stops
        // asking has to stop hearing about it. Without clearing, the map grows
        // stale entries and a widget is rebuilt for a value it no longer uses.
        thread_local! {
            static READS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
        }
        struct Fickle;
        impl Component for Fickle {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                if READS.with(|r| r.get()) {
                    context.inherited::<Published>();
                }
                record("fickle");
                leaf(|| Sized(1.0))
            }
        }

        reset_builds();
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Published(1), component(Fickle)));
        let provider = tree.root.expect("a mounted root");
        assert_eq!(tree.dependent_count(provider), 1);

        // Stop reading, and rebuild for a reason of its own.
        READS.with(|r| r.set(false));
        tree.publish(Published(2));
        tree.rebuild_dirty();
        assert_eq!(builds_of("fickle"), 2);
        assert_eq!(
            tree.dependent_count(provider),
            0,
            "it no longer reads the value"
        );

        tree.publish(Published(3));
        assert_eq!(tree.rebuild_dirty(), 0, "and should not be rebuilt for it");
    }

    // -- Inherited values, by aspect -------------------------------------------

    /// A published value with two distinguishable parts, standing in for a
    /// `MediaQueryData` whose size moved while its padding did not.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct AB {
        a: i32,
        b: i32,
    }

    impl DependentNotify for AB {
        fn is_aspect_stale(old: &Self, new: &Self, aspect: &str) -> bool {
            match aspect {
                "a" => old.a != new.a,
                "b" => old.b != new.b,
                // Not every part of a value has a name. A reader asking after
                // an unnamed one hears about every change rather than
                // silently none.
                _ => true,
            }
        }
    }

    /// Reads `aspect` of the published value, and says when it built.
    struct AspectReader {
        label: &'static str,
        aspect: &'static str,
    }

    impl Component for AspectReader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let value = context
                .inherited_aspect::<AB>(self.aspect)
                .map_or(0, |v| v.a);
            record(self.label);
            leaf(move || Sized(value as f32))
        }
    }

    fn model_tree(readers: Vec<AnyWidget>) -> ElementTree {
        let mut tree = ElementTree::new();
        tree.rebuild(provide_model(AB { a: 1, b: 1 }, column(readers)));
        tree
    }

    #[test]
    fn an_aspect_reader_is_rebuilt_only_when_its_aspect_changed() {
        reset_builds();
        let mut tree = model_tree(vec![
            component(AspectReader {
                label: "reads-a",
                aspect: "a",
            }),
            component(AspectReader {
                label: "reads-b",
                aspect: "b",
            }),
        ]);
        assert_eq!(builds_of("reads-a"), 1);
        assert_eq!(builds_of("reads-b"), 1);

        // `a` moved and `b` did not: one reader, not both. The size changing
        // is not the padding reader's news.
        assert!(tree.publish(AB { a: 2, b: 1 }));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(builds_of("reads-a"), 2);
        assert_eq!(builds_of("reads-b"), 1);

        // And the other way around.
        assert!(tree.publish(AB { a: 2, b: 2 }));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(builds_of("reads-a"), 2, "still only told once");
        assert_eq!(builds_of("reads-b"), 2);
    }

    #[test]
    fn a_reader_of_several_aspects_is_rebuilt_when_either_changes() {
        struct ReadsBoth;

        impl Component for ReadsBoth {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let a = context.inherited_aspect::<AB>("a").map_or(0, |v| v.a);
                let _ = context.inherited_aspect::<AB>("b");
                record("both");
                leaf(move || Sized(a as f32))
            }
        }

        reset_builds();
        let mut tree = model_tree(vec![component(ReadsBoth)]);
        assert_eq!(builds_of("both"), 1);

        tree.publish(AB { a: 2, b: 1 });
        tree.rebuild_dirty();
        assert_eq!(builds_of("both"), 2, "the a it reads moved");

        tree.publish(AB { a: 2, b: 3 });
        tree.rebuild_dirty();
        assert_eq!(builds_of("both"), 3, "and so did the b");
    }

    #[test]
    fn a_whole_reader_of_a_model_value_hears_about_every_change() {
        // A reader that did not qualify -- a SafeArea reading the whole
        // MediaQuery -- is rebuilt for a change in any part, model or no.
        struct ReadsWhole;

        impl Component for ReadsWhole {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let value = context.inherited::<AB>().map_or(0, |v| v.a);
                record("whole");
                leaf(move || Sized(value as f32))
            }
        }

        reset_builds();
        let mut tree = model_tree(vec![
            component(ReadsWhole),
            component(AspectReader {
                label: "reads-a",
                aspect: "a",
            }),
        ]);

        // `b` moved: the aspect reader of `a` is spared, the whole reader is
        // not.
        assert!(tree.publish(AB { a: 1, b: 2 }));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(builds_of("whole"), 2);
        assert_eq!(builds_of("reads-a"), 1);
    }

    #[test]
    fn a_build_that_read_whole_is_not_narrowed_by_a_later_aspect_read() {
        // Upstream keeps an empty aspect set empty: a reader that read the
        // value whole cannot go back and pretend it only wanted a part of it.
        struct WholeThenPart;

        impl Component for WholeThenPart {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let value = context.inherited::<AB>().map_or(0, |v| v.a);
                let _ = context.inherited_aspect::<AB>("a");
                record("whole-then-part");
                leaf(move || Sized(value as f32))
            }
        }

        reset_builds();
        let mut tree = model_tree(vec![component(WholeThenPart)]);
        tree.publish(AB { a: 1, b: 9 });
        tree.rebuild_dirty();
        assert_eq!(builds_of("whole-then-part"), 2, "the b change was its news");
    }

    #[test]
    fn an_aspect_the_value_cannot_speak_for_counts_as_changed() {
        // Same shape as the readers above, but the aspect names no part of the
        // value: every change is its news, never none.
        reset_builds();
        let mut tree = model_tree(vec![component(AspectReader {
            label: "reads-c",
            aspect: "c",
        })]);
        tree.publish(AB { a: 1, b: 9 });
        tree.rebuild_dirty();
        assert_eq!(builds_of("reads-c"), 2);
    }

    #[test]
    fn a_value_without_aspect_comparison_notifies_whole() {
        // `Published` does not implement DependentNotify. A reader that
        // qualified its dependence anyway gets today's behavior: the value
        // cannot say which part changed, so every change is its news.
        struct Qualified;

        impl Component for Qualified {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let value = context
                    .inherited_aspect::<Published>("anything")
                    .map_or(0, |v| v.0);
                record("qualified");
                leaf(move || Sized(value as f32))
            }
        }

        reset_builds();
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Published(1), component(Qualified)));
        assert!(tree.publish(Published(2)));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(builds_of("qualified"), 2);
    }

    #[test]
    fn publishing_an_identical_model_value_is_not_a_change() {
        // The whole-value comparison still gates everything, aspect or no:
        // republishing what is already published rebuilds nobody.
        reset_builds();
        let mut tree = model_tree(vec![component(AspectReader {
            label: "reads-a",
            aspect: "a",
        })]);
        assert!(
            !tree.publish(AB { a: 1, b: 1 }),
            "the same value is not news"
        );
        assert_eq!(tree.rebuild_dirty(), 0);
        assert_eq!(builds_of("reads-a"), 1);
    }

    #[test]
    fn a_change_nobody_subscribed_to_rebuilds_nothing() {
        // The provider is there and the value changes, but no reader ever
        // registered: nobody is notified, nobody rebuilds.
        reset_builds();
        let mut tree = ElementTree::new();
        tree.rebuild(provide_model(AB { a: 1, b: 1 }, component(Bystander)));
        let provider = tree.root.expect("a mounted root");
        assert_eq!(tree.dependent_count(provider), 0, "nobody reads it");

        assert!(tree.publish(AB { a: 5, b: 5 }), "the value did change");
        assert_eq!(tree.rebuild_dirty(), 0, "but there was nobody to tell");
        assert_eq!(builds_of("bystander"), 1);
    }

    // -- Notifications --------------------------------------------------------

    /// A notification that only says its name, so a test can tell which
    /// listener saw it.
    struct Ping(&'static str);

    impl Notification for Ping {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// A different type entirely, to show a listener is chosen by type.
    struct Other(u32);

    impl Notification for Other {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// Dispatches one notification during its build.
    struct Dispatcher(&'static str);

    impl Component for Dispatcher {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            context.dispatch_notification(&Ping(self.0));
            leaf(|| Empty)
        }
    }

    /// Records every notification it is offered, under a label.
    fn recorder(
        log: &Rc<RefCell<Vec<&'static str>>>,
        label: &'static str,
        handled: bool,
    ) -> Box<dyn for<'a> Fn(&'a Ping) -> bool> {
        let log = log.clone();
        Box::new(move |_notification| {
            log.borrow_mut().push(label);
            handled
        })
    }

    #[test]
    fn notifications_bubble_from_the_child_up_through_every_listener() {
        // Nearest listener first: upstream's notification tree is a chain from
        // the dispatching element to the root, walked in that order.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut tree = ElementTree::new();
        tree.rebuild(notification_listener(
            recorder(&log, "outer", false),
            notification_listener(
                recorder(&log, "inner", false),
                component(Dispatcher("ping")),
            ),
        ));
        assert_eq!(*log.borrow(), vec!["inner", "outer"], "nearest first");
    }

    #[test]
    fn a_listener_that_handles_a_notification_stops_it_bubbling() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut tree = ElementTree::new();
        tree.rebuild(notification_listener(
            recorder(&log, "outer", false),
            notification_listener(recorder(&log, "inner", true), component(Dispatcher("ping"))),
        ));
        assert_eq!(
            *log.borrow(),
            vec!["inner"],
            "the outer listener never heard it"
        );
    }

    #[test]
    fn listeners_only_hear_the_type_they_listen_for() {
        // A listener for Ping is not offered Other, and vice versa: upstream's
        // `notification is T` failing costs nothing and bothers nobody.
        let pings = Rc::new(RefCell::new(Vec::new()));
        let others = Rc::new(RefCell::new(Vec::new()));

        struct DispatchOther;

        impl Component for DispatchOther {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                context.dispatch_notification(&Other(1));
                leaf(|| Empty)
            }
        }

        let ping_log = pings.clone();
        let other_log = others.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(notification_listener(
            move |_notification: &Other| {
                other_log.borrow_mut().push("other");
                false
            },
            notification_listener(
                move |_notification: &Ping| {
                    ping_log.borrow_mut().push("ping");
                    false
                },
                component(DispatchOther),
            ),
        ));
        assert_eq!(*others.borrow(), vec!["other"]);
        assert!(
            pings.borrow().is_empty(),
            "the Ping listener was offered an Other"
        );
    }

    #[test]
    fn an_unmounted_listener_hears_nothing() {
        // Releasing an element takes its registration with it, the way
        // upstream's notification chain drops a deactivated element's node.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut tree = ElementTree::new();
        tree.rebuild(notification_listener(
            recorder(&log, "outer", false),
            notification_listener(
                recorder(&log, "inner", false),
                component(Dispatcher("first")),
            ),
        ));
        assert_eq!(*log.borrow(), vec!["inner", "outer"]);

        // The inner listener is gone; the dispatcher is still there.
        log.borrow_mut().clear();
        tree.rebuild(notification_listener(
            recorder(&log, "outer", false),
            component(Dispatcher("second")),
        ));
        assert_eq!(
            *log.borrow(),
            vec!["outer"],
            "the released listener heard a dispatch"
        );
    }

    #[test]
    fn a_sink_dispatches_from_outside_any_build() {
        // The shape the scrolling code needs: a widget that keeps the sink its
        // build was given and dispatches through it later, from inside a
        // set_state -- upstream's Scrollable holding its notificationContext.
        #[derive(Default)]
        struct HolderState;

        struct Holder {
            handles: Rc<RefCell<Option<StateHandle<HolderState>>>>,
            sinks: Rc<RefCell<Option<NotificationSink>>>,
        }

        impl StatefulComponent for Holder {
            type State = HolderState;

            fn build(
                &self,
                _state: &HolderState,
                handle: StateHandle<HolderState>,
                context: &mut BuildContext,
            ) -> AnyWidget {
                *self.handles.borrow_mut() = Some(handle);
                *self.sinks.borrow_mut() = Some(context.notification_sink());
                leaf(|| Empty)
            }
        }

        let log = Rc::new(RefCell::new(Vec::new()));
        let handles = Rc::new(RefCell::new(None));
        let sinks = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(notification_listener(
            recorder(&log, "heard", false),
            stateful(Holder {
                handles: handles.clone(),
                sinks: sinks.clone(),
            }),
        ));

        // Not from a build -- from nowhere in particular, as a pointer event
        // arrives between frames.
        sinks
            .borrow()
            .as_ref()
            .expect("built")
            .dispatch(&Ping("late"));
        assert_eq!(*log.borrow(), vec!["heard"]);

        // And once the tree is gone the sink is quietly inert, the way a
        // defunct context is upstream.
        let sink = sinks.borrow().clone().expect("built");
        drop(tree);
        sink.dispatch(&Ping("too late"));
        assert_eq!(*log.borrow(), vec!["heard"]);
    }

    // -- Persistent render objects -----------------------------------------

    /// The element under the root that a probe leaf ends up at.
    ///
    /// Reached by walking rather than guessed, because the numbering is an
    /// arena's business and a test that hard-codes it is testing the arena.
    fn only_leaf_under(tree: &ElementTree, root: ElementId) -> ElementId {
        let mut id = root;
        loop {
            let children = tree.children_of(id);
            match children.first() {
                Some(child) => id = *child,
                None => return id,
            }
        }
    }

    #[test]
    fn an_element_that_did_not_rebuild_keeps_its_render_object() {
        // The whole point of section sixteen's first item: a render object
        // survives the frame, so it can be given a layer, or asked to skip a
        // layout, or trusted to remember what it measured.
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("static")),
            stateful(CounterWidget {
                label: "counter",
                key: None,
                sink: sink.clone(),
            }),
        ]));
        let _ = tree.build_render_tree();

        let root = tree.root().expect("mounted");
        let children = tree.children_of(root);
        let static_side = only_leaf_under(&tree, children[0]);
        let counter_side = only_leaf_under(&tree, children[1]);
        let static_before = tree.render_of(static_side).expect("built");
        let counter_before = tree.render_of(counter_side).expect("built");

        // One counter ticks. Nothing else has anything new to say.
        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.count += 1);
        assert_eq!(tree.rebuild_dirty(), 1);
        let _ = tree.build_render_tree();

        let static_after = tree.render_of(static_side).expect("still there");
        let counter_after = tree.render_of(counter_side).expect("still there");
        assert!(
            static_before.is(&static_after),
            "an untouched element was rebuilt anyway"
        );
        assert!(
            !counter_before.is(&counter_after),
            "the one that changed should be new"
        );
    }

    #[test]
    fn a_remade_child_remakes_the_parents_that_hold_it() {
        // A parent holds its children by handle. If a child is remade and its
        // parent is not, the parent is holding last frame's child -- so the
        // spine from the change to the root has to come with it, and nothing
        // else does.
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("static")),
            stateful(CounterWidget {
                label: "counter",
                key: None,
                sink: sink.clone(),
            }),
        ]));
        let _ = tree.build_render_tree();

        let root = tree.root().expect("mounted");
        let root_before = tree.render_of(root).expect("built");

        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.count += 1);
        tree.rebuild_dirty();
        let _ = tree.build_render_tree();

        let root_after = tree.render_of(root).expect("still there");
        assert!(
            !root_before.is(&root_after),
            "the column still holds the old child"
        );
    }

    #[test]
    fn an_idle_frame_remakes_nothing_at_all() {
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("static")),
            stateful(CounterWidget {
                label: "counter",
                key: None,
                sink: sink.clone(),
            }),
        ]));
        let first = tree.build_render_tree().expect("mounted");

        assert_eq!(tree.rebuild_dirty(), 0, "nothing is dirty");
        let second = tree.build_render_tree().expect("still mounted");
        assert!(
            first.is(&second),
            "a frame with no changes rebuilt the render tree"
        );
    }

    #[test]
    fn what_a_kept_render_object_kept() {
        // Identity is only interesting because state rides on it. A render
        // object that survived is the one that did the measuring.
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("static")),
            stateful(CounterWidget {
                label: "counter",
                key: None,
                sink: sink.clone(),
            }),
        ]));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));

        let static_side = only_leaf_under(&tree, tree.children_of(tree.root().unwrap())[0]);
        let kept = tree.render_of(static_side).expect("built");
        let measured = kept.size();

        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.count += 1);
        tree.rebuild_dirty();
        let _ = tree.build_render_tree();

        // Before any layout has run this frame: the size is last frame's,
        // because it is the same object. A tree rebuilt from nothing would
        // report zero here.
        let after = tree.render_of(static_side).expect("still there");
        assert_eq!(after.size(), measured);
    }

    // -- Paintings that do not happen twice --------------------------------

    /// A leaf inside a repaint boundary, which is where a layer comes from.
    struct Kept;

    impl Component for Kept {
        fn build(&self, _context: &mut BuildContext) -> AnyWidget {
            crate::widgets::repaint_boundary(leaf(|| Sized(10.0)))
        }
    }

    #[test]
    fn a_subtree_that_did_not_change_hands_back_the_layer_it_had() {
        // The three pieces of this section together. A component nobody
        // rebuilt keeps its render object; a render object nobody rebuilt is
        // not laid out again; and a repaint boundary over one that was not
        // laid out again gives the engine the layer it already made. What the
        // frame costs is the part that changed.
        use crate::engine::LayerTree;
        use crate::engine_test_stubs::{layer_calls, reset_layer_calls};

        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Kept),
            stateful(CounterWidget {
                label: "counter",
                key: None,
                sink: sink.clone(),
            }),
        ]));

        let mut frame = |tree: &mut ElementTree| {
            let mut root = tree.build_render_tree().expect("a mounted root");
            root.layout(BoxConstraints::loose(200.0, 200.0));
            reset_layer_calls();
            let mut layers = LayerTree::new(200, 200);
            {
                let mut context =
                    crate::render::PaintContext::new(&mut layers, Size::new(200.0, 200.0));
                root.paint(&mut context, crate::render::Offset::ZERO);
            }
            layer_calls()
        };

        let first = frame(&mut tree);
        assert_eq!(first.retainable, 1, "the first frame has to draw it");
        assert_eq!(first.retained, 0);

        // The counter beside it ticks. Nothing under the boundary changed.
        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.count += 1);
        assert_eq!(tree.rebuild_dirty(), 1, "only the counter was rebuilt");

        let second = frame(&mut tree);
        assert_eq!(
            second.retained, 1,
            "the layer it already had was thrown away"
        );
        assert_eq!(second.retainable, 0, "and the same drawing recorded again");
    }

    // -- Layouts that do not happen twice ----------------------------------

    thread_local! {
        static LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// A leaf that says how many times it was asked to lay itself out.
    struct Counted(f32);

    impl RenderBox for Counted {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            LAYOUTS.with(|n| n.set(n.get() + 1));
            constraints.constrain(Size::square(self.0))
        }
        fn size(&self) -> Size {
            Size::square(self.0)
        }
        fn paint(&self, _c: &mut crate::render::PaintContext, _o: crate::render::Offset) {}
        fn update_from(
            &mut self,
            fresh: &mut dyn RenderBox,
        ) -> Option<crate::render::UpdateEffect> {
            let fresh = fresh.as_any_mut().downcast_mut::<Counted>()?;
            let effect = crate::render::UpdateEffect::relayout_if(self.0 != fresh.0);
            self.0 = fresh.0;
            Some(effect)
        }
    }

    struct Watched;

    impl Component for Watched {
        fn build(&self, _context: &mut BuildContext) -> AnyWidget {
            leaf(|| Counted(10.0))
        }
    }

    fn layouts() -> usize {
        LAYOUTS.with(|n| n.get())
    }

    /// A tree with one watched leaf beside one that a counter can dirty.
    fn watched_tree(
        sink: &Rc<RefCell<Option<crate::framework::StateHandle<Counter>>>>,
    ) -> ElementTree {
        LAYOUTS.with(|n| n.set(0));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Watched),
            stateful(CounterWidget {
                label: "counter",
                key: None,
                sink: sink.clone(),
            }),
        ]));
        tree
    }

    #[test]
    fn a_subtree_that_did_not_change_is_not_laid_out_again() {
        // The point of a render object outliving its frame. Upstream's
        // `RenderObject.layout` returns immediately when the object is clean
        // and the constraints are the ones it already answered; the same test
        // is on the handle here, and this is it working through a real frame.
        let sink = Rc::new(RefCell::new(None));
        let mut tree = watched_tree(&sink);
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(layouts(), 1, "the first frame has to measure it");

        // Something else entirely changes.
        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.count += 1);
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("still mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(
            layouts(),
            1,
            "the untouched leaf was measured a second time"
        );
    }

    #[test]
    fn a_subtree_that_did_change_is_laid_out_again() {
        // The other half, and the more important one: a skip that skips too
        // much shows a stale interface, which is worse than any amount of
        // measuring.
        let sink = Rc::new(RefCell::new(None));
        let mut tree = watched_tree(&sink);
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));
        let before = layouts();

        // The counter's own leaf is remade, so it is a new render object with
        // nothing measured yet.
        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.count += 1);
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("still mounted");
        let counter_side = only_leaf_under(&tree, tree.children_of(tree.root().unwrap())[1]);
        let counter = tree.render_of(counter_side).expect("built");
        assert!(
            counter.needs_layout(BoxConstraints::loose(200.0, 200.0)),
            "a render object that was just made has never been measured"
        );
        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(layouts(), before, "and the watched leaf still was not");
    }

    #[test]
    fn different_constraints_are_a_different_question() {
        // A window that resized asks the same objects something new, and the
        // answer they have is to the old question. Upstream tests the
        // constraints for exactly this reason.
        let sink = Rc::new(RefCell::new(None));
        let mut tree = watched_tree(&sink);
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(layouts(), 1);

        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(layouts(), 1, "the same question twice");

        root.layout(BoxConstraints::loose(120.0, 200.0));
        assert_eq!(layouts(), 2, "a narrower window is a new question");
    }

    #[test]
    fn a_render_object_can_be_told_its_answer_went_stale() {
        // "Unchanged" is only worth trusting if it can be revoked, and this is
        // the revoking: upstream's `markNeedsLayout`. It is what an object
        // calls on itself after taking a configuration that moved something,
        // and what the tests below are watching for.
        let sink = Rc::new(RefCell::new(None));
        let mut tree = watched_tree(&sink);
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(layouts(), 1);

        let watched = only_leaf_under(&tree, tree.children_of(tree.root().unwrap())[0]);
        tree.render_of(watched).expect("built").mark_needs_layout();
        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(layouts(), 2, "it was told to measure again and did not");
    }

    // -- Render objects that are told, rather than replaced ------------------
    //
    // Everything above is about the subtrees a rebuild did not reach. This is
    // about the one it did: upstream's `RenderObjectElement.update` hands the
    // new widget to the render object that is already there, so a screen that
    // rebuilt keeps every object in it and only the parts that actually differ
    // are measured again. Without it, a list where one row changed rebuilds one
    // row and re-measures all of them.

    /// A screen with a half that never changes and a half that ticks. Both are
    /// rebuilt every time, which is the point: being rebuilt is not the same as
    /// being different.
    struct Screen {
        sink: Rc<RefCell<Option<StateHandle<Counter>>>>,
        boundary: bool,
    }

    impl StatefulComponent for Screen {
        type State = Counter;

        fn build(
            &self,
            state: &Counter,
            handle: StateHandle<Counter>,
            _context: &mut BuildContext,
        ) -> AnyWidget {
            *self.sink.borrow_mut() = Some(handle);
            let ticking = 10.0 + state.count as f32;
            let steady = leaf(|| Counted(10.0));
            let steady = if self.boundary {
                crate::widgets::repaint_boundary(steady)
            } else {
                steady
            };
            column(vec![steady, leaf(move || Counted(ticking))])
        }
    }

    fn screen(boundary: bool) -> (ElementTree, Rc<RefCell<Option<StateHandle<Counter>>>>) {
        LAYOUTS.with(|n| n.set(0));
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Screen {
            sink: sink.clone(),
            boundary,
        }));
        (tree, sink)
    }

    /// The two halves of the screen, as elements.
    fn halves(tree: &ElementTree) -> (ElementId, ElementId) {
        let column = tree.children_of(tree.root().expect("mounted"))[0];
        let children = tree.children_of(column);
        (children[0], children[1])
    }

    #[test]
    fn a_rebuilt_subtree_keeps_the_render_objects_it_had() {
        // The headline. A `set_state` rebuilds both halves of this screen --
        // both widgets are new objects -- and neither render object is.
        let (mut tree, sink) = screen(false);
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(layouts(), 2, "the first frame measures both");

        let (steady, ticking) = halves(&tree);
        let was_steady = tree.render_of(steady).expect("built");
        let was_ticking = tree.render_of(ticking).expect("built");

        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.count += 1);
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("still mounted");

        assert!(
            tree.render_of(steady).expect("still there").is(&was_steady),
            "a row that was rebuilt and did not change was replaced"
        );
        assert!(
            tree.render_of(ticking)
                .expect("still there")
                .is(&was_ticking),
            "a row that changed was replaced instead of being told"
        );

        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(
            layouts(),
            3,
            "the half that did not change was measured again"
        );
    }

    #[test]
    fn a_row_that_did_change_still_shows_the_change() {
        // The other half, and the one worth being afraid of: an object that
        // takes a new configuration and does not act on it shows the old
        // interface forever, and no test of identity would notice.
        let (mut tree, sink) = screen(false);
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));

        let (_, ticking) = halves(&tree);
        assert_eq!(
            tree.render_of(ticking).expect("built").size(),
            Size::square(10.0)
        );

        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.count += 3);
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("still mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));

        assert_eq!(
            tree.render_of(ticking).expect("still there").size(),
            Size::square(13.0),
            "the object was kept and the new size was not"
        );
    }

    #[test]
    fn a_boundary_inside_a_rebuilt_subtree_keeps_its_layer() {
        // What the two before this are for. Upstream puts a repaint boundary
        // around every item of a lazy list; it only pays if an item that was
        // rebuilt without changing keeps the object holding the layer, which is
        // exactly what taking a configuration instead of making an object does.
        use crate::engine::LayerTree;
        use crate::engine_test_stubs::{layer_calls, reset_layer_calls};

        let (mut tree, sink) = screen(true);

        let frame = |tree: &mut ElementTree| {
            let mut root = tree.build_render_tree().expect("a mounted root");
            root.layout(BoxConstraints::loose(200.0, 200.0));
            reset_layer_calls();
            let mut layers = LayerTree::new(200, 200);
            {
                let mut context =
                    crate::render::PaintContext::new(&mut layers, Size::new(200.0, 200.0));
                root.paint(&mut context, crate::render::Offset::ZERO);
            }
            layer_calls()
        };

        let first = frame(&mut tree);
        assert_eq!(first.retainable, 1, "the first frame has to draw it");
        assert_eq!(first.retained, 0);

        // The other half ticks. The boundary is rebuilt along with it, and has
        // nothing new to say.
        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.count += 1);
        tree.rebuild_dirty();

        let second = frame(&mut tree);
        assert_eq!(
            second.retained, 1,
            "the drawing it already had was thrown away"
        );
        assert_eq!(second.retainable, 0, "and recorded a second time");
    }

    // -- Build error isolation (upstream ComponentElement.performRebuild's
    // try/catch around build -> ErrorWidget) --

    use std::cell::Cell;

    /// A component that panics on build while `fail` is set, so a test can
    /// flip it off and have the same element retry.
    struct Flaky {
        label: &'static str,
        fail: Rc<Cell<bool>>,
        sink: Rc<RefCell<Option<StateHandle<Counter>>>>,
    }

    impl StatefulComponent for Flaky {
        type State = Counter;

        fn build(
            &self,
            state: &Counter,
            handle: StateHandle<Counter>,
            _context: &mut BuildContext,
        ) -> AnyWidget {
            record(self.label);
            *self.sink.borrow_mut() = Some(handle);
            if self.fail.get() {
                panic!("{} exploded", self.label);
            }
            let count = state.count;
            leaf(move || Sized(count as f32 + 1.0))
        }
    }

    fn flaky(fail: &Rc<Cell<bool>>, sink: &Rc<RefCell<Option<StateHandle<Counter>>>>) -> AnyWidget {
        stateful(Flaky {
            label: "flaky",
            fail: fail.clone(),
            sink: sink.clone(),
        })
    }

    /// Whether the element is the gray box a panicked build leaves behind.
    fn is_error_placeholder(tree: &ElementTree, id: ElementId) -> bool {
        tree.nodes[id.0]
            .as_ref()
            .is_some_and(|node| node.widget.type_id == TypeId::of::<ErrorPlaceholder>())
    }

    /// The one child of a component element, which is where the placeholder
    /// or the real subtree ends up.
    fn only_child(tree: &ElementTree, id: ElementId) -> ElementId {
        let children = tree.children_of(id);
        assert_eq!(children.len(), 1, "a component has exactly one child");
        children[0]
    }

    #[test]
    fn a_panicking_build_is_replaced_and_the_frame_finishes() {
        reset_builds();
        let fail = Rc::new(Cell::new(true));
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("sibling")),
            flaky(&fail, &sink),
        ]));

        // The flaky component threw; its sibling did not, and neither failure
        // stopped the other from building.
        assert_eq!(builds_of("flaky"), 1);
        assert_eq!(builds_of("sibling"), 1);

        let root = tree.root().expect("mounted");
        let [sibling, flaky_element] = [tree.children_of(root)[0], tree.children_of(root)[1]];
        // The subtree that threw is the placeholder; the sibling is not.
        assert!(is_error_placeholder(
            &tree,
            only_child(&tree, flaky_element)
        ));
        assert!(!is_error_placeholder(&tree, only_child(&tree, sibling)));

        // The frame completes: a render tree still comes out, and the
        // element's state survived the panic.
        assert!(tree.build_render_tree().is_some());
        assert_eq!(
            tree.state::<Counter, _>(flaky_element, |s| s.count),
            Some(0)
        );
    }

    #[test]
    fn a_retried_build_recovers_the_subtree() {
        reset_builds();
        let fail = Rc::new(Cell::new(true));
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![flaky(&fail, &sink)]));
        let handle = sink.borrow().clone().expect("built");
        let flaky_element = handle.element();
        assert!(is_error_placeholder(
            &tree,
            only_child(&tree, flaky_element)
        ));

        // The next rebuild of the same element runs build again; this time it
        // returns, and the real subtree takes the placeholder's place.
        fail.set(false);
        handle.set_state(|s| s.count += 1);
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(builds_of("flaky"), 2);
        assert!(!is_error_placeholder(
            &tree,
            only_child(&tree, flaky_element)
        ));
        // The state carried over: the build drew the count the set_state wrote.
        assert_eq!(
            tree.state::<Counter, _>(flaky_element, |s| s.count),
            Some(1)
        );
        assert!(tree.build_render_tree().is_some());
    }

    #[test]
    fn a_component_that_always_panics_stays_bounded() {
        reset_builds();
        let fail = Rc::new(Cell::new(true));
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![flaky(&fail, &sink)]));
        let handle = sink.borrow().clone().expect("built");
        let flaky_element = handle.element();
        assert_eq!(builds_of("flaky"), 1);

        // The panic marks nothing: with no set_state driving it, the dirty
        // list is empty and the frame loop settles immediately.
        assert_eq!(tree.rebuild_dirty(), 0);

        // Driven externally, every frame is one build attempt and one
        // placeholder -- no growth, no runaway.
        for frame in 0..100 {
            handle.set_state(|s| s.count += 1);
            assert_eq!(tree.rebuild_dirty(), 1, "frame {frame}");
            assert_eq!(builds_of("flaky"), 2 + frame, "frame {frame}");
        }
        assert!(is_error_placeholder(
            &tree,
            only_child(&tree, flaky_element)
        ));
        assert_eq!(
            tree.state::<Counter, _>(flaky_element, |s| s.count),
            Some(100),
            "every set_state landed, none looped"
        );
        assert!(tree.build_render_tree().is_some());
    }

    // -- Global keys ---------------------------------------------------------
    //
    // Everything above moves state by matching a child where it is; a global
    // key moves it by name, which is the only way state crosses from one
    // parent to another. Upstream's deactivate -> retake -> activate, with
    // the frame's end as the deadline for a claim.

    thread_local! {
        static INITIAL_STATES: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
        static STATE_DROPS: Cell<usize> = const { Cell::new(0) };
    }

    /// A stateful widget whose initial state announces itself, so a test can
    /// tell a remount (initial state again) from a retake (state kept).
    struct Mover {
        label: &'static str,
        sink: Rc<RefCell<Option<StateHandle<MoverState>>>>,
    }

    #[derive(Default)]
    struct MoverState {
        count: i32,
    }

    impl StatefulComponent for Mover {
        type State = MoverState;

        fn initial_state(&self) -> MoverState {
            INITIAL_STATES.with(|states| states.borrow_mut().push(self.label));
            MoverState::default()
        }

        fn build(
            &self,
            _state: &MoverState,
            handle: StateHandle<MoverState>,
            _context: &mut BuildContext,
        ) -> AnyWidget {
            record(self.label);
            *self.sink.borrow_mut() = Some(handle);
            leaf(|| Sized(1.0))
        }
    }

    /// A stateful widget whose state says when it was dropped, so a test can
    /// tell "parked, then released at frame end" from "leaked".
    struct DropCounted;

    #[derive(Default)]
    struct DropCountedState;

    impl Drop for DropCountedState {
        fn drop(&mut self) {
            STATE_DROPS.with(|drops| drops.set(drops.get() + 1));
        }
    }

    impl StatefulComponent for DropCounted {
        type State = DropCountedState;

        fn build(
            &self,
            _state: &DropCountedState,
            _handle: StateHandle<DropCountedState>,
            _context: &mut BuildContext,
        ) -> AnyWidget {
            record("drop-counted");
            leaf(|| Sized(1.0))
        }
    }

    thread_local! {
        static DISPOSED: std::cell::RefCell<Vec<&'static str>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// A widget that says goodbye, so a test can tell "released" from
    /// "released and told".
    struct Farewell(&'static str);

    #[derive(Default)]
    struct FarewellState;

    impl StatefulComponent for Farewell {
        type State = FarewellState;

        fn dispose(&self, _state: &mut FarewellState) {
            DISPOSED.with(|disposed| disposed.borrow_mut().push(self.0));
        }

        fn build(
            &self,
            _state: &FarewellState,
            _handle: StateHandle<FarewellState>,
            _context: &mut BuildContext,
        ) -> AnyWidget {
            leaf(|| Sized(1.0))
        }
    }

    fn disposed() -> Vec<&'static str> {
        DISPOSED.with(|disposed| disposed.borrow().clone())
    }

    /// Upstream's `State.dispose`, which is how a state lets go of what the
    /// tree does not own -- a timer, a listener, an entry in an overlay
    /// somewhere else.
    #[test]
    fn a_released_element_is_told_it_is_going() {
        DISPOSED.with(|disposed| disposed.borrow_mut().clear());
        let mut tree = ElementTree::new();
        tree.rebuild(holder(stateful(Farewell("gone"))));
        assert_eq!(disposed(), Vec::<&str>::new(), "still mounted");

        tree.rebuild(holder(leaf(|| Empty)));
        assert_eq!(disposed(), vec!["gone"], "and told once it is not");
    }

    #[test]
    fn a_rebuild_in_place_is_not_a_goodbye() {
        // The same widget type and key: the element is updated, not released,
        // so nothing is disposed. Upstream calls `dispose` from `unmount`, not
        // from `update`.
        DISPOSED.with(|disposed| disposed.borrow_mut().clear());
        let mut tree = ElementTree::new();
        tree.rebuild(holder(stateful(Farewell("staying"))));
        tree.rebuild(holder(stateful(Farewell("staying"))));
        assert_eq!(disposed(), Vec::<&str>::new());
    }

    #[test]
    fn a_whole_released_subtree_is_told() {
        // Children as well as the parent, so a page that goes takes everything
        // it was showing with it.
        DISPOSED.with(|disposed| disposed.borrow_mut().clear());
        let mut tree = ElementTree::new();
        tree.rebuild(holder(single(stateful(Farewell("child")), |child| {
            let mut flex = RenderFlex::column();
            flex = flex.push(child);
            flex
        })));
        tree.rebuild(holder(leaf(|| Empty)));
        assert_eq!(disposed(), vec!["child"]);
    }

    /// A one-child wrapper whose widget type is stable across frames, so the
    /// reconciliation updates it in place and the only thing that moves in
    /// the tests below is the child.
    fn holder(child: AnyWidget) -> AnyWidget {
        single(child, |child| {
            let mut flex = RenderFlex::column();
            flex = flex.push(child);
            flex
        })
    }

    fn initial_states_of(label: &str) -> usize {
        INITIAL_STATES.with(|states| states.borrow().iter().filter(|l| **l == label).count())
    }

    #[test]
    fn a_global_key_moves_state_between_parents() {
        // The reparent the element tree could not do before: one rebuild
        // where the first holder drops the keyed child and the second takes
        // it. Upstream's deactivate -> retake -> activate, state and all.
        reset_builds();
        let key = GlobalKey::new();
        let sink = Rc::new(RefCell::new(None));
        let mover = || {
            with_global_key(
                key,
                stateful(Mover {
                    label: "mover",
                    sink: sink.clone(),
                }),
            )
        };
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![holder(mover()), holder(leaf(|| Empty))]));
        let handle = sink.borrow().clone().unwrap();
        handle.set_state(|state| state.count = 41);
        let element = tree.current_element(&key).expect("mounted under the key");
        assert_eq!(
            tree.current_state::<MoverState, _>(&key, |s| s.count),
            Some(41)
        );

        // Same rebuild: the first holder loses it, the second gains it.
        tree.rebuild(column(vec![holder(leaf(|| Empty)), holder(mover())]));

        assert_eq!(
            tree.current_element(&key),
            Some(element),
            "the element moved; it did not remount"
        );
        assert_eq!(
            tree.current_state::<MoverState, _>(&key, |s| s.count),
            Some(41),
            "the state crossed the move"
        );
        assert_eq!(
            initial_states_of("mover"),
            1,
            "initial state ran once, not again for the retake"
        );
        // The handle the first build was given still writes.
        assert!(handle.set_state(|state| state.count += 1));
        assert_eq!(
            tree.current_state::<MoverState, _>(&key, |s| s.count),
            Some(42)
        );
        assert!(tree.build_render_tree().is_some());
    }

    #[test]
    fn a_global_key_can_move_before_its_old_parent_reconciles() {
        // The other order: the new parent is earlier in the walk, so it
        // claims the element while the element is still attached -- upstream's
        // "forward-looking inactivity", where the old parent has not dropped
        // the child yet but will this frame.
        reset_builds();
        let key = GlobalKey::new();
        let sink = Rc::new(RefCell::new(None));
        let mover = || {
            with_global_key(
                key,
                stateful(Mover {
                    label: "mover",
                    sink: sink.clone(),
                }),
            )
        };
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![holder(leaf(|| Empty)), holder(mover())]));
        let handle = sink.borrow().clone().unwrap();
        handle.set_state(|state| state.count = 41);

        tree.rebuild(column(vec![holder(mover()), holder(leaf(|| Empty))]));

        assert_eq!(tree.current_element(&key), Some(handle.element()));
        assert_eq!(
            tree.current_state::<MoverState, _>(&key, |s| s.count),
            Some(41)
        );
        assert_eq!(initial_states_of("mover"), 1);
    }

    #[test]
    fn an_unclaimed_parked_element_is_released_at_the_end_of_the_rebuild() {
        // Dropped, with nothing claiming the key in the same rebuild: the
        // state survives the drop only until the frame ends, which is
        // upstream's finalizeTree unmounting the inactive.
        reset_builds();
        STATE_DROPS.with(|drops| drops.set(0));
        let key = GlobalKey::new();
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![with_global_key(key, stateful(DropCounted))]));
        assert_eq!(
            STATE_DROPS.with(|drops| drops.get()),
            0,
            "mounted, not dropped"
        );
        let element = tree.current_element(&key).expect("mounted under the key");

        tree.rebuild(column(vec![component(Static("after"))]));

        assert_eq!(
            STATE_DROPS.with(|drops| drops.get()),
            1,
            "released exactly once, at frame end"
        );
        assert_eq!(
            tree.current_element(&key),
            None,
            "the key names nothing now"
        );
        assert!(
            !tree.is_mounted(element),
            "the parked element was released, not left parked"
        );
    }

    #[test]
    #[should_panic(expected = "Multiple widgets used the same GlobalKey")]
    fn the_same_global_key_cannot_be_mounted_twice() {
        let key = GlobalKey::new();
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            with_global_key(key, stateful(DropCounted)),
            with_global_key(key, stateful(DropCounted)),
        ]));
    }

    #[test]
    fn without_a_global_key_a_move_still_loses_state() {
        // The behavior the key exists to change, pinned: without one, a move
        // is a drop and a mount, the state goes with the drop, and the mount
        // starts over.
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mover = || {
            stateful(Mover {
                label: "plain",
                sink: sink.clone(),
            })
        };
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![holder(mover()), holder(leaf(|| Empty))]));
        sink.borrow()
            .clone()
            .unwrap()
            .set_state(|state| state.count = 41);

        tree.rebuild(column(vec![holder(leaf(|| Empty)), holder(mover())]));

        assert_eq!(
            initial_states_of("plain"),
            2,
            "the move remounted, as it does without a key"
        );
        let handle = sink.borrow().clone().unwrap();
        assert_eq!(
            tree.state::<MoverState, _>(handle.element(), |s| s.count),
            Some(0)
        );
    }
}
