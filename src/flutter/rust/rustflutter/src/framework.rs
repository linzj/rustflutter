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

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use crate::render::BoxedRender;

// -- Identity -----------------------------------------------------------------

/// Distinguishes two widgets of the same type in the same position.
///
/// Without one, a list that reorders its children re-associates every element
/// with a different item, and any state they held goes with the position
/// rather than the item.
pub type Key = Option<u64>;

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
    fn create_render(&self, children: Vec<BoxedRender>) -> BoxedRender;
}

/// A widget with its concrete type erased.
pub struct AnyWidget {
    inner: WidgetKind,
    type_id: TypeId,
    key: Key,
    /// Set only by [`provide`]. The element registers it so descendants can
    /// find it with [`BuildContext::inherited`].
    provided: Option<Provided>,
}

enum WidgetKind {
    Component(Box<dyn ComponentObject>),
    Render(Box<dyn RenderWidgetObject>),
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
}

/// Wraps a [`Component`] as an [`AnyWidget`].
pub fn component<C: Component>(widget: C) -> AnyWidget {
    AnyWidget {
        type_id: TypeId::of::<C>(),
        key: widget.key(),
        inner: WidgetKind::Component(Box::new(StatelessObject(widget))),
        provided: None,
    }
}

/// Wraps a [`StatefulComponent`] as an [`AnyWidget`].
pub fn stateful<C: StatefulComponent>(widget: C) -> AnyWidget {
    AnyWidget {
        type_id: TypeId::of::<C>(),
        key: widget.key(),
        inner: WidgetKind::Component(Box::new(StatefulObject(widget))),
        provided: None,
    }
}

/// Wraps a [`RenderWidget`] as an [`AnyWidget`].
pub fn render_widget<W: RenderWidget>(widget: W) -> AnyWidget {
    AnyWidget {
        type_id: TypeId::of::<W>(),
        key: widget.key(),
        inner: WidgetKind::Render(Box::new(RenderObjectWidget(widget))),
        provided: None,
    }
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
        // The widget is consumed exactly once per rebuild, so taking is safe;
        // AnyWidget is not Clone because the closures inside it are not.
        self.child.borrow_mut().take().into_iter().collect()
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
    render_widget(SingleWidget { key: None, child: RefCell::new(Some(child)), wrap })
}

/// [`single`] with an explicit key.
pub fn keyed_single<F, R>(key: u64, child: AnyWidget, wrap: F) -> AnyWidget
where
    F: Fn(BoxedRender) -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    render_widget(SingleWidget { key: Some(key), child: RefCell::new(Some(child)), wrap })
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
        std::mem::take(&mut *self.children.borrow_mut())
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
    render_widget(ManyWidget { key: None, children: RefCell::new(children), assemble })
}

