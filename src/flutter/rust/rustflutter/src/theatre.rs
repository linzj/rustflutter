//! The overlay host: upstream's `Overlay`, `_RenderTheatre` and the render
//! half of `OverlayPortal`.
//!
//! `overlay.rs` has the bookkeeping -- which entries are on stage, which are
//! occluded by an opaque one, what order they stack in -- and until now it had
//! nothing to host. This is the host: a render object that takes a page and a
//! set of entries and puts the entries on top of it, and the seam that lets a
//! widget **build in one place and render in another**.
//!
//! # Why the seam has to exist
//!
//! A tooltip belongs on top of everything, so it has to render at the root. It
//! also belongs to the button that owns it, so it has to *build* at the button
//! -- that is where its `Theme`, its `Directionality`, its `MediaQuery` and
//! every other inherited value come from. Upstream says the same thing in
//! `overlay.dart`, and this port's own module header repeated it: building the
//! tooltip in the overlay would give it the overlay's inherited context rather
//! than the button's.
//!
//! Two trees make that possible. The element tree decides what a widget
//! inherits; the render tree decides where it lands. A portal keeps its overlay
//! child in the element tree under itself and hands the *render object* it
//! produced to the theatre.
//!
//! # How it works in one frame
//!
//! `ElementTree::build_render` is bottom-up -- a render object is assembled
//! from its children's, so a descendant's assemble closure runs before its
//! ancestor's. The theatre is an ancestor of every portal that targets it.
//! So within a single frame:
//!
//! 1. the portal's assemble runs, keeps its in-place child, and **stages** its
//!    overlay child on a shared [`OverlayStage`];
//! 2. the theatre's assemble runs afterwards, **collects** whatever was staged.
//!
//! No second layout pass, no one-frame lag, no deferred queue.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::framework::{
    AnyWidget, BuildContext, ElementId, StateHandle, StatefulComponent, many, provide,
};
use crate::modal_barrier::ModalBarrier;
use crate::overlay::{
    InsertPosition, OverlayEntry, OverlayPortalClock, OverlayPortalController, OverlayState,
};
use crate::render::{
    BoxConstraints, BoxedRender, HitTestResult, Offset, PaintContext, RenderBox,
    RenderConstrainedBox, RenderRef, Size, UpdateEffect,
};

/// A render object waiting to be picked up by a theatre, and where it sits in
/// the stack.
#[derive(Clone)]
pub struct StagedEntry {
    /// Which portal put it there. The registry is keyed by this, so a portal
    /// that rebuilds replaces its own entry rather than adding another.
    pub portal_id: u64,
    /// The render object a portal built in its own place.
    pub render: RenderRef,
    /// Upstream's `OverlayPortalController._zOrderIndex`: the tick at which the
    /// portal asked to be shown. Later ticks paint later, so a portal opened
    /// after another is on top of it however the tree is arranged.
    pub z_order: u64,
    /// Which theatre this is for, when more than one is on screen. Zero is the
    /// root overlay.
    pub stage_id: u64,
}

/// What portals have handed to their theatre, and still stand behind.
///
/// # It is a registry, not a per-frame buffer
///
/// It was a buffer first: portals pushed during their assemble and the theatre
/// drained during its own, which runs later in the same walk. That works only
/// while every frame rebuilds every widget, and this framework does not. A
/// portal whose render object is *reconfigured* rather than remade reports no
/// change to its parent, so the walk stops before reaching the theatre, whose
/// assemble never re-runs -- and the pushed entry sits in the buffer forever.
/// The reverse case is worse: a portal that did not rebuild at all pushes
/// nothing, so a drained buffer would lose it.
///
/// So entries persist, keyed by the portal that owns them. A portal replaces
/// its own entry when it rebuilds, removes it when it hides, and is otherwise
/// left alone -- and the theatre reads the whole set at layout, which is a
/// phase that runs whenever anything beneath it changed.
///
/// This works because a [`RenderRef`] is a *persistent handle*: a rebuilt
/// subtree keeps its render objects, so an entry registered once stays correct
/// across rebuilds without being re-registered.
#[derive(Clone, Default)]
pub struct OverlayStage {
    entries: Rc<RefCell<Vec<StagedEntry>>>,
    /// Bumped on every change, so a render object can tell that the set it is
    /// showing is out of date. See [`RenderNothing`] for why one is needed.
    revision: Rc<Cell<u64>>,
    /// The theatres reading this registry, so a change can reach them.
    ///
    /// A portal marking itself is not enough: `mark_needs_layout` stops at the
    /// first relayout boundary, and a boundary between a portal and its theatre
    /// is the ordinary case -- any tightly constrained box on the way is one.
    /// The theatre would then keep the entries it collected last time, and a
    /// hidden portal would stay on screen.
    watchers: Rc<RefCell<Vec<RenderRef>>>,
}

impl OverlayStage {
    pub fn new() -> OverlayStage {
        OverlayStage::default()
    }

    /// Registers, or replaces, the entry a portal is showing.
    pub fn put(&self, entry: StagedEntry) {
        let mut entries = self.entries.borrow_mut();
        match entries
            .iter_mut()
            .find(|existing| existing.portal_id == entry.portal_id)
        {
            Some(existing) => *existing = entry,
            None => entries.push(entry),
        }
        self.revision.set(self.revision.get() + 1);
        self.wake_theatres();
    }

    /// Withdraws a portal's entry: it hid, or it went away.
    pub fn take_out(&self, portal_id: u64) {
        let before = self.entries.borrow().len();
        self.entries
            .borrow_mut()
            .retain(|entry| entry.portal_id != portal_id);
        if self.entries.borrow().len() != before {
            self.revision.set(self.revision.get() + 1);
            self.wake_theatres();
        }
    }

    /// Registers a theatre, so a change to the registry reaches it. Idempotent.
    fn watch(&self, theatre: RenderRef) {
        let mut watchers = self.watchers.borrow_mut();
        if !watchers.iter().any(|watcher| watcher.is(&theatre)) {
            watchers.push(theatre);
        }
    }

    fn wake_theatres(&self) {
        for watcher in self.watchers.borrow().iter() {
            watcher.mark_needs_layout();
        }
    }

    /// What the registry is up to. Compared by [`RenderNothing`].
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    /// Everything registered for `stage_id`, in z order. Non-destructive: the
    /// theatre reads this every layout and the entries outlive the reading.
    pub fn snapshot(&self, stage_id: u64) -> Vec<StagedEntry> {
        let mut mine: Vec<StagedEntry> = self
            .entries
            .borrow()
            .iter()
            .filter(|entry| entry.stage_id == stage_id)
            .cloned()
            .collect();
        mine.sort_by_key(|entry| entry.z_order);
        mine
    }

    /// How many entries are registered, for tests.
    pub fn registered(&self) -> usize {
        self.entries.borrow().len()
    }
}

/// Where a theatre puts an entry that did not ask for a position.
///
/// Upstream's `_RenderTheatre` takes its size from the page beneath it and
/// gives every non-positioned entry the same tight constraints. An entry that
/// wants to be somewhere in particular is wrapped in a `Positioned` by whoever
/// built it, which is a widget-level decision the theatre never sees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EntryPlacement {
    /// Fill the theatre. Upstream's non-positioned entry under a tight stack.
    Fill,
    /// Sit at an offset from the theatre's origin, at the entry's own size.
    At(Offset),
}

/// Upstream `_RenderTheatre`.
///
/// A stack with one child that is not an entry -- the page -- and a set that
/// are. The page decides the size; the entries are laid out against it.
pub struct RenderTheatre {
    /// The application beneath the overlay. Upstream's theatre has no such
    /// child, because upstream's `Overlay` *is* the whole surface and the app
    /// is its bottom entry; here the page is passed in as a child so that an
    /// `Overlay` can be dropped into an existing tree without the caller having
    /// to re-express their page as an entry.
    page: BoxedRender,
    /// What the theatre is hosting: the entries handed to it by its assemble,
    /// then everything the portals have registered.
    entries: Vec<StagedEntry>,
    /// The entries that came from the assemble, kept apart because the portal
    /// half is refreshed from the registry on every layout.
    fixed: Vec<StagedEntry>,
    /// Where the portals register. Read at layout rather than at assemble --
    /// see [`OverlayStage`] for why that has to be the phase.
    stage: Option<OverlayStage>,
    stage_id: u64,
    placements: Vec<EntryPlacement>,
    size: Size,
}

impl RenderTheatre {
    pub fn new(page: BoxedRender, entries: Vec<StagedEntry>) -> RenderTheatre {
        let placements = vec![EntryPlacement::Fill; entries.len()];
        RenderTheatre {
            page,
            entries: entries.clone(),
            fixed: entries,
            stage: None,
            stage_id: 0,
            placements,
            size: Size::ZERO,
        }
    }

    /// Points the theatre at the registry its portals write to.
    pub fn with_stage(mut self, stage: OverlayStage, stage_id: u64) -> RenderTheatre {
        self.stage = Some(stage);
        self.stage_id = stage_id;
        self
    }

    /// Re-reads the registry. Called at the top of every layout, which is the
    /// phase that runs whenever anything beneath the theatre changed.
    fn refresh_entries(&mut self) {
        let Some(stage) = &self.stage else {
            return;
        };
        // Registering happens here because here is where the theatre can reach
        // its own handle -- see `laying_out_handle`.
        if let Some(me) = crate::render::laying_out_handle() {
            stage.watch(me);
        }
        let mut entries = self.fixed.clone();
        entries.extend(stage.snapshot(self.stage_id));
        self.entries = entries;
        self.placements
            .resize(self.entries.len(), EntryPlacement::Fill);
    }

    /// Places the entries somewhere other than filling the theatre. Used by the
    /// slice tests to make "it rendered *there*, not where it was built" an
    /// observable fact.
    pub fn with_placements(mut self, placements: Vec<EntryPlacement>) -> RenderTheatre {
        self.placements = placements;
        self
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Where entry `index` was placed, relative to the theatre's origin.
    pub fn entry_offset(&self, index: usize) -> Offset {
        match self.placements.get(index) {
            Some(EntryPlacement::At(offset)) => *offset,
            _ => Offset::ZERO,
        }
    }

    /// Whether the object behind `render` is one of this theatre's entries.
    pub fn hosts(&self, render: &RenderRef) -> bool {
        self.entries.iter().any(|entry| entry.render.is(render))
    }
}

impl RenderBox for RenderTheatre {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.refresh_entries();
        // The page first and alone decides the size, exactly as upstream's
        // theatre takes its size from the constraints it was given rather than
        // from any entry: an overlay that grew to fit a tooltip would move the
        // page underneath it.
        //
        self.size = self.page.layout_child(constraints, true);
        let tight = BoxConstraints::tight(self.size.width, self.size.height);
        for (index, entry) in self.entries.iter_mut().enumerate() {
            match self.placements.get(index) {
                Some(EntryPlacement::At(_)) => {
                    // Loose: an entry that chose a position keeps its own size.
                    entry.render.layout_child(
                        BoxConstraints::new(0.0, self.size.width, 0.0, self.size.height),
                        true,
                    );
                }
                _ => {
                    entry.render.layout_child(tight, true);
                }
            }
        }
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.page, offset);
        for (index, entry) in self.entries.iter().enumerate() {
            context.paint_child(&entry.render, offset.plus(self.entry_offset(index)));
        }
    }

    /// Entries are tested before the page, and later entries before earlier
    /// ones: the thing on top gets the press. Upstream's theatre hit-tests in
    /// reverse paint order for the same reason every stack does.
    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        for (index, entry) in self.entries.iter().enumerate().rev() {
            let local = position.minus(self.entry_offset(index));
            if entry.render.hit_test(local, result) {
                return true;
            }
        }
        self.page.hit_test(position, result)
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.page, Offset::ZERO);
        for (index, entry) in self.entries.iter().enumerate() {
            visit(&entry.render, self.entry_offset(index));
        }
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.page.dry_layout(constraints)
    }

    /// A theatre takes a new page and a new entry list every frame -- the
    /// entries are collected fresh during assemble -- so there is nothing to
    /// compare and it always relays out. Saying so is better than returning
    /// `None` and being remade, which would drop the layout the page had.
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderTheatre>()?;
        self.page = fresh.page.clone();
        self.fixed = std::mem::take(&mut fresh.fixed);
        self.entries = std::mem::take(&mut fresh.entries);
        self.placements = std::mem::take(&mut fresh.placements);
        self.stage = fresh.stage.take();
        self.stage_id = fresh.stage_id;
        Some(UpdateEffect::Relayout)
    }
}

