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
use crate::overlay::{InsertPosition, OverlayEntry, OverlayState};
use crate::render::{
    BoxConstraints, BoxedRender, HitTestResult, Offset, PaintContext, RenderBox,
    RenderConstrainedBox, RenderRef, Size, UpdateEffect,
};

/// A render object waiting to be picked up by a theatre, and where it sits in
/// the stack.
#[derive(Clone)]
pub struct StagedEntry {
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

/// The side channel between a portal and its theatre.
///
/// One frame's worth of staged entries. Portals push during their assemble;
/// the theatre drains during its own, which runs later in the same walk.
///
/// It is a `RefCell<Vec<_>>` rather than anything cleverer because the whole
/// exchange happens inside one synchronous tree walk on one thread: there is no
/// moment at which a push and a drain could interleave.
#[derive(Clone, Default)]
pub struct OverlayStage {
    entries: Rc<RefCell<Vec<StagedEntry>>>,
}

impl OverlayStage {
    pub fn new() -> OverlayStage {
        OverlayStage::default()
    }

    /// Called from a portal's assemble.
    pub fn stage(&self, entry: StagedEntry) {
        self.entries.borrow_mut().push(entry);
    }

    /// Called from a theatre's assemble: takes everything staged for
    /// `stage_id`, in z order, and leaves the rest for another theatre.
    pub fn collect(&self, stage_id: u64) -> Vec<StagedEntry> {
        let mut all = self.entries.borrow_mut();
        let mut mine = Vec::new();
        let mut rest = Vec::new();
        for entry in all.drain(..) {
            if entry.stage_id == stage_id {
                mine.push(entry);
            } else {
                rest.push(entry);
            }
        }
        *all = rest;
        mine.sort_by_key(|entry| entry.z_order);
        mine
    }

    /// How many entries are waiting. For tests and for the assertion that a
    /// frame left nothing behind.
    pub fn pending(&self) -> usize {
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
    entries: Vec<StagedEntry>,
    placements: Vec<EntryPlacement>,
    /// Which of the leading entries offered to size the overlay, in order.
    /// Upstream's `canSizeOverlay`, which matters only under unbounded
    /// constraints.
    entry_can_size: Vec<bool>,
    size: Size,
}

impl RenderTheatre {
    pub fn new(page: BoxedRender, entries: Vec<StagedEntry>) -> RenderTheatre {
        let placements = vec![EntryPlacement::Fill; entries.len()];
        RenderTheatre {
            page,
            entries,
            placements,
            entry_can_size: Vec::new(),
            size: Size::ZERO,
        }
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

    /// The topmost entry that offered to size the overlay, if any.
    ///
    /// Topmost, not first: the entry nearest the reader is the one whose size
    /// the overlay should be, and it is the last in paint order.
    fn sizing_entry(&self) -> Option<usize> {
        (0..self.entries.len())
            .rev()
            .find(|index| self.entry_can_size.get(*index).copied().unwrap_or(false))
    }

    /// Whether the object behind `render` is one of this theatre's entries.
    pub fn hosts(&self, render: &RenderRef) -> bool {
        self.entries.iter().any(|entry| entry.render.is(render))
    }
}

impl RenderBox for RenderTheatre {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // The page first and alone decides the size, exactly as upstream's
        // theatre takes its size from the constraints it was given rather than
        // from any entry: an overlay that grew to fit a tooltip would move the
        // page underneath it.
        //
        // Except when nothing can: under unbounded constraints there is no
        // size to inherit, and upstream lets the topmost entry that offered
        // `canSizeOverlay` answer instead. An entry that offers is taking on
        // the question the overlay could not answer, which is why it has to
        // cope with unbounded constraints itself.
        if !constraints.has_bounded_width() || !constraints.has_bounded_height() {
            if let Some(index) = self.sizing_entry() {
                let chosen = self.entries[index].render.layout_child(constraints, true);
                self.size = chosen;
                let tight = BoxConstraints::tight(chosen.width, chosen.height);
                self.page.layout_child(tight, true);
                for (other, entry) in self.entries.iter_mut().enumerate() {
                    if other != index {
                        entry.render.layout_child(tight, true);
                    }
                }
                return self.size;
            }
        }
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
        self.entries = std::mem::take(&mut fresh.entries);
        self.placements = std::mem::take(&mut fresh.placements);
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
}

impl RenderPortal {
    pub fn new(child: BoxedRender) -> RenderPortal {
        RenderPortal { child }
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
        let changed = !self.child.is(&fresh.child);
        self.child = fresh.child.clone();
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
    z_order: u64,
    child: AnyWidget,
    overlay_child: Option<AnyWidget>,
) -> AnyWidget {
    portal_on(stage, 0, z_order, child, overlay_child)
}

/// [`portal`] aimed at a particular theatre.
pub fn portal_on(
    stage: OverlayStage,
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
                stage.stage(StagedEntry {
                    render,
                    z_order,
                    stage_id,
                });
            }
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
        let entries = stage.collect(stage_id);
        let placements = if placements.is_empty() {
            vec![EntryPlacement::Fill; entries.len()]
        } else {
            let mut filled = placements.clone();
            filled.resize(entries.len(), EntryPlacement::Fill);
            filled
        };
        RenderTheatre::new(
            rendered.pop().expect("a theatre always has its page"),
            entries,
        )
        .with_placements(placements)
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
}

/// A descendant's way to reach the overlay above it. Upstream's `Overlay.of`.
///
/// Published with `provide`, so `Overlay::of(context)` is an inherited lookup.
#[derive(Clone)]
pub struct OverlayHandle {
    handle: StateHandle<LiveOverlay>,
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
        self.handle.element() == other.handle.element()
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

