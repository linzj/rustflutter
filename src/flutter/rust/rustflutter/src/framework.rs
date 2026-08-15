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
        Some(Box::new(C::State::default()))
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
    }
}

/// Wraps a [`StatefulComponent`] as an [`AnyWidget`].
pub fn stateful<C: StatefulComponent>(widget: C) -> AnyWidget {
    AnyWidget {
        type_id: TypeId::of::<C>(),
        key: widget.key(),
        inner: WidgetKind::Component(Box::new(StatefulObject(widget))),
    }
}

/// Wraps a [`RenderWidget`] as an [`AnyWidget`].
pub fn render_widget<W: RenderWidget>(widget: W) -> AnyWidget {
    AnyWidget {
        type_id: TypeId::of::<W>(),
        key: widget.key(),
        inner: WidgetKind::Render(Box::new(RenderObjectWidget(widget))),
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
        build: move || Box::new(build()) as BoxedRender,
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
        build: move || Box::new(build()) as BoxedRender,
    })
}

struct SingleWidget<F> {
    key: Key,
    child: RefCell<Option<AnyWidget>>,
    wrap: F,
}

impl<F: Fn(BoxedRender) -> BoxedRender + 'static> RenderWidget for SingleWidget<F> {
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
            Some(child) => (self.wrap)(child),
            // A child that failed to build leaves nothing to wrap. Producing an
            // empty box keeps the frame going rather than dropping the parent.
            None => Box::new(crate::widgets::Empty),
        }
    }
}

/// A render widget with one child, given a way to wrap the child's render
/// object.
///
/// ```ignore
/// single(child, |c| Box::new(RenderPadding::new(EdgeInsets::all(8.0), c)))
/// ```
pub fn single<F>(child: AnyWidget, wrap: F) -> AnyWidget
where
    F: Fn(BoxedRender) -> BoxedRender + 'static,
{
    render_widget(SingleWidget { key: None, child: RefCell::new(Some(child)), wrap })
}

/// [`single`] with an explicit key.
pub fn keyed_single<F>(key: u64, child: AnyWidget, wrap: F) -> AnyWidget
where
    F: Fn(BoxedRender) -> BoxedRender + 'static,
{
    render_widget(SingleWidget { key: Some(key), child: RefCell::new(Some(child)), wrap })
}

struct ManyWidget<F> {
    key: Key,
    children: RefCell<Vec<AnyWidget>>,
    assemble: F,
}

impl<F: Fn(Vec<BoxedRender>) -> BoxedRender + 'static> RenderWidget for ManyWidget<F> {
    fn key(&self) -> Key {
        self.key
    }

    fn children(&self) -> Vec<AnyWidget> {
        std::mem::take(&mut *self.children.borrow_mut())
    }

    fn create_render(&self, children: Vec<BoxedRender>) -> BoxedRender {
        (self.assemble)(children)
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
pub fn many<F>(children: Vec<AnyWidget>, assemble: F) -> AnyWidget
where
    F: Fn(Vec<BoxedRender>) -> BoxedRender + 'static,
{
    render_widget(ManyWidget { key: None, children: RefCell::new(children), assemble })
}

/// [`many`] with an explicit key.
pub fn keyed_many<F>(key: u64, children: Vec<AnyWidget>, assemble: F) -> AnyWidget
where
    F: Fn(Vec<BoxedRender>) -> BoxedRender + 'static,
{
    render_widget(ManyWidget { key: Some(key), children: RefCell::new(children), assemble })
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
}

impl Shared {
    fn new() -> Rc<Shared> {
        Rc::new(Shared {
            states: RefCell::new(HashMap::new()),
            dirty: RefCell::new(Vec::new()),
            pending: RefCell::new(Vec::new()),
            needs_frame: Cell::new(false),
            generations: RefCell::new(HashMap::new()),
        })
    }

    fn generation(&self, id: ElementId) -> u64 {
        self.generations.borrow().get(&id).copied().unwrap_or(0)
    }

    fn bump_generation(&self, id: ElementId) {
        let mut generations = self.generations.borrow_mut();
        *generations.entry(id).or_insert(0) += 1;
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
}

impl BuildContext {
    fn shared(&self) -> &Rc<Shared> {
        &self.shared
    }

    /// The element being built.
    pub fn element(&self) -> ElementId {
        self.element
    }

    /// How deep in the element tree this build is. Useful for diagnostics.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Asks for another frame without changing any state -- what an animation
    /// that reads the clock rather than storing a value needs.
    pub fn request_frame(&self) {
        self.shared.needs_frame.set(true);
    }
}

// -- Elements -----------------------------------------------------------------

struct ElementNode {
    widget: AnyWidget,
    children: Vec<ElementId>,
    parent: Option<ElementId>,
    depth: usize,
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
        let mut state = self.shared.states.borrow_mut().remove(&id);
        let mut context = BuildContext { shared: Rc::clone(&self.shared), element: id, depth };

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

        let id = self.allocate(ElementNode {
            widget,
            children: Vec::new(),
            parent,
            depth,
        });
        if let Some(state) = state {
            self.shared.states.borrow_mut().insert(id, state);
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

        if let Some(node) = self.nodes[id.0].as_mut() {
            node.widget = widget;
            node.depth = depth;
            node.parent = parent;
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
    pub fn build_render_tree(&self) -> Option<BoxedRender> {
        self.root.map(|root| self.build_render(root))
    }

    fn build_render(&self, id: ElementId) -> BoxedRender {
        let node = self.nodes[id.0].as_ref().expect("render walk hit a freed element");
        match &node.widget.inner {
            WidgetKind::Component(_) => match node.children.first() {
                Some(child) => self.build_render(*child),
                None => Box::new(crate::widgets::Empty),
            },
            WidgetKind::Render(render) => {
                let children = node
                    .children
                    .iter()
                    .map(|child| self.build_render(*child))
                    .collect();
                render.create_render(children)
            }
        }
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

    /// Counts how many times each label was built, so a test can tell a
    /// partial rebuild from a total one.
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
}