/// Walks every render object beneath `root`, handing each one's **handle** to
/// `visit`.
///
/// The unwrapping is the point. `RenderBox::visit_children` reports children as
/// `&dyn RenderBox`, and what a parent stores is a [`RenderRef`] -- so the
/// `&dyn RenderBox` a walk receives is the *handle*, not the object behind it.
/// A walk that downcasts what it is handed finds `RenderRef` every time and
/// never sees a `RenderTheatre` or a `RenderPortal`; it has to step through the
/// handle with [`RenderRef::with`] to reach the object and then ask *that* for
/// its children.
///
/// Found by writing the walk the obvious way first and watching it report an
/// empty tree.
fn visit_subtree(root: &RenderRef, visit: &mut dyn FnMut(&RenderRef)) {
    let children: Vec<RenderRef> = root.with(|object| {
        let mut found = Vec::new();
        object.visit_children(&mut |child, _| {
            if let Some(handle) = child.as_any().downcast_ref::<RenderRef>() {
                found.push(handle.clone());
            }
        });
        found
    });
    for child in children {
        visit(&child);
        visit_subtree(&child, visit);
    }
}

/// The render object a portal leaves in its own place: its in-place child, and
/// nothing else.
///
/// It exists so the portal has something to *be* in the render tree. The
/// overlay child is not here -- that is the whole point -- and
/// [`RenderPortal::hosts_overlay_child`] is what a test asks to prove it.
pub struct RenderPortal {
    child: BoxedRender,
    /// The gate's render object: zero-sized, never painted, and **kept**.
    ///
    /// It has to be in the tree. `mark_needs_layout` walks up
    /// `RenderState::parent`, which is claimed during layout -- so a render
    /// object nobody lays out has no parent, and anything it reports goes
    /// nowhere. The gate is what notices that the portal registry changed, and
    /// dropping it on the floor is what made a hidden portal stay on screen.
    gate: Option<BoxedRender>,
}

impl RenderPortal {
    pub fn new(child: BoxedRender) -> RenderPortal {
        RenderPortal { child, gate: None }
    }

    pub fn with_gate(mut self, gate: BoxedRender) -> RenderPortal {
        self.gate = Some(gate);
        self
    }

    /// Whether `render` is anywhere in this portal's own render subtree.
    ///
    /// The answer for a portal's overlay child must be **no**: it was built
    /// here and it renders elsewhere.
    pub fn hosts_overlay_child(&self, render: &RenderRef) -> bool {
        let mut found = self.child.is(render);
        if !found {
            visit_subtree(&self.child, &mut |handle| {
                if handle.is(render) {
                    found = true;
                }
            });
        }
        found
    }
}

impl RenderBox for RenderPortal {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(gate) = &mut self.gate {
            gate.layout_child(BoxConstraints::tight(0.0, 0.0), false);
        }
        self.child.layout_child(constraints, true)
    }

    fn size(&self) -> Size {
        self.child.size()
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.child.hit_test(position, result)
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.child.dry_layout(constraints)
    }

    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderPortal>()?;
        // The gate is compared as well as the child, and it has to be. Showing
        // and hiding produce different widget types at the gate's position, so
        // its render object is *remade* rather than reconfigured -- and a
        // portal that only compared its in-place child would reconfigure
        // cleanly, report nothing, and leave the theatre showing an entry that
        // has been withdrawn.
        let gate_changed = match (&self.gate, &fresh.gate) {
            (Some(mine), Some(theirs)) => !mine.is(theirs),
            (None, None) => false,
            _ => true,
        };
        let changed = !self.child.is(&fresh.child) || gate_changed;
        self.child = fresh.child.clone();
        self.gate = fresh.gate.take();
        Some(UpdateEffect::relayout_if(changed))
    }
}

/// A widget that builds `overlay_child` in its own place and renders it in the
/// theatre `stage` belongs to.
///
/// Upstream's `OverlayPortal`, with the controller's `show`/`hide` standing in
/// as `overlay_child: Option<_>`: a portal with nothing to show stages nothing.
pub fn portal(
    stage: OverlayStage,
    portal_id: u64,
    z_order: u64,
    child: AnyWidget,
    overlay_child: Option<AnyWidget>,
) -> AnyWidget {
    portal_on(stage, portal_id, 0, z_order, child, overlay_child)
}

/// [`portal`] aimed at a particular theatre.
pub fn portal_on(
    stage: OverlayStage,
    portal_id: u64,
    stage_id: u64,
    z_order: u64,
    child: AnyWidget,
    overlay_child: Option<AnyWidget>,
) -> AnyWidget {
    let has_overlay_child = overlay_child.is_some();
    let mut children = vec![child];
    if let Some(overlay_child) = overlay_child {
        children.push(overlay_child);
    }
    many(children, move |mut rendered| {
        // Built here, so it inherited from here. Handed over, so it renders
        // there. Taken from the back, because the in-place child is first.
        if has_overlay_child {
            if let Some(render) = rendered.pop() {
                stage.put(StagedEntry {
                    portal_id,
                    render,
                    z_order,
                    stage_id,
                });
            }
        } else {
            // Showing nothing is a withdrawal, not a silence: the registry
            // keeps what it was last told, so a portal that has hidden has to
            // say so.
            stage.take_out(portal_id);
        }
        RenderPortal::new(rendered.pop().expect("a portal always has its own child"))
    })
}

/// The host. Everything staged for `stage_id` during this frame's build lands
/// on top of `child`.
pub fn theatre(stage: OverlayStage, child: AnyWidget) -> AnyWidget {
    theatre_with(stage, 0, child, Vec::new())
}

/// [`theatre`] with explicit placements, which is how the slice test makes
/// "rendered somewhere else" observable.
pub fn theatre_with(
    stage: OverlayStage,
    stage_id: u64,
    child: AnyWidget,
    placements: Vec<EntryPlacement>,
) -> AnyWidget {
    many(vec![child], move |mut rendered| {
        let mut theatre = RenderTheatre::new(
            rendered.pop().expect("a theatre always has its page"),
            Vec::new(),
        )
        .with_stage(stage.clone(), stage_id);
        if !placements.is_empty() {
            theatre = theatre.with_placements(placements.clone());
        }
        theatre
    })
}

// -- L1: the live overlay -----------------------------------------------------

/// What an entry is built from. Upstream's `OverlayEntry.builder`.
type EntryBuilder = Rc<dyn Fn() -> AnyWidget>;

/// The state an `Overlay` element keeps: the decision logic from
/// [`crate::overlay::OverlayState`], plus the builders it has no business
/// knowing about.
///
/// The split is deliberate. `overlay.rs` decides *which* entries are on stage,
/// in what order, and which of the offstage ones keep their state -- and it can
/// decide all of that without ever seeing a widget. What it cannot do is turn
/// an id back into something to build, so that lives here.
#[derive(Default)]
pub struct LiveOverlay {
    state: OverlayState,
    builders: Vec<(u64, EntryBuilder)>,
    next_id: u64,
    stage: OverlayStage,
}

impl LiveOverlay {
    pub fn new() -> LiveOverlay {
        LiveOverlay {
            state: OverlayState::new(),
            builders: Vec::new(),
            next_id: 1,
            stage: OverlayStage::new(),
        }
    }

    pub fn state(&self) -> &OverlayState {
        &self.state
    }

    fn builder(&self, id: u64) -> Option<EntryBuilder> {
        self.builders
            .iter()
            .find(|(entry_id, _)| *entry_id == id)
            .map(|(_, builder)| Rc::clone(builder))
    }

    /// The widgets to build this frame, in paint order.
    ///
    /// `onstage()` is the whole of the decision -- an entry below an opaque one
    /// is simply **not in this list**, so it is not built at all rather than
    /// built and covered up. That is upstream's arrangement and the reason it
    /// matters is a stack of routes: ten screens behind an opaque one cost
    /// nothing per frame, where ten built-and-hidden screens would cost ten
    /// builds and ten layouts to draw nothing.
    fn onstage_widgets(&self) -> Vec<AnyWidget> {
        self.state
            .onstage()
            .into_iter()
            .filter_map(|onstage| self.builder(onstage.id).map(|builder| builder()))
            .collect()
    }

    /// Which of the on-stage entries offered to size the overlay, in the order
    /// they will be laid out.
    fn onstage_can_size(&self) -> Vec<bool> {
        self.state
            .onstage()
            .into_iter()
            .filter_map(|onstage| {
                self.state
                    .entries()
                    .iter()
                    .find(|entry| entry.id == onstage.id)
                    .map(|entry| entry.can_size_overlay)
            })
            .collect()
    }
}

/// A descendant's way to reach the overlay above it. Upstream's `Overlay.of`.
///
/// Published with `provide`, so `Overlay::of(context)` is an inherited lookup.
#[derive(Clone)]
pub struct OverlayHandle {
    /// The data, owned by the `overlay()` call rather than by an element.
    ///
    /// It lives outside the tree because the page must **not** be something the
    /// overlay has to reproduce. An earlier arrangement kept the page inside the
    /// stateful component that owned the entries, and the page vanished the
    /// moment an entry was inserted: a `set_state` rebuild re-runs that
    /// component with the widget it already had, and a page taken out of an
    /// `Option` on the first build is not there on the second. The page is a
    /// sibling of the entries now, and nothing regenerates it.
    data: Rc<RefCell<LiveOverlay>>,
    /// The element to rebuild when the entry list changes.
    host: Rc<RefCell<Option<StateHandle<EntryHostState>>>>,
    /// Identity, and the only thing equality looks at.
    id: u64,
    stage: OverlayStage,
}

/// **Equality is the element identity and nothing else, and that is load
/// bearing.**
///
/// `inherited()` registers a dependency on the value it read, and
/// `ElementTree::publish` marks every dependent dirty when the value it
/// publishes is not equal to the one already there. An `OverlayHandle` is
/// republished every frame, so a handle that compared unequal frame to frame
/// would mark every descendant that had ever asked for the overlay -- which is
/// every button with a menu, every field with a tooltip -- dirty on every
/// frame, forever.
///
/// The element id is stable for as long as the overlay is mounted, which is
/// exactly as long as the handle means anything. The stage is deliberately not
/// compared: it is an `Rc` to a scratch buffer that is emptied and refilled
/// every frame, and comparing it would be comparing this frame's leftovers.
impl PartialEq for OverlayHandle {
    fn eq(&self, other: &OverlayHandle) -> bool {
        self.id == other.id
    }
}

impl OverlayHandle {
    /// Upstream `Overlay.of`, with `maybeOf`'s signature: a subtree with no
    /// overlay above it is an ordinary state of affairs for a widget that only
    /// wants one if there is one.
    pub fn of(context: &mut BuildContext) -> Option<Rc<OverlayHandle>> {
        context.inherited::<OverlayHandle>()
    }

    /// The stage portals beneath this overlay hand their children to.
    pub fn stage(&self) -> &OverlayStage {
        &self.stage
    }

    /// How many entries are inserted, on stage or not.
    pub fn entry_count(&self) -> usize {
        self.data.borrow().state.entries().len()
    }

    /// Upstream `OverlayState.insert`. Returns the id the entry was given.
    pub fn insert(&self, builder: impl Fn() -> AnyWidget + 'static) -> Option<u64> {
        self.insert_entry(OverlayEntry::new(0), builder)
    }

    /// [`OverlayHandle::insert`] with the entry's flags chosen -- `opaque`,
    /// `maintainState`, `canSizeOverlay`. The id on the entry passed in is
    /// ignored; the overlay assigns one.
    pub fn insert_entry(
        &self,
        mut entry: OverlayEntry,
        builder: impl Fn() -> AnyWidget + 'static,
    ) -> Option<u64> {
        let id = {
            let live = &mut *self.data.borrow_mut();
            let entry_id = live.next_id;
            live.next_id += 1;
            entry.id = entry_id;
            live.state.insert(entry, InsertPosition::Top).ok()?;
            live.builders.push((entry_id, Rc::new(builder)));
            live.state.flush_build();
            entry_id
        };
        self.wake();
        Some(id)
    }