    pub fn element(&self) -> ElementId {
        self.handle.element()
    }

    /// Upstream `OverlayState.insert`. Returns the id the entry was given, or
    /// `None` if the overlay has gone away.
    pub fn insert(&self, builder: impl Fn() -> AnyWidget + 'static) -> Option<u64> {
        self.insert_entry(OverlayEntry::new(0), builder)
    }

    /// [`OverlayHandle::insert`] with the entry's flags -- `opaque`,
    /// `maintainState`, `canSizeOverlay` -- chosen. The id on the entry passed
    /// in is ignored; the overlay assigns one.
    pub fn insert_entry(
        &self,
        entry: OverlayEntry,
        builder: impl Fn() -> AnyWidget + 'static,
    ) -> Option<u64> {
        if !self.handle.is_valid() {
            return None;
        }
        let builder: EntryBuilder = Rc::new(builder);
        let id = Rc::new(Cell::new(0u64));
        let assigned = Rc::clone(&id);
        let accepted = self.handle.set_state(move |live| {
            let entry_id = live.next_id;
            live.next_id += 1;
            assigned.set(entry_id);
            let mut entry = entry;
            entry.id = entry_id;
            let _ = live.state.insert(entry, InsertPosition::Top);
            live.builders.push((entry_id, builder));
            live.state.flush_build();
        });
        accepted.then(|| id.get())
    }

    /// Upstream `OverlayEntry.remove`.
    pub fn remove(&self, id: u64) -> bool {
        self.handle.set_state(move |live| {
            live.state.remove(id, false);
            live.builders.retain(|(entry_id, _)| *entry_id != id);
            live.state.flush_build();
        })
    }
}

/// Upstream `Overlay`: the host.
///
/// Wraps a page and puts entries on top of it. The page is a child rather than
/// the bottom entry -- upstream's overlay *is* the whole surface and the app is
/// its first entry, but an overlay that can be dropped over an existing tree is
/// more useful here than one the caller has to re-express their app for.
pub struct OverlayHost {
    page: RefCell<Option<AnyWidget>>,
}

impl OverlayHost {
    pub fn new(page: AnyWidget) -> OverlayHost {
        OverlayHost {
            page: RefCell::new(Some(page)),
        }
    }
}

impl StatefulComponent for OverlayHost {
    type State = LiveOverlay;

    fn initial_state(&self) -> LiveOverlay {
        LiveOverlay::new()
    }

    fn build(
        &self,
        state: &LiveOverlay,
        handle: StateHandle<LiveOverlay>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let stage = state.stage.clone();
        let page = self
            .page
            .borrow_mut()
            .take()
            .unwrap_or_else(|| crate::framework::render_widget(EmptyPage));

        let mut children = vec![page];
        let entry_count = {
            let entries = state.onstage_widgets();
            let count = entries.len();
            children.extend(entries);
            count
        };

        let sizing = state
            .state
            .entries()
            .iter()
            .map(|entry| entry.can_size_overlay)
            .collect::<Vec<_>>();

        provide(
            OverlayHandle {
                handle,
                stage: stage.clone(),
            },
            many(children, move |mut rendered| {
                // The staged portal children, collected in the same walk that
                // built them -- see this module's header.
                let staged = stage.collect(0);
                let entries: Vec<StagedEntry> = rendered
                    .split_off(1)
                    .into_iter()
                    .enumerate()
                    .map(|(index, render)| StagedEntry {
                        render,
                        // Inserted entries stack in list order, beneath every
                        // portal: a portal is opened by the thing it belongs
                        // to and a dialog by the application, and the
                        // application's surfaces go under.
                        z_order: index as u64,
                        stage_id: 0,
                    })
                    .chain(staged.into_iter().map(|mut entry| {
                        entry.z_order = entry.z_order.saturating_add(1 << 32);
                        entry
                    }))
                    .collect();
                let mut theatre = RenderTheatre::new(rendered.pop().expect("the page"), entries);
                theatre.entry_can_size = sizing.iter().take(entry_count).copied().collect();
                theatre
            }),
        )
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

/// Puts an overlay above `page`. Upstream's `Overlay` in `WidgetsApp`.
pub fn overlay(page: AnyWidget) -> AnyWidget {
    crate::framework::stateful(OverlayHost::new(page))
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
        let root = tree.build_render_tree().expect("a mounted root");

        assert_eq!(
            with_theatre(&root, |theatre| theatre.entry_count()),
            1,
            "the portal staged its overlay child and the theatre took it"
        );
        assert_eq!(
            stage.pending(),
            0,
            "and nothing was left waiting for a later frame"
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
        let mut root = tree.build_render_tree().expect("a mounted root");
        root.layout(BoxConstraints::tight(800.0, 600.0));

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
        let root = tree.build_render_tree().expect("a mounted root");
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
            portal(stage.clone(), 0, leaf(10.0, 10.0), None),
        ));
        let root = tree.build_render_tree().expect("a mounted root");
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
                9,
                portal(stage.clone(), 3, leaf(10.0, 10.0), Some(leaf(1.0, 1.0))),
                Some(leaf(2.0, 2.0)),
            ),
        ));
        let root = tree.build_render_tree().expect("a mounted root");

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
        stage.stage(StagedEntry {
            render: RenderRef::new(RenderConstrainedBox::tight(1.0, 1.0)),
            z_order: 0,
            stage_id: 7,
        });
        stage.stage(StagedEntry {
            render: RenderRef::new(RenderConstrainedBox::tight(2.0, 2.0)),
            z_order: 1,
            stage_id: 8,
        });

