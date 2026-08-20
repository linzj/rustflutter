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

use std::cell::RefCell;
use std::rc::Rc;

use crate::framework::{AnyWidget, many};
use crate::render::{
    BoxConstraints, BoxedRender, HitTestResult, Offset, PaintContext, RenderBox, RenderRef, Size,
    UpdateEffect,
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
    size: Size,
}

impl RenderTheatre {
    pub fn new(page: BoxedRender, entries: Vec<StagedEntry>) -> RenderTheatre {
        let placements = vec![EntryPlacement::Fill; entries.len()];
        RenderTheatre {
            page,
            entries,
            placements,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{BuildContext, Component, ElementTree, provide};
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
}