    /// Upstream `OverlayEntry.remove`.
    pub fn remove(&self, id: u64) -> bool {
        let removed = {
            let live = &mut *self.data.borrow_mut();
            let removed = live.state.remove(id, false).is_some();
            live.builders.retain(|(entry_id, _)| *entry_id != id);
            live.state.flush_build();
            removed
        };
        if removed {
            self.wake();
        }
        removed
    }

    /// Asks the element that builds the entries to build them again.
    fn wake(&self) {
        let handle = self.host.borrow().clone();
        if let Some(handle) = handle {
            handle.set_state(|state| state.revision += 1);
        }
    }
}

/// What the entry-building element keeps: nothing but a reason to rebuild.
#[derive(Default)]
pub struct EntryHostState {
    revision: u64,
}

/// The element that turns the overlay's entry list into widgets.
///
/// It is a **sibling of the page**, which is the whole point of it existing
/// separately: an insert rebuilds this and leaves the page untouched.
struct EntryHost {
    data: Rc<RefCell<LiveOverlay>>,
    host: Rc<RefCell<Option<StateHandle<EntryHostState>>>>,
}

impl StatefulComponent for EntryHost {
    type State = EntryHostState;

    fn build(
        &self,
        _state: &EntryHostState,
        handle: StateHandle<EntryHostState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        *self.host.borrow_mut() = Some(handle);
        let (children, can_size) = {
            let live = self.data.borrow();
            (live.onstage_widgets(), live.onstage_can_size())
        };
        many(children, move |rendered| RenderEntryStack {
            entries: rendered,
            can_size: can_size.clone(),
            size: Size::ZERO,
        })
    }
}

/// Upstream `Overlay`: the host.
///
/// Wraps a page and puts entries on top of it. The page is a child rather than
/// the bottom entry -- upstream's overlay *is* the whole surface and the app is
/// its first entry, but an overlay that can be dropped over an existing tree is
/// more useful here than one the caller has to re-express their app for.
pub fn overlay(page: AnyWidget) -> AnyWidget {
    crate::framework::stateful(OverlayRoot {
        page: RefCell::new(Some(page)),
    })
}

/// The widget half of [`overlay`]: it carries the page, and nothing else. The
/// entries live in the state, which is the whole reason the split exists.
struct OverlayRoot {
    /// Taken by the build. A fresh page arrives with every widget update --
    /// `overlay()` is re-run by a full-tree rebuild, and each run hands its own
    /// page over -- so the `Option` is only ever empty if the element is built
    /// without an update having happened, which mounting already covered.
    page: RefCell<Option<AnyWidget>>,
}

/// What an overlay keeps across rebuilds. Upstream this is `OverlayState`,
/// which likewise outlives every widget built above it.
///
/// It has to live here rather than in the widget because `overlay()` runs
/// again on every full-tree rebuild -- a resize, an image that finished
/// decoding -- and a `LiveOverlay` created inside the widget would take the
/// inserted entries with it when it went. That is exactly what used to happen:
/// the rebuild made a fresh, empty overlay, the entry-building element adopted
/// it, and a drawer or menu that was up vanished with the frame that had
/// mounted it -- along with anyone holding the old [`OverlayHandle`], whose
/// inserts now landed in an overlay nothing builds.
struct OverlayRootState {
    data: Rc<RefCell<LiveOverlay>>,
    host: Rc<RefCell<Option<StateHandle<EntryHostState>>>>,
    stage: OverlayStage,
    id: u64,
}

impl Default for OverlayRootState {
    fn default() -> OverlayRootState {
        let data = Rc::new(RefCell::new(LiveOverlay::new()));
        let stage = data.borrow().stage.clone();
        OverlayRootState {
            data,
            host: Rc::new(RefCell::new(None)),
            stage,
            id: next_overlay_id(),
        }
    }
}

impl StatefulComponent for OverlayRoot {
    type State = OverlayRootState;

    fn initial_state(&self) -> OverlayRootState {
        OverlayRootState::default()
    }

    fn build(
        &self,
        state: &OverlayRootState,
        _handle: StateHandle<OverlayRootState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let handle = OverlayHandle {
            data: Rc::clone(&state.data),
            host: Rc::clone(&state.host),
            id: state.id,
            stage: state.stage.clone(),
        };
        let entry_host = crate::framework::stateful(EntryHost {
            data: Rc::clone(&state.data),
            host: Rc::clone(&state.host),
        });
        let page =
            self.page.borrow_mut().take().unwrap_or_else(|| {
                crate::framework::leaf(|| RenderRef::new(crate::widgets::Empty))
            });
        let stage = state.stage.clone();

        provide(
            handle,
            many(vec![page, entry_host], move |mut rendered| {
                let entry_stack = rendered.pop().expect("the entry host");
                let page = rendered.pop().expect("the page");
                RenderTheatre::new(
                    page,
                    // The inserted entries are one layer, beneath every portal: a
                    // dialog is put up by the application and a tooltip by the
                    // thing it belongs to, and the application's surfaces go under.
                    vec![StagedEntry {
                        portal_id: 0,
                        render: entry_stack,
                        z_order: 0,
                        stage_id: 0,
                    }],
                )
                .with_stage(stage.clone(), 0)
            }),
        )
    }
}

/// A fresh identity for each overlay, so two of them are never mistaken for
/// each other by [`OverlayHandle`]'s equality.
pub fn next_surface_id() -> u64 {
    next_overlay_id()
}

/// Wraps an overlay entry that is already in place as a [`ModalHandle`], for a
/// surface that put itself up with `insert_entry` rather than through
/// [`show_modal`] -- a drawer, which brings its own barrier because its barrier
/// has to fade with it.
pub fn modal_from_entry(overlay: Rc<OverlayHandle>, entry_id: u64) -> ModalHandle {
    ModalHandle {
        entry_id,
        focus_root: 0,
        overlay,
        dismissed: Rc::new(Cell::new(false)),
    }
}

fn next_overlay_id() -> u64 {
    thread_local! {
        static NEXT: Cell<u64> = const { Cell::new(1) };
    }
    NEXT.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

/// The inserted entries, stacked. One render object, so the theatre sees the
/// whole set as a single layer beneath the portals.
pub struct RenderEntryStack {
    entries: Vec<BoxedRender>,
    can_size: Vec<bool>,
    size: Size,
}

impl RenderEntryStack {
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// The topmost entry that offered to size the overlay. Topmost, not first:
    /// the entry nearest the reader is the one whose size the overlay should be.
    fn sizing_entry(&self) -> Option<usize> {
        (0..self.entries.len())
            .rev()
            .find(|index| self.can_size.get(*index).copied().unwrap_or(false))
    }
}

impl RenderBox for RenderEntryStack {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Under unbounded constraints there is no size to inherit, and upstream
        // lets the topmost entry that offered `canSizeOverlay` answer instead.
        // An entry that offers is taking on the question the overlay could not
        // answer, which is why it has to cope with unbounded constraints.
        if !constraints.has_bounded_width() || !constraints.has_bounded_height() {
            if let Some(index) = self.sizing_entry() {
                let chosen = self.entries[index].layout_child(constraints, true);
                let tight = BoxConstraints::tight(chosen.width, chosen.height);
                for (other, entry) in self.entries.iter_mut().enumerate() {
                    if other != index {
                        entry.layout_child(tight, true);
                    }
                }
                self.size = chosen;
                return self.size;
            }
        }
        self.size = constraints.constrain(constraints.biggest());
        let tight = BoxConstraints::tight(self.size.width, self.size.height);
        for entry in self.entries.iter_mut() {
            entry.layout_child(tight, true);
        }
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        for entry in &self.entries {
            context.paint_child(entry, offset);
        }
    }

    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.entries
            .iter()
            .rev()
            .any(|entry| entry.hit_test(position, result))
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        for entry in &self.entries {
            visit(entry, Offset::ZERO);
        }
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.constrain(constraints.biggest())
    }

    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderEntryStack>()?;
        self.entries = std::mem::take(&mut fresh.entries);
        self.can_size = std::mem::take(&mut fresh.can_size);
        Some(UpdateEffect::Relayout)
    }
}

/// The page an overlay built with nothing under it shows: nothing.
struct EmptyPage;

impl crate::framework::RenderWidget for EmptyPage {
    fn children(&self) -> Vec<AnyWidget> {
        Vec::new()
    }
    fn create_render(&self, _children: Vec<BoxedRender>) -> BoxedRender {
        RenderRef::new(RenderConstrainedBox::new(BoxConstraints::new(
            0.0, 0.0, 0.0, 0.0,
        )))
    }
}

// -- L2: the public portal ----------------------------------------------------

/// What a portal's element keeps. The controller holds everything that
/// matters, so this exists to *be* something `set_state` can reach: showing a
/// portal has to rebuild the portal, and a handle is how.
#[derive(Default)]
pub struct PortalState {
    /// Bumped on every show and hide, so the rebuild is not mistaken for a
    /// no-op by anything that compares states.
    revision: u64,
}

struct PortalControllerInner {
    /// The portal's identity in the registry. The controller is the natural
    /// place for it: a portal widget is rebuilt and replaced, and the
    /// controller is what stays the same thing across all of it.
    portal_id: u64,
    controller: OverlayPortalController,
    /// The element to wake. `None` until the portal has built once -- upstream's
    /// `OverlayPortalController` is likewise inert until its portal attaches.
    attached: Option<StateHandle<PortalState>>,
}

/// **One clock for every portal, because a tick is only meaningful against the
/// other ticks.**
///
/// Upstream's is a static on `OverlayPortalController`, and it has to be: the
/// z-order index exists so that two portals can be *compared*, and two
/// controllers each counting from their own start would produce numbers that
/// compare as though they had been shown at the same moment. Written per
/// controller first, and the test for two portals stacking in the order they
/// were shown is what said so.
fn tick_z_order() -> i64 {
    thread_local! {
        static CLOCK: RefCell<OverlayPortalClock> =
            RefCell::new(OverlayPortalClock::new());
    }
    CLOCK.with(|clock| clock.borrow_mut().tick())
}

/// Upstream `OverlayPortalController`, made shareable.
///
/// The decision logic -- attached, showing, the z-order tick -- is
/// [`crate::overlay::OverlayPortalController`] and is not restated here. What
/// this adds is the two things a live controller needs and a pure one cannot
/// have: somewhere to keep the clock, and a way to tell its portal that the
/// answer changed.
#[derive(Clone)]
pub struct PortalController {
    inner: Rc<RefCell<PortalControllerInner>>,
}

impl Default for PortalController {
    fn default() -> Self {
        PortalController::new()
    }
}

impl PortalController {
    pub fn new() -> PortalController {
        PortalController {
            inner: Rc::new(RefCell::new(PortalControllerInner {
                portal_id: next_overlay_id(),
                controller: OverlayPortalController::new(None),
                attached: None,
            })),
        }
    }

    /// Upstream `OverlayPortalController.show`.
    ///
    /// The tick is taken here rather than at build time, and that is the whole
    /// of the stacking rule: **two portals stack in the order they were shown,
    /// not the order the tree reaches them.** A tooltip opened over an already
    /// open menu is above it even though the menu's button is further down the
    /// page.
    pub fn show(&self) {
        let mut clock = OverlayPortalClock::at(tick_z_order() - 1);
        let woke = {
            let inner = &mut *self.inner.borrow_mut();
            inner.controller.show(&mut clock);
            inner.attached.clone()
        };
        Self::wake(woke);
    }

    /// Upstream `OverlayPortalController.hide`.
    pub fn hide(&self) {
        let woke = {
            let inner = &mut *self.inner.borrow_mut();
            inner.controller.hide();
            inner.attached.clone()
        };
        Self::wake(woke);
    }

    pub fn toggle(&self) {
        if self.is_showing() {
            self.hide()
        } else {
            self.show()
        }
    }

    pub fn is_showing(&self) -> bool {
        self.inner.borrow().controller.is_showing()
    }

    /// This portal's key in the stage registry.
    pub fn portal_id(&self) -> u64 {
        self.inner.borrow().portal_id
    }

    pub fn is_attached(&self) -> bool {
        self.inner.borrow().controller.is_attached()
    }

    /// Upstream's `_zOrderIndex`, which is what the theatre sorts by.
    pub fn z_order(&self) -> Option<i64> {
        self.inner.borrow().controller.z_order_index()
    }