/// [`many`] with an explicit key.
pub fn keyed_many<F, R>(key: u64, children: Vec<AnyWidget>, assemble: F) -> AnyWidget
where
    F: Fn(Vec<BoxedRender>) -> R + 'static,
    R: crate::render::RenderBox + 'static,
{
    render_widget(ManyWidget { key: Some(key), children: RefCell::new(children), assemble })
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
        self.child.borrow_mut().take().into_iter().collect()
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
#[derive(Clone)]
struct Provided {
    type_id: TypeId,
    value: Rc<dyn Any>,
    same: fn(&dyn Any, &dyn Any) -> bool,
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
    let widget = Provider { value: Rc::new(value), child: RefCell::new(Some(child)) };
    let provided = widget.provided();
    let mut any = render_widget(widget);
    any.provided = Some(provided);
    any
}

// -- State --------------------------------------------------------------------

type Mutation = Box<dyn FnOnce(&mut dyn Any)>;

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
    /// Who reads each provider, and what each reader reads. Two maps of the
    /// same relation: the first is what a change has to rebuild, the second is
    /// what an unmounted element has to be removed from. Upstream keeps the
    /// same pair, as `InheritedElement._dependents` and
    /// `Element._dependencies`.
    dependents: RefCell<HashMap<ElementId, Vec<ElementId>>>,
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

    /// Records that `reader` read what `provider` publishes.
    ///
    /// Upstream's `InheritedElement.updateDependencies`, called from
    /// `dependOnInheritedWidgetOfExactType` for the same reason: reading a
    /// value is what makes a widget care about it changing, and nothing else
    /// can tell.
    fn depend(&self, provider: ElementId, reader: ElementId) {
        let mut dependents = self.dependents.borrow_mut();
        let readers = dependents.entry(provider).or_default();
        if !readers.contains(&reader) {
            readers.push(reader);
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
                    readers.retain(|id| *id != reader);
                }
            }
        }
    }

    /// Marks everything that reads `provider` for rebuilding.
    fn notify_dependents(&self, provider: ElementId) -> usize {
        let readers = self
            .dependents
            .borrow()
            .get(&provider)
            .cloned()
            .unwrap_or_default();
        for reader in &readers {
            self.mark_dirty(*reader);
        }
        readers.len()
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
    id: ElementId,
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
        ElementRef { id: self.element, generation: self.shared.generation(self.element) }
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
        self.inherited::<T>().unwrap_or_else(|| Rc::new(T::default()))
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
        }
    }

    /// How many elements are mounted. A rebuild that reuses everything leaves
    /// this unchanged.
    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
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
        // this walk would otherwise be holding.
        let ids: Vec<ElementId> = (0..self.nodes.len())
            .filter(|index| self.nodes[*index].is_some())
            .map(ElementId)
            .collect();

        let mut wants_frame = false;
        for id in ids {
            let Some(node) = self.nodes[id.0].as_ref() else { continue };
            let WidgetKind::Component(_) = &node.widget.inner else { continue };

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
        self.nodes[id.0].as_ref().map(|node| node.children.clone()).unwrap_or_default()
    }

    /// Whether an element is still in the tree.
    pub fn is_mounted(&self, id: ElementId) -> bool {
        self.nodes.get(id.0).is_some_and(|slot| slot.is_some())
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
        if let Some(node) = self.nodes[id.0].take() {
            for child in node.children {
                self.release(child);
            }
        }
        self.shared.states.borrow_mut().remove(&id);
        self.shared.parents.borrow_mut().remove(&id);
        self.shared.provided.borrow_mut().remove(&id);
        self.shared.clear_dependencies(id);
        self.shared.dependents.borrow_mut().remove(&id);
        self.shared.dirty.borrow_mut().retain(|d| *d != id);
        self.shared.pending.borrow_mut().retain(|(d, _)| *d != id);
        self.free.push(id.0);
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
            if self.nodes[id.0].is_none() {
                continue;
            }
            if done.iter().any(|ancestor| self.is_ancestor(*ancestor, id)) {
                continue;
            }
            self.rebuild_component(id);
            done.push(id);
            rebuilt += 1;
        }
        rebuilt
    }

    /// Republishes a provided value, rebuilding only what reads it.
    ///
    /// Returns whether anything changed. This is the half of inherited widgets
    /// that a full rebuild cannot express: the view's metrics change many
    /// times a second while a keyboard opens, and the answer to that should be
    /// rebuilding the two widgets that asked about the padding, not the page.
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

        let replacement = Provided::of(Rc::new(value));
        {
            let mut provided = self.shared.provided.borrow_mut();
            let Some(current) = provided.get(&target) else { return false };
            if (current.same)(current.value.as_ref(), replacement.value.as_ref()) {
                return false;
            }
            provided.insert(target, replacement);
        }
        self.shared.notify_dependents(target);
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
        let old_child = self.nodes[id.0].as_ref().and_then(|n| n.children.first().copied());
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
            let node = self.nodes[id.0].as_ref().expect("element vanished mid-build");
            let WidgetKind::Component(component) = &node.widget.inner else {
                unreachable!("build_component on a render element");
            };
            component.build(state.as_deref_mut(), id, &mut context)
        };

        if let Some(state) = state {
            self.shared.states.borrow_mut().insert(id, state);
        }
        built
    }

    fn mount(&mut self, widget: AnyWidget, parent: Option<ElementId>, depth: usize) -> ElementId {
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
        let id = self.allocate(ElementNode {
            widget,
            children: Vec::new(),
            parent,
            depth,
            render: None,
            render_dirty: true,
        });
        if let Some(state) = state {
            self.shared.states.borrow_mut().insert(id, state);
        }
        self.shared.parents.borrow_mut().insert(id, parent);
        match provided {
            Some(provided) => {
                self.shared.provided.borrow_mut().insert(id, provided);
            }
            None => {
                self.shared.provided.borrow_mut().remove(&id);
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
        let matches = self.nodes[id.0]
            .as_ref()
            .is_some_and(|node| node.widget.can_update(&widget));
        if !matches {
            self.release(id);
            return self.mount(widget, parent, depth);
        }

        let is_component = matches!(widget.inner, WidgetKind::Component(_));
        let children_widgets = match &widget.inner {
            WidgetKind::Component(_) => Vec::new(),
            WidgetKind::Render(render) => render.children(),
        };

        let provided = widget.provided.clone();
        if let Some(node) = self.nodes[id.0].as_mut() {
            node.widget = widget;
            node.depth = depth;
            node.parent = parent;
            // A new widget describes a new render object. Upstream calls
            // `updateRenderObject` here and mutates the existing one in place;
            // this makes a new one, which is the same thing from every
            // observer's point of view except the identity -- and the identity
            // only matters for the elements that were *not* touched, which is
            // most of them.
            node.render_dirty = true;
        }
        self.shared.parents.borrow_mut().insert(id, parent);
        match provided {
            Some(provided) => {
                self.shared.provided.borrow_mut().insert(id, provided);
            }
            None => {
                self.shared.provided.borrow_mut().remove(&id);
            }
        }

        if is_component {
            let built = self.build_component(id, depth);
            self.last_rebuilt.push(id);
            let old_child = self.nodes[id.0].as_ref().and_then(|n| n.children.first().copied());
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

    /// Reconciles a child list.
    ///
    /// Keyed children are matched by key wherever they moved to; the rest are
    /// matched by position. That is the rule that makes a reordered list keep
    /// its state, and it is why a list of anything stateful should be keyed.
    fn update_children(
        &mut self,
        old: Vec<ElementId>,
        new: Vec<AnyWidget>,
        parent: ElementId,
        depth: usize,
    ) -> Vec<ElementId> {
        // Index the old keyed children so a moved one can be found again.
        let mut keyed: HashMap<(TypeId, u64), ElementId> = HashMap::new();
        for id in &old {
            if let Some(node) = self.nodes[id.0].as_ref() {
                if let Some(key) = node.widget.key {
                    keyed.insert((node.widget.type_id, key), *id);
                }
            }
        }

        let mut taken: Vec<bool> = vec![false; old.len()];
        let mut result = Vec::with_capacity(new.len());

        for (position, widget) in new.into_iter().enumerate() {
            let reuse = match widget.key {
                Some(key) => keyed.remove(&(widget.type_id, key)).inspect(|id| {
                    if let Some(index) = old.iter().position(|o| o == id) {
                        taken[index] = true;
                    }
                }),
                None => old.get(position).copied().and_then(|candidate| {
                    // Skip a positional candidate that a keyed child claimed.
                    if taken[position] {
                        return None;
                    }
                    let usable = self.nodes[candidate.0]
                        .as_ref()
                        .is_some_and(|n| n.widget.key.is_none() && n.widget.can_update(&widget));
                    if usable {
                        taken[position] = true;
                        Some(candidate)
                    } else {
                        None
                    }
                }),
            };

            let id = match reuse {
                Some(existing) => self.update(existing, widget, Some(parent), depth),
                None => self.mount(widget, Some(parent), depth),
            };
            result.push(id);
        }

        // Whatever was not claimed is gone.
        for (index, id) in old.iter().enumerate() {
            if !taken[index] && !result.contains(id) && self.nodes[id.0].is_some() {
                self.release(*id);
            }
        }

        result
    }

    /// Walks the element tree and produces the render tree for this frame.
    ///
    /// "Produces" overstates it now. An element whose widget did not change,
    /// and none of whose descendants' render objects were remade, hands back
    /// the object it made before -- so a screen where one counter ticks keeps
    /// every other render object it had, along with everything they had
    /// measured. Upstream's arrangement, reached the other way round: there
    /// the render object is the thing that persists and the widget is the
    /// description, and an element only calls `updateRenderObject` when the
    /// description changed.
    pub fn build_render_tree(&mut self) -> Option<BoxedRender> {
        let root = self.root?;
        Some(self.build_render(root).0)
    }

    /// The render object an element is currently holding, if it has one.
    ///
    /// For tests: persistence is only observable as identity, and identity is
    /// only reachable from here.
    pub fn render_of(&self, id: ElementId) -> Option<BoxedRender> {
        self.nodes[id.0].as_ref().and_then(|node| node.render.clone())
    }

    /// Builds `id`'s render object, and its subtree's underneath it.
    ///
    /// A `MediaQuery` on the way down changes the text scale for everything
    /// below it, so the walk carries it. Upstream a `Text` reads
    /// `MediaQuery.textScalerOf(context)` in its own `build`; a render object
    /// here is made inside a closure that has no context, so the scale is
    /// pushed for the duration of the subtree instead and taken by whatever is
    /// constructed inside it. Same value, same place in the frame -- the
    /// paragraph ends up holding it either way, which is what matters, since
    /// shaping happens at layout when the walk is long over.
    /// Returns this element's render object, and whether it is a new one.
    fn build_render(&mut self, id: ElementId) -> (BoxedRender, bool) {
        let scale = {
            let provided = self.shared.provided.borrow();
            provided.get(&id).and_then(|entry| {
                entry
                    .value
                    .downcast_ref::<crate::media_query::MediaQueryData>()
                    .map(|data| data.text_scale_factor)
            })
        };
        match scale {
            Some(scale) => {
                crate::media_query::with_text_scale(scale, || self.build_render_node(id))
            }
            None => self.build_render_node(id),
        }
    }

    fn build_render_node(&mut self, id: ElementId) -> (BoxedRender, bool) {
        let (is_component, children, dirty, cached) = {
            let node = self.nodes[id.0].as_ref().expect("render walk hit a freed element");
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
            if let Some(cached) = cached {
                return (cached, false);
            }
        }

        let built = {
            let node = self.nodes[id.0].as_ref().expect("element vanished mid-walk");
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

#[cfg(test)]
mod tests {
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
            stateful(CounterWidget { label: "counter", key: None, sink: sink.clone() }),
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
            stateful(CounterWidget { label: "counter", key: None, sink: sink.clone() }),
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
        assert!(!handle.is_valid() || tree.state::<Counter, _>(handle.element(), |s| s.count).is_none());
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
            let one = stateful(CounterWidget { label: "one", key: Some(1), sink: a.clone() });
            let two = stateful(CounterWidget { label: "two", key: Some(2), sink: b.clone() });
            if swap { column(vec![two, one]) } else { column(vec![one, two]) }
        };

        tree.rebuild(widgets(&first, &second, false));
        let handle_one = first.borrow().clone().unwrap();
        let handle_two = second.borrow().clone().unwrap();
        handle_one.set_state(|s| s.count = 11);
        handle_two.set_state(|s| s.count = 22);

        tree.rebuild(widgets(&first, &second, true));

        // Both kept their own counts even though their positions swapped.
        assert_eq!(tree.state::<Counter, _>(handle_one.element(), |s| s.count), Some(11));
        assert_eq!(tree.state::<Counter, _>(handle_two.element(), |s| s.count), Some(22));
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
                stateful(CounterWidget { label: "one", key: None, sink: a.clone() }),
                stateful(CounterWidget { label: "two", key: None, sink: b.clone() }),
            ])
        };

        tree.rebuild(widgets(&first, &second));
        let handle_one = first.borrow().clone().unwrap();
        handle_one.set_state(|s| s.count = 11);

        // Rebuild with the sinks swapped: the widgets are the same type with no
        // key, so position wins and the first slot keeps its state.
        tree.rebuild(widgets(&second, &first));
        assert_eq!(tree.state::<Counter, _>(handle_one.element(), |s| s.count), Some(11));
    }

    #[test]
    fn removing_a_child_frees_its_element_and_state() {
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("keep")),
            stateful(CounterWidget { label: "drop", key: None, sink: sink.clone() }),
        ]));
        let mounted = tree.len();
        let handle = sink.borrow().clone().unwrap();
        handle.set_state(|s| s.count = 3);

        tree.rebuild(column(vec![component(Static("keep"))]));
        assert!(tree.len() < mounted);
        assert_eq!(tree.state::<Counter, _>(handle.element(), |s| s.count), None);
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
        assert_eq!(tree.state::<Counter, _>(handle.element(), |s| s.count), None);
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
        assert_eq!(tree.state::<Counter, _>(handle.element(), |s| s.count), Some(0));

        // The queued mutation lands at the start of the next pass.
        tree.rebuild_dirty();
        assert_eq!(tree.state::<Counter, _>(handle.element(), |s| s.count), Some(1));
        tree.rebuild_dirty();
        assert_eq!(tree.state::<Counter, _>(handle.element(), |s| s.count), Some(2));
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

        struct Outer(Rc<RefCell<Option<StateHandle<Counter>>>>, Rc<RefCell<Option<StateHandle<Counter>>>>);

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
                stateful(CounterWidget { label: "inner", key: None, sink: self.1.clone() })
            }
        }

        let outer_sink = Rc::new(RefCell::new(None));
        let inner_sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Outer(outer_sink.clone(), inner_sink.clone())));
        assert_eq!(builds_of("outer"), 1);
        assert_eq!(builds_of("inner"), 1);

        outer_sink.borrow().clone().unwrap().set_state(|s| s.count += 1);
        inner_sink.borrow().clone().unwrap().set_state(|s| s.count += 1);

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
        assert_eq!(builds_of("reader"), 2, "the reader should have been rebuilt");
        assert_eq!(builds_of("bystander"), 1, "and nothing else should have been");
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
        assert_eq!(tree.dependent_count(provider), 0, "it no longer reads the value");

        tree.publish(Published(3));
        assert_eq!(tree.rebuild_dirty(), 0, "and should not be rebuilt for it");
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
            stateful(CounterWidget { label: "counter", key: None, sink: sink.clone() }),
        ]));
        let _ = tree.build_render_tree();

        let root = tree.root().expect("mounted");
        let children = tree.children_of(root);
        let static_side = only_leaf_under(&tree, children[0]);
        let counter_side = only_leaf_under(&tree, children[1]);
        let static_before = tree.render_of(static_side).expect("built");
        let counter_before = tree.render_of(counter_side).expect("built");

        // One counter ticks. Nothing else has anything new to say.
        sink.borrow().as_ref().expect("built").set_state(|state| state.count += 1);
        assert_eq!(tree.rebuild_dirty(), 1);
        let _ = tree.build_render_tree();

        let static_after = tree.render_of(static_side).expect("still there");
        let counter_after = tree.render_of(counter_side).expect("still there");
        assert!(static_before.is(&static_after), "an untouched element was rebuilt anyway");
        assert!(!counter_before.is(&counter_after), "the one that changed should be new");
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
            stateful(CounterWidget { label: "counter", key: None, sink: sink.clone() }),
        ]));
        let _ = tree.build_render_tree();

        let root = tree.root().expect("mounted");
        let root_before = tree.render_of(root).expect("built");

        sink.borrow().as_ref().expect("built").set_state(|state| state.count += 1);
        tree.rebuild_dirty();
        let _ = tree.build_render_tree();

        let root_after = tree.render_of(root).expect("still there");
        assert!(!root_before.is(&root_after), "the column still holds the old child");
    }

    #[test]
    fn an_idle_frame_remakes_nothing_at_all() {
        reset_builds();
        let sink = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Static("static")),
            stateful(CounterWidget { label: "counter", key: None, sink: sink.clone() }),
        ]));
        let first = tree.build_render_tree().expect("mounted");

        assert_eq!(tree.rebuild_dirty(), 0, "nothing is dirty");
        let second = tree.build_render_tree().expect("still mounted");
        assert!(first.is(&second), "a frame with no changes rebuilt the render tree");
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
            stateful(CounterWidget { label: "counter", key: None, sink: sink.clone() }),
        ]));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));

        let static_side = only_leaf_under(&tree, tree.children_of(tree.root().unwrap())[0]);
        let kept = tree.render_of(static_side).expect("built");
        let measured = kept.size();

        sink.borrow().as_ref().expect("built").set_state(|state| state.count += 1);
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
            stateful(CounterWidget { label: "counter", key: None, sink: sink.clone() }),
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
        sink.borrow().as_ref().expect("built").set_state(|state| state.count += 1);
        assert_eq!(tree.rebuild_dirty(), 1, "only the counter was rebuilt");

        let second = frame(&mut tree);
        assert_eq!(second.retained, 1, "the layer it already had was thrown away");
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
    fn watched_tree(sink: &Rc<RefCell<Option<crate::framework::StateHandle<Counter>>>>)
        -> ElementTree
    {
        LAYOUTS.with(|n| n.set(0));
        let mut tree = ElementTree::new();
        tree.rebuild(column(vec![
            component(Watched),
            stateful(CounterWidget { label: "counter", key: None, sink: sink.clone() }),
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
        sink.borrow().as_ref().expect("built").set_state(|state| state.count += 1);
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("still mounted");
        root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(layouts(), 1, "the untouched leaf was measured a second time");
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
        sink.borrow().as_ref().expect("built").set_state(|state| state.count += 1);
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("still mounted");
        let counter_side =
            only_leaf_under(&tree, tree.children_of(tree.root().unwrap())[1]);
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
        // Nothing in this framework changes a render object in place today --
        // a change comes through the element tree and comes out as a new
        // object -- but "unchanged" is only worth trusting if it can be
        // revoked. Upstream's `markNeedsLayout` is the same escape hatch.
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
}