        assert_eq!(stage.collect(7).len(), 1);
        assert_eq!(
            stage.pending(),
            1,
            "the other theatre's entry is still waiting for it"
        );
        assert_eq!(stage.collect(8).len(), 1);
        assert_eq!(stage.pending(), 0);
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

    fn theatre_entry_count(tree: &mut ElementTree) -> usize {
        let root = tree.build_render_tree().expect("a mounted root");
        find_theatre(&root, |theatre| theatre.entry_count())
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
        let (_tree, handle) = mounted_overlay();
        assert!(
            handle.handle.is_valid(),
            "a live handle to a mounted overlay"
        );
    }

    #[test]
    fn an_inserted_entry_is_a_child_of_the_theatre_on_the_next_frame() {
        let (mut tree, handle) = mounted_overlay();
        assert_eq!(theatre_entry_count(&mut tree), 0);

        handle.insert(|| counted_entry(1)).expect("inserted");
        tree.rebuild_dirty();
        assert_eq!(theatre_entry_count(&mut tree), 1);
        assert_eq!(entry_builds(), vec![1]);
    }

    #[test]
    fn and_removing_it_takes_it_back_out() {
        let (mut tree, handle) = mounted_overlay();
        let id = handle.insert(|| counted_entry(1)).expect("inserted");
        tree.rebuild_dirty();
        assert_eq!(theatre_entry_count(&mut tree), 1);

        assert!(handle.remove(id));
        tree.rebuild_dirty();
        assert_eq!(theatre_entry_count(&mut tree), 0);
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
        assert_eq!(theatre_entry_count(&mut tree), 1);
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
            handle: first.handle.clone(),
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

        // The portal stages onto the overlay's own stage.
        let staged = StagedEntry {
            render: RenderRef::new(RenderConstrainedBox::tight(5.0, 5.0)),
            z_order: 0,
            stage_id: 0,
        };
        handle.stage().stage(staged);

        assert_eq!(
            theatre_entry_count(&mut tree),
            2,
            "the inserted entry and the staged portal child"
        );
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
        handle.stage().stage(StagedEntry {
            render: RenderRef::new(RenderConstrainedBox::tight(5.0, 5.0)),
            z_order: 0,
            stage_id: 0,
        });

        let root = tree.build_render_tree().expect("a mounted root");
        let orders = find_theatre(&root, |theatre| {
            theatre
                .entries
                .iter()
                .map(|entry| entry.z_order)
                .collect::<Vec<_>>()
        });
        assert_eq!(orders.len(), 3);
        assert!(
            orders[2] > orders[1] && orders[1] > orders[0],
            "the portal is on top: {orders:?}"
        );
    }

    #[test]
    fn under_unbounded_constraints_the_topmost_volunteer_sizes_the_overlay() {
        // A stack cannot decide its own size with nothing to inherit, so
        // upstream lets an entry that offered answer instead.
        let page = RenderRef::new(RenderConstrainedBox::tight(10.0, 10.0));
        let entries = vec![
            StagedEntry {
                render: RenderRef::new(RenderConstrainedBox::tight(50.0, 40.0)),
                z_order: 0,
                stage_id: 0,
            },
            StagedEntry {
                render: RenderRef::new(RenderConstrainedBox::tight(70.0, 60.0)),
                z_order: 1,
                stage_id: 0,
            },
        ];
        let mut theatre = RenderTheatre::new(page, entries);
        theatre.entry_can_size = vec![true, true];

        let size = theatre.layout(BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY));
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
            render: RenderRef::new(RenderConstrainedBox::tight(70.0, 60.0)),
            z_order: 0,
            stage_id: 0,
        }];
        let mut theatre = RenderTheatre::new(page, entries);
        theatre.entry_can_size = vec![true];

        assert_eq!(
            theatre.layout(BoxConstraints::new(0.0, 800.0, 0.0, 600.0)),
            Size::new(10.0, 10.0),
            "an overlay that grew to fit an entry would move the page under it"
        );
    }
}