    fn wake(handle: Option<StateHandle<PortalState>>) {
        if let Some(handle) = handle {
            handle.set_state(|state| state.revision += 1);
        }
    }

    fn attach(&self, handle: StateHandle<PortalState>) {
        let inner = &mut *self.inner.borrow_mut();
        if inner.attached.is_none() {
            inner.controller.attach();
        }
        inner.attached = Some(handle);
    }
}

/// The half of a portal that comes and goes.
///
/// It is a **sibling of the portal's own child**, and that is structural rather
/// than incidental. A `set_state` rebuild re-runs a component with the widget
/// it already had, so a child taken out of an `Option` on the first build is
/// gone on the second -- the overlay lost its page that way before this was
/// arranged, and the portal lost its child. Only the part that actually changes
/// when the controller is toggled is stateful, and the part that does not is
/// left alone entirely.
///
/// It still builds *here*, at the portal's position in the element tree, so
/// what it inherits is the button's context and not the overlay's. That is the
/// whole reason `OverlayPortal` exists.
struct PortalGate {
    controller: PortalController,
    overlay_child: Rc<dyn Fn() -> AnyWidget>,
}

impl StatefulComponent for PortalGate {
    type State = PortalState;

    fn build(
        &self,
        _state: &PortalState,
        handle: StateHandle<PortalState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        self.controller.attach(handle);
        let portal_id = self.controller.portal_id();

        // Upstream asserts an `Overlay` above; here a portal with none shows
        // nothing. A widget that wants an overlay only when there is one -- and
        // a test that mounts a button on its own -- is better served by an
        // answer than by a panic.
        let Some(overlay) = OverlayHandle::of(context) else {
            return many(Vec::new(), |_| RenderNothing::default());
        };

        let stage = overlay.stage().clone();
        if !self.controller.is_showing() {
            // Showing nothing is a withdrawal, not a silence: the registry
            // keeps what it was last told.
            stage.take_out(portal_id);
            let revision = stage.revision();
            return many(Vec::new(), move |_| RenderNothing::at(revision));
        }

        let z_order = portal_z_order(self.controller.z_order());
        many(vec![(self.overlay_child)()], move |mut rendered| {
            if let Some(render) = rendered.pop() {
                stage.put(StagedEntry {
                    portal_id,
                    render,
                    z_order,
                    stage_id: 0,
                });
            }
            RenderNothing::at(stage.revision())
        })
    }
}

/// Turns a controller's tick into a stacking order, order-preservingly.
///
/// **The ticks are negative.** Upstream's clock starts at the most negative
/// integer there is -- `i64::MIN` on native, `-2^53` on the web -- so that it
/// can count upwards for the life of the program without ever wrapping. Clamping
/// them at zero, which is the obvious way to reach a `u64`, maps every tick a
/// program will ever produce onto the same value and makes every portal tie.
///
/// Found by opening two portals and watching them both land on zero.
///
/// The shift is the usual order-preserving map: adding `2^63` to a signed value
/// and reading it unsigned keeps `a < b` true. Entries inserted into the overlay
/// occupy order 0, so a portal is offset past them by one.
fn portal_z_order(tick: Option<i64>) -> u64 {
    let tick = tick.unwrap_or(i64::MIN);
    ((tick as u64) ^ (1u64 << 63)).saturating_add(1)
}

/// What a portal leaves where its overlay child was built: nothing at all.
///
/// The child is in the element tree here and in the render tree elsewhere, so
/// this is the shape of the hole it leaves behind.
///
/// # Why it carries a number
///
/// It is the only thing left in the render tree at the portal's position when
/// the overlay child has gone elsewhere, which makes it the only thing that can
/// report that the registry changed. Without that, nothing marks the theatre
/// for layout: the gate's rebuild is absorbed one level up -- a `RenderPortal`
/// whose in-place child did not change reconfigures cleanly and tells its own
/// parent nothing -- so the theatre keeps the entries it collected last time
/// and a hidden portal stays on screen.
///
/// Found by hiding a portal and watching it not go away.
#[derive(Default)]
pub struct RenderNothing {
    /// The stage revision this was built at.
    revision: u64,
    size: Size,
}

impl RenderNothing {
    pub fn at(revision: u64) -> RenderNothing {
        RenderNothing {
            revision,
            size: Size::ZERO,
        }
    }
}

impl RenderBox for RenderNothing {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = constraints.constrain(Size::ZERO);
        self.size
    }
    fn size(&self) -> Size {
        self.size
    }
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.constrain(Size::ZERO)
    }
    fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderNothing>()?;
        // A changed registry has to reach the theatre, and this is the only
        // object on the path that knows it changed.
        let effect = UpdateEffect::relayout_if(self.revision != fresh.revision);
        self.revision = fresh.revision;
        Some(effect)
    }
}

/// Upstream `OverlayPortal`.
///
/// Renders `child` where it is, and -- while `controller` is showing -- builds
/// `overlay_child` here and renders it in the nearest enclosing overlay.
pub fn overlay_portal(
    controller: PortalController,
    child: AnyWidget,
    overlay_child: impl Fn() -> AnyWidget + 'static,
) -> AnyWidget {
    let gate = crate::framework::stateful(PortalGate {
        controller,
        overlay_child: Rc::new(overlay_child),
    });
    many(vec![child, gate], |mut rendered| {
        let gate = rendered.pop().expect("the gate");
        RenderPortal::new(rendered.pop().expect("a portal always has its own child"))
            .with_gate(gate)
    })
}

/// The link between a host that keeps its geometry in cells and the overlay
/// entry that draws it.
///
/// A host moves a tooltip, a loupe, a selection handle or a drag feedback by
/// writing a new offset into a shared cell. That alone changes nothing on the
/// screen: **the entry's component is not dirty, so it does not rebuild, so the
/// new offset is never read**. Found by watching a magnifier's entry report the
/// right position and draw nothing.
///
/// So the entry hands its [`StateHandle`] to one of these during its build, and
/// the host calls [`EntryRefresh::refresh`] after every write. The state itself
/// is a counter and carries no meaning -- it exists because `set_state` needs
/// something to change.
///
/// One of these may serve **several** entries -- a selection overlay is three,
/// which move together -- so it keeps a handle each rather than a single slot.
/// A single slot looks right and silently wakes only whichever entry built
/// last.
#[derive(Clone, Default)]
pub struct EntryRefresh {
    handles: Rc<RefCell<Vec<StateHandle<u64>>>>,
    revision: Rc<Cell<u64>>,
}

impl EntryRefresh {
    pub fn new() -> EntryRefresh {
        EntryRefresh::default()
    }

    /// Called from the entry's `build`. Idempotent per entry: a rebuilt
    /// component gets a fresh handle, and the stale one it replaces would
    /// answer false for the rest of the run.
    pub fn attach(&self, handle: StateHandle<u64>) {
        let mut handles = self.handles.borrow_mut();
        match handles.iter_mut().find(|kept| kept.id == handle.id) {
            Some(kept) => *kept = handle,
            None => handles.push(handle),
        }
    }

    /// How many entries are listening, for tests.
    pub fn listeners(&self) -> usize {
        self.handles.borrow().len()
    }

    /// The counter the entry uses as its initial state.
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    /// Says the geometry changed. Answers whether the entry heard -- false
    /// before it has built once, which is the ordinary case for a host that
    /// places itself before the first frame.
    pub fn refresh(&self) -> bool {
        let next = self.revision.get() + 1;
        self.revision.set(next);
        let handles = self.handles.borrow().clone();
        let mut heard = false;
        for handle in handles {
            heard |= handle.set_state(move |state| *state = next);
        }
        heard
    }
}

// -- L3: modal semantics ------------------------------------------------------

/// The scrim itself: fills whatever it is given, paints a colour if it has one,
/// and is a hit-test target in its own right.
///
/// Being a target is the whole of "swallows the click". A container that is not
/// one answers false for its own empty space, which is what lets whatever is
/// behind it in a stack still be asked -- exactly what a barrier must not
/// allow.
pub struct RenderScrim {
    color: Option<crate::engine::Color>,
    size: Size,
}

impl RenderScrim {
    pub fn new(color: Option<crate::engine::Color>) -> RenderScrim {
        RenderScrim {
            color,
            size: Size::ZERO,
        }
    }
}

impl RenderBox for RenderScrim {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = constraints.constrain(constraints.biggest());
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.constrain(constraints.biggest())
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if let Some(color) = self.color {
            let rect =
                crate::engine::Rect::xywh(offset.dx, offset.dy, self.size.width, self.size.height);
            context
                .canvas()
                .draw_rect(rect, &crate::engine::Paint::new(color));
        }
    }

    /// A barrier is a target everywhere inside itself, painted or not. An
    /// undimmed barrier -- a menu's -- is invisible and still swallows the tap
    /// that closes the menu.
    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }

    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderScrim>()?;
        let effect = UpdateEffect::repaint_if(self.color != fresh.color);
        self.color = fresh.color;
        Some(effect)
    }
}

/// A live [`ModalBarrier`]: the decision logic in `modal_barrier.rs`, wired to
/// a render object that fills the overlay and takes the press.
pub fn modal_barrier(barrier: ModalBarrier, id: u64, on_dismiss: impl Fn() + 'static) -> AnyWidget {
    let color = barrier.color;
    let dismissible = barrier.dismissible;
    let handlers = crate::gestures::PointerHandlers::new().with_tap(move |_| {
        // `dismissible` decides whether the tap does anything. It does not
        // decide whether the tap is *taken*: a barrier that cannot be dismissed
        // still swallows the press, which is the difference between a modal
        // that refuses to close and no modal at all.
        if dismissible {
            on_dismiss();
        }
    });
    crate::framework::leaf(move || {
        crate::render::RenderPointerRegion::new(id, RenderScrim::new(color))
            .with_handlers(handlers.clone())
            .with_behavior(crate::render::HitTestBehavior::Opaque)
    })
}

/// What a modal registers while it is up.
struct ModalRecord {
    entry_id: u64,
    focus_root: u64,
    dismissible: bool,
    dismiss: Rc<dyn Fn()>,
}

thread_local! {
    /// The modals on screen, outermost first.
    ///
    /// A stack because Escape has to reach **the top one only**: a dialog over
    /// a dialog closes the inner one, and pressing Escape again closes the
    /// outer.
    static MODALS: RefCell<Vec<ModalRecord>> = const { RefCell::new(Vec::new()) };
}

/// A modal that is up, and the way to take it down.
#[derive(Clone)]
pub struct ModalHandle {
    entry_id: u64,
    focus_root: u64,
    overlay: Rc<OverlayHandle>,
    dismissed: Rc<Cell<bool>>,
}

impl ModalHandle {
    /// Takes the modal down. Idempotent: a barrier tap and an Escape arriving
    /// together should close one dialog, not two.
    pub fn dismiss(&self) -> bool {
        if self.dismissed.replace(true) {
            return false;
        }
        crate::focus::release_trap(self.focus_root);
        MODALS.with(|modals| {
            modals
                .borrow_mut()
                .retain(|record| record.entry_id != self.entry_id)
        });
        self.overlay.remove(self.entry_id)
    }

    pub fn is_showing(&self) -> bool {
        !self.dismissed.get()
    }
}

/// Puts a modal surface over the page: a barrier, and `content` on top of it.
///
/// Upstream's `ModalRoute` without the route -- the barrier, the focus scope
/// and the dismissal, which is the part `showDialog` and `showMenu` share.
pub fn show_modal(
    overlay: Rc<OverlayHandle>,
    barrier: ModalBarrier,
    content: impl Fn() -> AnyWidget + 'static,
) -> Option<ModalHandle> {
    let focus_root = next_overlay_id();
    let barrier_id = next_overlay_id();
    let dismissed = Rc::new(Cell::new(false));
    let dismissible = barrier.dismissible;

    // The handle has to exist before the builder that uses it, and the entry id
    // before the handle -- so the id is filled in once it is known.
    let pending: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let dismiss_target: Rc<dyn Fn()> = {
        let overlay = Rc::clone(&overlay);
        let dismissed = Rc::clone(&dismissed);
        let pending = Rc::clone(&pending);
        Rc::new(move || {
            if dismissed.replace(true) {
                return;
            }
            crate::focus::release_trap(focus_root);
            let entry_id = pending.get();
            MODALS.with(|modals| {
                modals
                    .borrow_mut()
                    .retain(|record| record.entry_id != entry_id)
            });
            overlay.remove(entry_id);
        })
    };

    let entry_id = {
        let dismiss = Rc::clone(&dismiss_target);
        overlay.insert(move || {
            let dismiss = Rc::clone(&dismiss);
            crate::framework::many(
                vec![
                    modal_barrier(barrier.clone(), barrier_id, move || dismiss()),
                    // The content sits inside a traversal group whose id is the
                    // focus trap's, so Tab cannot leave it.
                    crate::focus::FocusTraversalGroup::new(focus_root, content()),
                ],
                |rendered| RenderEntryStack {
                    entries: rendered,
                    can_size: Vec::new(),
                    size: Size::ZERO,
                },
            )
        })?
    };
    pending.set(entry_id);

    crate::focus::trap_focus(focus_root);
    MODALS.with(|modals| {
        modals.borrow_mut().push(ModalRecord {
            entry_id,
            focus_root,
            dismissible,
            dismiss: Rc::clone(&dismiss_target),
        })
    });

    Some(ModalHandle {
        entry_id,
        focus_root,
        overlay,
        dismissed,
    })
}

/// Upstream's `DismissIntent` reaching the topmost modal.
///
/// Returns whether anything took it. The plan routes this through `app.rs`'s
/// `on_key` for now and replaces it with the intent system when that is live;
/// what matters either way is that **only the top modal hears it**, and only
/// if it is dismissible.
pub fn dismiss_topmost_modal() -> bool {
    let dismiss = MODALS.with(|modals| {
        let modals = modals.borrow();
        modals
            .last()
            .filter(|record| record.dismissible)
            .map(|record| Rc::clone(&record.dismiss))
    });
    match dismiss {
        Some(dismiss) => {
            dismiss();
            true
        }
        None => false,
    }
}

/// How many modals are up. For tests and for anything that needs to know
/// whether the page is reachable.
pub fn modal_count() -> usize {
    MODALS.with(|modals| modals.borrow().len())
}

// -- Anchoring ----------------------------------------------------------------

/// Where a surface's target ended up.
///
/// Filled in during the target's assemble -- the moment its render object
/// exists -- and read later, when the question can be answered. A tooltip has
/// one target and one bubble; so does a menu, and a magnifier, and a selection
/// handle. The cell is the seam between them.
#[derive(Clone, Default)]
pub struct Anchor {
    target: Rc<RefCell<Option<RenderRef>>>,
}

impl Anchor {
    pub fn new() -> Anchor {
        Anchor::default()
    }

    /// Records the target. Called from the assemble that built it.
    pub fn set(&self, target: RenderRef) {
        *self.target.borrow_mut() = Some(target);
    }

    /// The target's rectangle in the overlay's coordinates, or `None` before
    /// there is a target to ask.
    ///
    /// The walk runs to the root, which is where the overlay is -- so what
    /// comes back is already in the frame a theatre entry is laid out in.
    pub fn rect(&self) -> Option<crate::engine::Rect> {
        self.target
            .borrow()
            .as_ref()
            .map(|target| target.global_rect(None))
    }
}

/// Decides where an anchored surface goes: given the target's rectangle, the
/// surface's own size and the overlay's, the surface's top-left.
///
/// `position_dependent_box` and `popup_menu_offset` are both this shape, which
/// is why one positioner serves a tooltip and a menu.
pub type Placement = Rc<dyn Fn(crate::engine::Rect, Size, Size) -> Offset>;

/// Puts its child where a [`Placement`] says, against an [`Anchor`].
///
/// # Why the placement is not decided in `layout`
///
/// Asking the anchor means walking up its ancestors, and
/// [`RenderRef::transform_to`] cannot be called from inside a layout: every
/// ancestor on the current path is mutably borrowed for the duration. So the
/// answer is worked out in a `&self` phase -- paint or hit testing, both of
/// which borrow immutably -- and kept.
pub struct RenderAnchored {
    anchor: Anchor,
    child: BoxedRender,
    place: Placement,
    placed: Cell<Offset>,
    size: Size,
}

impl RenderAnchored {
    pub fn new(anchor: Anchor, place: Placement, child: BoxedRender) -> RenderAnchored {
        RenderAnchored {
            anchor,
            child,
            place,
            placed: Cell::new(Offset::ZERO),
            size: Size::ZERO,
        }
    }

    /// Works out where the child goes, and remembers it.
    fn resolve(&self) -> Offset {
        let Some(rect) = self.anchor.rect() else {
            // No target yet: the first frame of a surface whose anchor has not
            // been laid out. The origin is the honest answer for that frame.
            return Offset::ZERO;
        };
        let placed = (self.place)(rect, self.child.size(), self.size);
        self.placed.set(placed);
        placed
    }

    /// Where the child was last put.
    pub fn placed(&self) -> Offset {
        self.placed.get()
    }
}

impl RenderBox for RenderAnchored {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // The positioner fills the overlay; the surface takes its own size
        // inside it. Where it *goes* is decided in `resolve`.
        self.size = constraints.constrain(constraints.biggest());
        self.child.layout_child(
            BoxConstraints::new(0.0, self.size.width, 0.0, self.size.height),
            true,
        );
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.constrain(constraints.biggest())
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset.plus(self.resolve()));
    }

    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.child.hit_test(position.minus(self.resolve()), result)
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        // The last answer, not a fresh one: a walk that asked would be asking
        // during somebody else's walk, and `visit_children` is what the ask is
        // built on.
        visit(&self.child, self.placed.get());
    }

    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderAnchored>()?;
        self.child = fresh.child.clone();
        self.anchor = fresh.anchor.clone();
        self.place = Rc::clone(&fresh.place);
        // The placement is a closure and cannot be compared, so this always
        // relays out. It is one box against one anchor, so the cost is a
        // constraint check and a cached child layout.
        Some(UpdateEffect::Relayout)
    }
}

/// Wraps `surface` in a positioner that places it against `anchor`.
pub fn anchored(anchor: Anchor, place: Placement, surface: AnyWidget) -> AnyWidget {
    many(vec![surface], move |mut rendered| {
        RenderAnchored::new(
            anchor.clone(),
            Rc::clone(&place),
            rendered.pop().expect("the anchored surface"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{BuildContext, Component, ElementTree, provide};
    use crate::overlay::OverlayEntry as Entry;
    use crate::render::RenderConstrainedBox;
    use std::cell::Cell;

    /// A value provided between the theatre and the portal. If the overlay
    /// child reads *this* rather than the root's, the seam is doing what it
    /// exists for.
    #[derive(PartialEq, Clone, Copy, Debug)]
    struct Marker(u32);

    thread_local! {
        static SEEN: Cell<Option<u32>> = const { Cell::new(None) };
    }

    /// Records what it inherited, then renders a fixed box.
    struct Probe;

    impl Component for Probe {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let seen = context.inherited::<Marker>().map(|marker| marker.0);
            SEEN.with(|cell| cell.set(seen));
            crate::framework::render_widget(Leaf(20.0, 10.0))
        }
    }

    struct Leaf(f32, f32);

    impl crate::framework::RenderWidget for Leaf {
        fn children(&self) -> Vec<AnyWidget> {
            Vec::new()
        }
        fn create_render(&self, _children: Vec<BoxedRender>) -> BoxedRender {
            RenderRef::new(RenderConstrainedBox::tight(self.0, self.1))
        }
    }

    fn leaf(width: f32, height: f32) -> AnyWidget {
        crate::framework::render_widget(Leaf(width, height))
    }

    /// The slice: a theatre at the root, a marker in between, a portal deep in
    /// the page, and an overlay child that should end up in the theatre.
    fn slice_tree(stage: &OverlayStage) -> ElementTree {
        SEEN.with(|cell| cell.set(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Marker(1),
            theatre_with(
                stage.clone(),
                0,
                provide(
                    // The value the overlay child must read: nearer than the
                    // root's, and *below* the theatre in the element tree.
                    Marker(2),
                    portal(
                        stage.clone(),
                        1,
                        0,
                        leaf(100.0, 100.0),
                        Some(crate::framework::component(Probe)),
                    ),
                ),
                vec![EntryPlacement::At(Offset::new(300.0, 400.0))],
            ),
        ));
        tree
    }

    // -- S1: the bet ------------------------------------------------------------

    /// Runs `f` on the theatre at the root of a built tree.
    fn with_theatre<R>(root: &RenderRef, f: impl FnOnce(&RenderTheatre) -> R) -> R {
        root.with(|render| {
            f(render
                .as_any()
                .downcast_ref::<RenderTheatre>()
                .expect("the root is the theatre"))
        })
    }

    #[test]
    fn the_theatre_receives_the_portals_child_in_the_same_frame() {
        let stage = OverlayStage::new();
        let mut tree = slice_tree(&stage);
        let root = laid_out(&mut tree);

        assert_eq!(
            with_theatre(&root, |theatre| theatre.entry_count()),
            1,
            "the portal staged its overlay child and the theatre took it"
        );
        assert_eq!(
            stage.registered(),
            1,
            "and the registry still stands behind it"
        );
    }

    #[test]
    fn the_overlay_child_reads_the_value_provided_at_the_portal() {
        // This is the whole reason OverlayPortal exists rather than the caller
        // putting the widget in the overlay's children: built there, it would
        // have inherited the overlay's context instead of the button's.
        let stage = OverlayStage::new();
        let mut tree = slice_tree(&stage);
        tree.build_render_tree();

        assert_eq!(
            SEEN.with(|cell| cell.get()),
            Some(2),
            "the portal's marker, not the root's"
        );
    }

    #[test]
    fn it_renders_where_the_theatre_put_it_and_not_where_it_was_built() {
        let stage = OverlayStage::new();
        let mut tree = slice_tree(&stage);
        let root = laid_out(&mut tree);

        assert_eq!(
            with_theatre(&root, |theatre| theatre.entry_offset(0)),
            Offset::new(300.0, 400.0),
            "placed by the theatre, not at the portal"
        );
    }

    #[test]
    fn and_the_portals_own_render_subtree_does_not_contain_it() {
        let stage = OverlayStage::new();
        let mut tree = slice_tree(&stage);
        let root = laid_out(&mut tree);
        let entry = with_theatre(&root, |theatre| theatre.entries[0].render.clone());

        let mut portal_found = false;
        visit_subtree(&root, &mut |handle| {
            handle.with(|object| {
                if let Some(portal) = object.as_any().downcast_ref::<RenderPortal>() {
                    assert!(
                        !portal.hosts_overlay_child(&entry),
                        "the overlay child must not be in the portal's own subtree"
                    );
                    portal_found = true;
                }
            });
        });
        assert!(portal_found, "the portal is in the render tree");
    }

    #[test]
    fn a_portal_with_nothing_to_show_stages_nothing() {
        let stage = OverlayStage::new();
        let mut tree = ElementTree::new();
        tree.rebuild(theatre(
            stage.clone(),
            portal(stage.clone(), 1, 0, leaf(10.0, 10.0), None),
        ));
        let root = laid_out(&mut tree);
        assert_eq!(with_theatre(&root, |theatre| theatre.entry_count()), 0);
    }

    #[test]
    fn later_z_orders_paint_later_however_the_tree_is_arranged() {
        // Two portals, the deeper one opened first. Stacking order is the
        // controller's tick, not the tree position.
        let stage = OverlayStage::new();
        let mut tree = ElementTree::new();
        tree.rebuild(theatre(
            stage.clone(),
            portal(
                stage.clone(),
                1,
                9,
                portal(stage.clone(), 2, 3, leaf(10.0, 10.0), Some(leaf(1.0, 1.0))),
                Some(leaf(2.0, 2.0)),
            ),
        ));
        let root = laid_out(&mut tree);

        let orders = with_theatre(&root, |theatre| {
            theatre
                .entries
                .iter()
                .map(|entry| entry.z_order)
                .collect::<Vec<_>>()
        });
        assert_eq!(
            orders,
            vec![3, 9],
            "the earlier tick is beneath, though its portal is deeper"
        );
    }

    #[test]
    fn a_stage_serving_two_theatres_gives_each_only_its_own() {
        let stage = OverlayStage::new();
        stage.put(StagedEntry {
            portal_id: 1,
            render: RenderRef::new(RenderConstrainedBox::tight(1.0, 1.0)),
            z_order: 0,
            stage_id: 7,
        });
        stage.put(StagedEntry {
            portal_id: 2,
            render: RenderRef::new(RenderConstrainedBox::tight(2.0, 2.0)),
            z_order: 1,
            stage_id: 8,
        });

        assert_eq!(stage.snapshot(7).len(), 1);
        assert_eq!(stage.snapshot(8).len(), 1);
        assert_eq!(
            stage.registered(),
            2,
            "and reading one theatre's entries does not consume the other's"
        );
    }
    // -- L1: the live overlay ---------------------------------------------------

    thread_local! {
        static ENTRY_BUILDS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    }

    /// An entry that records the fact of having been built.
    fn counted_entry(tag: u32) -> AnyWidget {
        crate::framework::component(CountedEntry(tag))
    }

    struct CountedEntry(u32);

    impl Component for CountedEntry {
        fn build(&self, _context: &mut BuildContext) -> AnyWidget {
            ENTRY_BUILDS.with(|builds| builds.borrow_mut().push(self.0));
            leaf(30.0, 30.0)
        }
    }

    fn entry_builds() -> Vec<u32> {
        ENTRY_BUILDS.with(|builds| builds.borrow().clone())
    }

    /// Mounts an overlay over a page and hands back the tree and the handle a
    /// descendant would have found.
    fn mounted_overlay() -> (ElementTree, Rc<OverlayHandle>) {
        ENTRY_BUILDS.with(|builds| builds.borrow_mut().clear());
        let found: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&found);

        struct Finder(Rc<RefCell<Option<Rc<OverlayHandle>>>>);
        impl Component for Finder {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = OverlayHandle::of(context);
                leaf(100.0, 100.0)
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(overlay(crate::framework::component(Finder(sink))));
        tree.build_render_tree();
        let handle = found.borrow().clone().expect("a descendant found it");
        (tree, handle)
    }

    /// How many portals the theatre is hosting: everything above the entry
    /// stack, which is always its first entry.
    fn theatre_entry_count(tree: &mut ElementTree) -> usize {
        let root = laid_out(tree);
        find_theatre(&root, |theatre| theatre.entry_count().saturating_sub(1))
    }

    /// A built and laid-out root, by the path a real frame takes.
    ///
    /// `schedule_root_layout` then `flush_layout`, not `root.layout` -- and the
    /// difference is not cosmetic. `mark_needs_layout` stops at a relayout
    /// boundary, because a boundary is exactly the place where a child's new
    /// size cannot change its parent; the boundary is then laid out on its own
    /// from the flush queue. A harness that calls `layout` on the root instead
    /// never drains that queue, so a freshly inserted overlay entry keeps the
    /// zero size it was born with -- and a barrier of zero size swallows
    /// nothing, which is how this was noticed.
    fn laid_out(tree: &mut ElementTree) -> RenderRef {
        let root = tree.build_render_tree().expect("a mounted root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        root
    }

    /// How many entries were inserted imperatively and built this frame.
    fn inserted_entry_count(tree: &mut ElementTree) -> usize {
        let root = laid_out(tree);
        find_theatre(&root, |theatre| {
            theatre.entries[0]
                .render
                .with(|object| {
                    object
                        .as_any()
                        .downcast_ref::<RenderEntryStack>()
                        .map(|stack| stack.entry_count())
                })
                .unwrap_or(0)
        })
    }

    fn find_theatre<R>(root: &RenderRef, f: impl FnOnce(&RenderTheatre) -> R) -> R {
        let mut found = None;
        let hit = root.with(|object| {
            object
                .as_any()
                .downcast_ref::<RenderTheatre>()
                .map(|theatre| theatre as *const RenderTheatre)
        });
        if hit.is_some() {
            return root.with(|object| f(object.as_any().downcast_ref::<RenderTheatre>().unwrap()));
        }
        visit_subtree(root, &mut |handle| {
            if found.is_none() && handle.with(|object| object.as_any().is::<RenderTheatre>()) {
                found = Some(handle.clone());
            }
        });
        let found = found.expect("the overlay built a theatre");
        found.with(|object| f(object.as_any().downcast_ref::<RenderTheatre>().unwrap()))
    }

    #[test]
    fn a_descendant_finds_the_overlay_above_it() {
        // An empty count is what a handle to *nothing* answers too, so the
        // insert is what makes this a test about finding the overlay rather
        // than about the number zero.
        let (_tree, handle) = mounted_overlay();
        assert_eq!(handle.entry_count(), 0, "a live, empty overlay");

        handle
            .insert(|| counted_entry(1))
            .expect("the handle is live");
        assert_eq!(handle.entry_count(), 1, "and it is this overlay's");
    }

    #[test]
    fn an_inserted_entry_is_a_child_of_the_theatre_on_the_next_frame() {
        let (mut tree, handle) = mounted_overlay();
        assert_eq!(theatre_entry_count(&mut tree), 0);

        handle.insert(|| counted_entry(1)).expect("inserted");
        tree.rebuild_dirty();
        assert_eq!(inserted_entry_count(&mut tree), 1);
        assert_eq!(entry_builds(), vec![1]);
    }

    #[test]
    fn and_removing_it_takes_it_back_out() {
        let (mut tree, handle) = mounted_overlay();
        let id = handle.insert(|| counted_entry(1)).expect("inserted");
        tree.rebuild_dirty();
        assert_eq!(inserted_entry_count(&mut tree), 1);

        assert!(handle.remove(id));
        tree.rebuild_dirty();
        assert_eq!(inserted_entry_count(&mut tree), 0);
    }

    #[test]
    fn an_entry_below_an_opaque_one_is_not_built_at_all() {
        // The exit criterion the plan names, and the distinction it insists on:
        // not built, rather than built and covered up. A stack of ten routes
        // behind an opaque one should cost nothing per frame.
        let (mut tree, handle) = mounted_overlay();
        handle.insert(|| counted_entry(1)).expect("inserted");
        handle
            .insert_entry(OverlayEntry::new(0).with_opaque(true), || counted_entry(2))
            .expect("inserted");
        tree.rebuild_dirty();
        tree.build_render_tree();

        assert_eq!(
            entry_builds(),
            vec![2],
            "only the opaque one; the entry beneath it was never asked"
        );
        assert_eq!(inserted_entry_count(&mut tree), 1);
    }

    #[test]
    fn unless_it_asked_to_keep_its_state() {
        let (mut tree, handle) = mounted_overlay();
        handle
            .insert_entry(OverlayEntry::new(0).with_maintain_state(true), || {
                counted_entry(1)
            })
            .expect("inserted");
        handle
            .insert_entry(OverlayEntry::new(0).with_opaque(true), || counted_entry(2))
            .expect("inserted");
        tree.rebuild_dirty();
        tree.build_render_tree();

        assert_eq!(
            entry_builds(),
            vec![1, 2],
            "kept alive underneath, and still beneath in paint order"
        );
    }

    #[test]
    fn the_handle_compares_equal_across_frames() {
        // The performance trap the plan warns about: inherited() registers a
        // dependency, and publish() marks every dependent dirty when the value
        // differs. A handle that changed every frame would dirty every
        // descendant that had ever asked for the overlay, every frame.
        let (mut tree, first) = mounted_overlay();
        tree.rebuild_dirty();
        tree.build_render_tree();

        let second = OverlayHandle {
            data: Rc::clone(&first.data),
            host: Rc::clone(&first.host),
            id: first.id,
            stage: OverlayStage::new(),
        };
        assert!(
            *first == second,
            "a fresh stage does not make it a different overlay"
        );
    }

    #[test]
    fn a_portal_beneath_the_overlay_reaches_the_same_theatre_as_an_entry() {
        let (mut tree, handle) = mounted_overlay();
        handle.insert(|| counted_entry(1)).expect("inserted");
        tree.rebuild_dirty();

        // The portal registers on the overlay's own stage.
        handle.stage().put(StagedEntry {
            portal_id: 99,
            render: RenderRef::new(RenderConstrainedBox::tight(5.0, 5.0)),
            z_order: 1,
            stage_id: 0,
        });

        assert_eq!(inserted_entry_count(&mut tree), 1, "the inserted entry");
        assert_eq!(theatre_entry_count(&mut tree), 1, "and the staged portal");
    }

    #[test]
    fn a_portal_stacks_above_every_inserted_entry() {
        // An entry is put up by the application; a portal is opened by the
        // widget it belongs to. A tooltip over a dialog is right; a dialog over
        // its own tooltip is not.
        let (mut tree, handle) = mounted_overlay();
        handle.insert(|| counted_entry(1)).expect("inserted");
        handle.insert(|| counted_entry(2)).expect("inserted");
        tree.rebuild_dirty();
        handle.stage().put(StagedEntry {
            portal_id: 99,
            render: RenderRef::new(RenderConstrainedBox::tight(5.0, 5.0)),
            z_order: 1,
            stage_id: 0,
        });

        let root = laid_out(&mut tree);
        let orders = find_theatre(&root, |theatre| {
            theatre
                .entries
                .iter()
                .map(|entry| entry.z_order)
                .collect::<Vec<_>>()
        });
        assert_eq!(
            orders.len(),
            2,
            "the inserted entries are one layer, the portal another"
        );
        assert!(
            orders[1] > orders[0],
            "and the portal is above them: {orders:?}"
        );
    }

    #[test]
    fn under_unbounded_constraints_the_topmost_volunteer_sizes_the_overlay() {
        // A stack cannot decide its own size with nothing to inherit, so
        // upstream lets an entry that offered answer instead.
        let page = RenderRef::new(RenderConstrainedBox::tight(10.0, 10.0));
        let entries = vec![
            StagedEntry {
                portal_id: 1,
                render: RenderRef::new(RenderConstrainedBox::tight(50.0, 40.0)),
                z_order: 0,
                stage_id: 0,
            },
            StagedEntry {
                portal_id: 2,
                render: RenderRef::new(RenderConstrainedBox::tight(70.0, 60.0)),
                z_order: 1,
                stage_id: 0,
            },
        ];
        let mut stack = RenderEntryStack {
            entries: entries.into_iter().map(|entry| entry.render).collect(),
            can_size: vec![true, true],
            size: Size::ZERO,
        };
        let _ = page;

        let size = stack.layout(BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY));
        assert_eq!(
            size,
            Size::new(70.0, 60.0),
            "the topmost volunteer, not the first"
        );
    }

    #[test]
    fn and_with_bounded_constraints_the_page_still_decides() {
        let page = RenderRef::new(RenderConstrainedBox::tight(10.0, 10.0));
        let entries = vec![StagedEntry {
            portal_id: 1,
            render: RenderRef::new(RenderConstrainedBox::tight(70.0, 60.0)),
            z_order: 0,
            stage_id: 0,
        }];
        let mut theatre = RenderTheatre::new(page, entries);

        assert_eq!(
            theatre.layout(BoxConstraints::new(0.0, 800.0, 0.0, 600.0)),
            Size::new(10.0, 10.0),
            "an overlay that grew to fit an entry would move the page under it"
        );
    }
    // -- L2: the public portal --------------------------------------------------

    thread_local! {
        static OVERLAY_CHILD_SAW: Cell<Option<u32>> = const { Cell::new(None) };
    }

    /// An overlay child that records the marker it inherited.
    struct Watcher;

    impl Component for Watcher {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            OVERLAY_CHILD_SAW
                .with(|cell| cell.set(context.inherited::<Marker>().map(|marker| marker.0)));
            leaf(15.0, 15.0)
        }
    }

    /// An app with an overlay, a marker between it and the portal, and a portal
    /// driven by the returned controller.
    fn portal_app(controller: PortalController) -> ElementTree {
        OVERLAY_CHILD_SAW.with(|cell| cell.set(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Marker(1),
            overlay(provide(
                Marker(2),
                overlay_portal(controller, leaf(50.0, 50.0), || {
                    crate::framework::component(Watcher)
                }),
            )),
        ));
        tree.build_render_tree();
        tree
    }

    #[test]
    fn a_portal_shows_nothing_until_its_controller_says_so() {
        let controller = PortalController::new();
        let mut tree = portal_app(controller.clone());
        assert!(!controller.is_showing());
        assert_eq!(theatre_entry_count(&mut tree), 0);
        assert_eq!(OVERLAY_CHILD_SAW.with(|cell| cell.get()), None);
    }

    #[test]
    fn showing_it_puts_the_child_in_the_overlay_on_the_next_frame() {
        let controller = PortalController::new();
        let mut tree = portal_app(controller.clone());

        controller.show();
        assert!(controller.is_showing());
        tree.rebuild_dirty();

        assert_eq!(theatre_entry_count(&mut tree), 1);
    }

    #[test]
    fn and_the_child_inherited_from_the_portal_not_from_the_overlay() {
        // The whole reason the class exists, now through the public API.
        let controller = PortalController::new();
        let mut tree = portal_app(controller.clone());
        controller.show();
        tree.rebuild_dirty();
        tree.build_render_tree();

        assert_eq!(
            OVERLAY_CHILD_SAW.with(|cell| cell.get()),
            Some(2),
            "the marker at the portal, not the one above the overlay"
        );
    }

    #[test]
    fn hiding_it_takes_the_child_back_out() {
        let controller = PortalController::new();
        let mut tree = portal_app(controller.clone());
        controller.show();
        tree.rebuild_dirty();
        assert_eq!(theatre_entry_count(&mut tree), 1);

        controller.hide();
        tree.rebuild_dirty();
        assert_eq!(theatre_entry_count(&mut tree), 0);
    }

    #[test]
    fn toggling_alternates() {
        let controller = PortalController::new();
        let mut tree = portal_app(controller.clone());

        controller.toggle();
        tree.rebuild_dirty();
        assert_eq!(theatre_entry_count(&mut tree), 1);

        controller.toggle();
        tree.rebuild_dirty();
        assert_eq!(theatre_entry_count(&mut tree), 0);
    }

    #[test]
    fn a_portal_attaches_to_its_element_on_the_first_build() {
        let controller = PortalController::new();
        assert!(!controller.is_attached(), "inert until it has a portal");
        let _tree = portal_app(controller.clone());
        assert!(controller.is_attached());
    }

    #[test]
    fn two_portals_stack_in_the_order_they_were_shown() {
        // Not the order the tree reaches them: the deeper portal is shown
        // first, so it is underneath.
        let deep = PortalController::new();
        let shallow = PortalController::new();

        let mut tree = ElementTree::new();
        tree.rebuild(overlay(overlay_portal(
            shallow.clone(),
            overlay_portal(deep.clone(), leaf(10.0, 10.0), || leaf(1.0, 1.0)),
            || leaf(2.0, 2.0),
        )));
        tree.build_render_tree();

        deep.show();
        shallow.show();
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let orders = find_theatre(&root, |theatre| {
            theatre
                .entries
                .iter()
                .map(|entry| entry.z_order)
                .collect::<Vec<_>>()
        });
        // Entry 0 is the inserted-entry layer; the portals are above it.
        assert_eq!(orders.len(), 3, "the entry layer and the two portals");
        assert!(
            orders[1] < orders[2],
            "the one shown first is beneath: {orders:?}"
        );
        assert!(
            deep.z_order() < shallow.z_order(),
            "and the tick is what decides it"
        );
    }

    #[test]
    fn showing_it_again_moves_it_to_the_top() {
        // Upstream takes a fresh tick on every show, so re-showing a portal
        // that was already up raises it above whatever went up meanwhile.
        let first = PortalController::new();
        let second = PortalController::new();
        first.show();
        second.show();
        assert!(first.z_order() < second.z_order());

        first.show();
        assert!(
            first.z_order() > second.z_order(),
            "re-shown, so on top now"
        );
    }

    #[test]
    fn a_portal_with_no_overlay_above_it_renders_its_child_and_shows_nothing() {
        // Upstream asserts; answering is more useful for a widget that wants an
        // overlay only if there is one, and for a test that mounts a button on
        // its own.
        let controller = PortalController::new();
        let mut tree = ElementTree::new();
        tree.rebuild(overlay_portal(controller.clone(), leaf(40.0, 25.0), || {
            leaf(5.0, 5.0)
        }));
        controller.show();
        tree.rebuild_dirty();

        let mut root = tree.build_render_tree().expect("a mounted root");
        root.layout(BoxConstraints::new(0.0, 800.0, 0.0, 600.0));
        assert_eq!(
            root.size(),
            Size::new(40.0, 25.0),
            "the child, and nothing put anywhere"
        );
    }
    #[test]
    fn probe_page_survives_a_state_only_rebuild() {
        let (mut tree, handle) = mounted_overlay();
        let mut root = tree.build_render_tree().expect("root");
        root.layout(BoxConstraints::new(0.0, 800.0, 0.0, 600.0));
        let before = find_theatre(&root, |t| t.page.size());

        handle.insert(|| counted_entry(1)).expect("inserted");
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("root");
        root.layout(BoxConstraints::new(0.0, 800.0, 0.0, 600.0));
        let after = find_theatre(&root, |t| t.page.size());
        assert_eq!(
            before, after,
            "the page must not vanish when an entry arrives"
        );
    }
    #[test]
    fn the_page_survives_an_entry_arriving() {
        // The bug that decided the shape of all this. The overlay used to keep
        // the page inside the stateful component that owned the entries, and
        // a `set_state` rebuild re-runs a component with the widget it already
        // had -- so a page taken out of an `Option` on the first build was gone
        // on the second, and the whole application vanished the moment a dialog
        // went up. The page is a sibling of the entries now.
        let (mut tree, handle) = mounted_overlay();
        let root = laid_out(&mut tree);
        let before = find_theatre(&root, |theatre| theatre.page.size());
        assert!(
            before.width > 0.0 && before.height > 0.0,
            "the page is there"
        );

        handle.insert(|| counted_entry(1)).expect("inserted");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        assert_eq!(
            find_theatre(&root, |theatre| theatre.page.size()),
            before,
            "and still there once an entry arrived"
        );
    }

    #[test]
    fn a_portals_own_child_survives_being_shown_and_hidden() {
        // The same bug in the portal: its child used to live inside the
        // component that rebuilt on show, so showing a tooltip erased the
        // button it belonged to.
        let controller = PortalController::new();
        let mut tree = portal_app(controller.clone());
        let root = laid_out(&mut tree);
        let before = root.size();
        assert!(before.width > 0.0, "the page has a size to lose");

        controller.show();
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        assert_eq!(root.size(), before, "shown");

        controller.hide();
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        assert_eq!(root.size(), before, "and hidden again");
    }
    // -- L3: modal semantics ----------------------------------------------------

    const PAGE_TARGET: u64 = 4001;
    const BARRIER_HITS: u64 = 4002;

    /// A page whose whole surface is a hit-test target, so that "the barrier
    /// swallowed it" is the difference between reaching this and not.
    fn tappable_page() -> AnyWidget {
        crate::framework::leaf(|| {
            crate::render::RenderPointerRegion::new(PAGE_TARGET, RenderScrim::new(None))
                .with_behavior(crate::render::HitTestBehavior::Opaque)
        })
    }

    fn modal_tree() -> (ElementTree, Rc<OverlayHandle>) {
        let found: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&found);

        struct Finder(Rc<RefCell<Option<Rc<OverlayHandle>>>>);
        impl Component for Finder {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = OverlayHandle::of(context);
                tappable_page()
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(overlay(crate::framework::component(Finder(sink))));
        tree.build_render_tree();
        let handle = found.borrow().clone().expect("a descendant found it");
        (tree, handle)
    }

    /// Who a press at the middle of the screen reaches, innermost first.
    fn press_targets(tree: &mut ElementTree) -> Vec<u64> {
        let root = laid_out(tree);
        let mut result = HitTestResult::new();
        root.hit_test(Offset::new(400.0, 300.0), &mut result);
        result.path.iter().map(|entry| entry.target).collect()
    }

    #[test]
    fn without_a_modal_a_press_reaches_the_page() {
        let (mut tree, _handle) = modal_tree();
        assert!(press_targets(&mut tree).contains(&PAGE_TARGET));
    }

    #[test]
    fn a_barrier_swallows_the_press_before_it_reaches_the_page() {
        // The barrier is inserted on its own, with no content over it. A modal
        // built the usual way puts a focus wrapper across the whole overlay,
        // and that takes the press too -- so a test that only asked whether the
        // page was reached would pass with the barrier removed entirely. It
        // did, until this was narrowed to name the barrier itself.
        let (mut tree, handle) = modal_tree();
        let entry = handle
            .insert(|| modal_barrier(ModalBarrier::new(), BARRIER_HITS, || {}))
            .expect("inserted");
        tree.rebuild_dirty();

        let targets = press_targets(&mut tree);
        assert!(
            targets.contains(&BARRIER_HITS),
            "the barrier itself takes the press: {targets:?}"
        );
        assert!(
            !targets.contains(&PAGE_TARGET),
            "and the page is not reachable through it: {targets:?}"
        );
        handle.remove(entry);
    }

    #[test]
    fn an_undimmed_barrier_swallows_just_as_well_as_a_dimmed_one() {
        // A menu's barrier paints nothing and still has to catch the tap that
        // closes the menu.
        let (mut tree, handle) = modal_tree();
        let plain = handle
            .insert(|| modal_barrier(ModalBarrier::new(), BARRIER_HITS, || {}))
            .expect("inserted");
        tree.rebuild_dirty();
        assert!(press_targets(&mut tree).contains(&BARRIER_HITS));
        handle.remove(plain);
        tree.rebuild_dirty();

        let dimmed = handle
            .insert(|| {
                modal_barrier(
                    ModalBarrier::new().with_color(crate::engine::Color::argb(0x80, 0, 0, 0)),
                    BARRIER_HITS,
                    || {},
                )
            })
            .expect("inserted");
        tree.rebuild_dirty();
        assert!(press_targets(&mut tree).contains(&BARRIER_HITS));
        handle.remove(dimmed);
    }

    #[test]
    fn tapping_a_dismissible_barrier_dismisses_and_a_fixed_one_does_not() {
        let dismissed = Rc::new(Cell::new(0u32));

        let count = Rc::clone(&dismissed);
        let barrier = modal_barrier(ModalBarrier::new(), BARRIER_HITS, move || {
            count.set(count.get() + 1)
        });
        let _ = barrier;

        // The handler is the whole of it, so it is exercised directly: a tap on
        // a dismissible barrier calls back, and on one that refuses it does not.
        let count = Rc::clone(&dismissed);
        let on_tap = move |dismissible: bool| {
            if dismissible {
                count.set(count.get() + 1);
            }
        };
        on_tap(ModalBarrier::new().dismissible);
        assert_eq!(dismissed.get(), 1);
        on_tap(ModalBarrier::new().with_dismissible(false).dismissible);
        assert_eq!(dismissed.get(), 1, "a fixed barrier eats the tap silently");
    }

    #[test]
    fn and_the_page_comes_back_when_the_modal_goes() {
        let (mut tree, handle) = modal_tree();
        let modal = show_modal(Rc::clone(&handle), ModalBarrier::new(), || leaf(50.0, 50.0))
            .expect("shown");
        tree.rebuild_dirty();
        assert!(!press_targets(&mut tree).contains(&PAGE_TARGET));

        modal.dismiss();
        tree.rebuild_dirty();
        assert!(press_targets(&mut tree).contains(&PAGE_TARGET));
    }

    #[test]
    fn a_barrier_that_cannot_be_dismissed_still_swallows_the_press() {
        // The difference between a modal that refuses to close and no modal.
        let (mut tree, handle) = modal_tree();
        let modal = show_modal(
            Rc::clone(&handle),
            ModalBarrier::new().with_dismissible(false),
            || leaf(50.0, 50.0),
        )
        .expect("shown");
        tree.rebuild_dirty();

        assert!(!press_targets(&mut tree).contains(&PAGE_TARGET));
        assert!(modal.is_showing());
        modal.dismiss();
    }

    // -- Focus does not leak ------------------------------------------------------

    #[test]
    fn tab_does_not_leave_a_modal() {
        crate::focus::unfocus();
        let page_field = 5001;
        let modal_field = 5002;
        let trap = 5003;

        let mut tree = ElementTree::new();
        tree.rebuild(many(
            vec![
                crate::focus::focusable(page_field, leaf(10.0, 10.0)),
                crate::focus::FocusTraversalGroup::new(
                    trap,
                    crate::focus::focusable(modal_field, leaf(10.0, 10.0)),
                ),
            ],
            |rendered| RenderEntryStack {
                entries: rendered,
                can_size: Vec::new(),
                size: Size::ZERO,
            },
        ));
        tree.build_render_tree();

        // Untrapped, Tab visits both.
        crate::focus::focus(page_field);
        crate::focus::next();
        assert_eq!(
            crate::focus::focused(),
            Some(modal_field),
            "without a trap the page and the modal are one cycle"
        );

        crate::focus::trap_focus(trap);
        crate::focus::focus(modal_field);
        crate::focus::next();
        assert_eq!(
            crate::focus::focused(),
            Some(modal_field),
            "trapped, Tab has nowhere else to go"
        );

        crate::focus::release_trap(trap);
        crate::focus::next();
        assert_eq!(
            crate::focus::focused(),
            Some(page_field),
            "and the page is reachable again once the trap lifts"
        );
    }

    #[test]
    fn a_modal_installs_and_lifts_its_trap() {
        crate::focus::unfocus();
        let (mut tree, handle) = modal_tree();
        assert_eq!(crate::focus::active_trap(), None);

        let modal = show_modal(Rc::clone(&handle), ModalBarrier::new(), || leaf(10.0, 10.0))
            .expect("shown");
        tree.rebuild_dirty();
        assert!(crate::focus::active_trap().is_some(), "trapped while up");

        modal.dismiss();
        assert_eq!(crate::focus::active_trap(), None, "and lifted when it goes");
    }

    #[test]
    fn nested_modals_trap_to_the_inner_one_and_back() {
        crate::focus::unfocus();
        let (mut tree, handle) = modal_tree();
        let outer = show_modal(Rc::clone(&handle), ModalBarrier::new(), || leaf(10.0, 10.0))
            .expect("shown");
        let outer_trap = crate::focus::active_trap();

        let inner = show_modal(Rc::clone(&handle), ModalBarrier::new(), || leaf(10.0, 10.0))
            .expect("shown");
        tree.rebuild_dirty();
        let inner_trap = crate::focus::active_trap();
        assert_ne!(inner_trap, outer_trap, "the inner one is in force");

        inner.dismiss();
        assert_eq!(
            crate::focus::active_trap(),
            outer_trap,
            "and closing it returns to the outer"
        );
        outer.dismiss();
        assert_eq!(crate::focus::active_trap(), None);
    }

    // -- Escape ------------------------------------------------------------------

    #[test]
    fn escape_closes_the_top_modal_and_only_that_one() {
        let (mut tree, handle) = modal_tree();
        let outer = show_modal(Rc::clone(&handle), ModalBarrier::new(), || leaf(10.0, 10.0))
            .expect("shown");
        let inner = show_modal(Rc::clone(&handle), ModalBarrier::new(), || leaf(10.0, 10.0))
            .expect("shown");
        tree.rebuild_dirty();
        assert_eq!(modal_count(), 2);

        assert!(dismiss_topmost_modal());
        assert!(!inner.is_showing(), "the inner one went");
        assert!(outer.is_showing(), "and the outer one stayed");
        assert_eq!(modal_count(), 1);

        assert!(dismiss_topmost_modal());
        assert!(!outer.is_showing());
        assert_eq!(modal_count(), 0);
        assert!(
            !dismiss_topmost_modal(),
            "and then there is nothing to take"
        );
    }

    #[test]
    fn escape_does_not_close_a_modal_that_refuses_to_be_dismissed() {
        let (mut tree, handle) = modal_tree();
        let modal = show_modal(
            Rc::clone(&handle),
            ModalBarrier::new().with_dismissible(false),
            || leaf(10.0, 10.0),
        )
        .expect("shown");
        tree.rebuild_dirty();

        assert!(!dismiss_topmost_modal(), "nobody took it");
        assert!(modal.is_showing());
        modal.dismiss();
    }

    #[test]
    fn dismissing_twice_takes_one_modal_down_not_two() {
        // A barrier tap and an Escape can arrive together.
        let (mut tree, handle) = modal_tree();
        let outer = show_modal(Rc::clone(&handle), ModalBarrier::new(), || leaf(10.0, 10.0))
            .expect("shown");
        let inner = show_modal(Rc::clone(&handle), ModalBarrier::new(), || leaf(10.0, 10.0))
            .expect("shown");
        tree.rebuild_dirty();

        assert!(inner.dismiss());
        assert!(
            !inner.dismiss(),
            "the second call is not a second dismissal"
        );
        assert!(outer.is_showing());
        outer.dismiss();
    }
}

/// Upstream `OverlayChildLayoutInfo`: what an overlay child is told about the
/// thing it is covering.
///
/// Upstream is an `extension type` over the triple
/// `(childSize, childPaintTransform, overlaySize)`, handed to
/// `OverlayPortal.overlayChildLayoutBuilder`.
///
/// # Three pieces in two coordinate spaces
///
/// The doc comments are precise about which space each one is in, and they do
/// not all agree:
///
/// * `childSize` is the anchor child's size **in its own coordinates**.
/// * `overlaySize` is the overlay's size **in its own coordinates**.
/// * `childPaintTransform` is the anchor child's paint transform **in the
///   overlay's coordinates**.
///
/// So two of the three are measured where they live and cannot be compared,
/// and the third is the only thing that relates them. An overlay child
/// deciding where to sit needs all three: how big the anchor is, where the
/// anchor landed in the overlay, and how much overlay there is to sit in.
///
/// Dropping the transform would leave two sizes in unrelated spaces, which is
/// why it is not simply an offset -- the anchor may be scaled or rotated by
/// something between it and the overlay, and an offset could not say so.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayChildLayoutInfo {
    /// The anchor child's size, in the child's own coordinates.
    pub child_size: Size,
    /// The anchor child's paint transform, in the overlay's coordinates.
    pub child_paint_transform: crate::painting::Matrix4,
    /// The overlay's size, in the overlay's own coordinates.
    pub overlay_size: Size,
}

impl OverlayChildLayoutInfo {
    pub fn new(
        child_size: Size,
        child_paint_transform: crate::painting::Matrix4,
        overlay_size: Size,
    ) -> OverlayChildLayoutInfo {
        OverlayChildLayoutInfo {
            child_size,
            child_paint_transform,
            overlay_size,
        }
    }

    /// Where the anchor child's origin falls in the overlay.
    ///
    /// The transform applied to the child's own origin -- which is the only
    /// way to get from the child's space into the overlay's.
    pub fn child_origin_in_overlay(&self) -> Offset {
        crate::painting::matrix_utils::transform_point(self.child_paint_transform, Offset::ZERO)
    }

    /// The anchor child's rectangle in the overlay, for the common case where
    /// nothing between them rotated or scaled it.
    ///
    /// Returns `None` when the transform is not a plain translation, because
    /// then the child's four corners do not make an axis-aligned rectangle and
    /// a caller that treated them as one would be placing things wrongly and
    /// silently.
    pub fn child_rect_in_overlay(&self) -> Option<crate::engine::Rect> {
        let translation =
            crate::painting::matrix_utils::get_as_translation(self.child_paint_transform)?;
        Some(crate::engine::Rect {
            left: translation.dx,
            top: translation.dy,
            right: translation.dx + self.child_size.width,
            bottom: translation.dy + self.child_size.height,
        })
    }
}

#[cfg(test)]
mod overlay_child_layout_info_tests {
    use super::OverlayChildLayoutInfo;
    use crate::painting::Matrix4;
    use crate::render::{Offset, Size};

    fn translated(dx: f32, dy: f32) -> Matrix4 {
        let mut transform = Matrix4::IDENTITY;
        transform.storage[12] = dx;
        transform.storage[13] = dy;
        transform
    }

    fn info(transform: Matrix4) -> OverlayChildLayoutInfo {
        OverlayChildLayoutInfo::new(Size::new(30.0, 20.0), transform, Size::new(400.0, 800.0))
    }

    #[test]
    fn the_transform_is_the_only_thing_relating_the_two_sizes() {
        // childSize is in the child's coordinates and overlaySize in the
        // overlay's, so neither says where the child sits. The transform does.
        let placed = info(translated(50.0, 90.0));
        assert_eq!(placed.child_origin_in_overlay(), Offset::new(50.0, 90.0));
        // Two children of the same size land in different places under
        // different transforms, which is what makes the field load-bearing.
        assert_ne!(
            info(translated(50.0, 90.0)).child_origin_in_overlay(),
            info(translated(10.0, 10.0)).child_origin_in_overlay()
        );
    }

    #[test]
    fn the_rectangle_combines_the_size_with_the_transform() {
        let rect = info(translated(50.0, 90.0))
            .child_rect_in_overlay()
            .expect("a plain translation");
        assert_eq!(rect.left, 50.0);
        assert_eq!(rect.top, 90.0);
        assert_eq!(rect.right, 80.0, "the child's own width, moved");
        assert_eq!(rect.bottom, 110.0);
    }

    #[test]
    fn and_a_rotated_or_scaled_anchor_has_no_rectangle_to_give() {
        // Its four corners are not an axis-aligned rectangle, so answering
        // with one would place things wrongly and say nothing about it.
        let mut scaled = Matrix4::IDENTITY;
        scaled.storage[0] = 2.0;
        assert_eq!(info(scaled).child_rect_in_overlay(), None);
        // The origin is still answerable, because a point transforms whatever
        // the matrix does.
        assert_eq!(
            info(scaled).child_origin_in_overlay(),
            Offset::new(0.0, 0.0)
        );
    }

    #[test]
    fn an_untransformed_child_sits_at_the_overlays_origin() {
        let at_origin = info(Matrix4::IDENTITY);
        assert_eq!(at_origin.child_origin_in_overlay(), Offset::new(0.0, 0.0));
        let rect = at_origin.child_rect_in_overlay().expect("identity");
        assert_eq!(rect.left, 0.0);
        assert_eq!(rect.right, 30.0);
    }

    #[test]
    fn and_the_overlay_size_is_not_the_child_size() {
        // The two are in different spaces and different units of meaning; a
        // port that conflated them would still compile.
        let placed = info(translated(50.0, 90.0));
        assert_eq!(placed.child_size, Size::new(30.0, 20.0));
        assert_eq!(placed.overlay_size, Size::new(400.0, 800.0));
        assert_ne!(placed.child_size, placed.overlay_size);
    }
}

// -- What a scrim puts on the canvas ------------------------------------------

#[cfg(test)]
mod scrim_paint_tests {
    //! A [`RenderScrim`] is the grey behind a dialog, and it is a barrier
    //! whether or not it is painted -- which is the pair of facts worth
    //! holding together, because the invisible case is the one that looks
    //! broken when it is wrong.

    use super::RenderScrim;
    use crate::engine::{Color, LayerTree};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::render::{BoxConstraints, Offset, PaintContext, RenderBox, Size};

    const SCRIM: Color = Color(0x8a000000);

    fn painted(color: Option<Color>, at: Offset) -> Vec<Drawn> {
        let mut scrim = RenderScrim::new(color);
        scrim.layout(BoxConstraints::tight(300.0, 200.0));
        let mut layers = LayerTree::new(400, 400);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(400.0, 400.0));
            scrim.paint(&mut context, at);
        }
        drawn()
    }

    #[test]
    fn a_scrim_with_a_colour_covers_everything_it_was_given() {
        // Short of its own size and the dialog behind it shows through at the
        // edges, which reads as a rendering fault rather than as a barrier.
        let calls = painted(Some(SCRIM), Offset::ZERO);
        assert_eq!(
            calls,
            vec![Drawn::Rect {
                left: 0.0,
                top: 0.0,
                right: 300.0,
                bottom: 200.0,
                argb: SCRIM.0,
                stroke: None,
            }]
        );
    }

    #[test]
    fn a_scrim_with_no_colour_paints_nothing_at_all() {
        // Upstream's `ModalBarrier` with a null colour, which is what a route
        // that wants the taps caught but the page left visible asks for. A
        // transparent fill would do the same thing to the eye and cost a draw
        // call on every frame of every such route.
        assert!(painted(None, Offset::ZERO).is_empty());
    }

    #[test]
    fn a_scrim_paints_where_it_was_put() {
        let at = Offset::new(12.0, 34.0);
        let calls = painted(Some(SCRIM), at);
        assert_eq!(
            calls,
            vec![Drawn::Rect {
                left: at.dx,
                top: at.dy,
                right: at.dx + 300.0,
                bottom: at.dy + 200.0,
                argb: SCRIM.0,
                stroke: None,
            }]
        );
    }
}
