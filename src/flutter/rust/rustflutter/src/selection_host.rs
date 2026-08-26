// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Selection handles and the toolbar, on the screen: upstream's
//! `SelectionOverlay`.
//!
//! `text_selection.rs` and `text_selection_controls.rs` port the whole of what
//! a selection overlay *decides* -- the handle sizes and anchors, the four
//! `can*` rules, the two toolbar anchors, the above-or-below choice, the drag
//! grab point. Both said the same thing about what was missing:
//!
//! > [`TextSelectionOverlay`] and [`SelectionOverlay`] position handles and a
//! > toolbar in an `Overlay`, which this crate does not have
//!
//! It does now.
//!
//! # Why the handles are in the overlay and not in the field
//!
//! This is upstream's own reason and it is a good one: **a handle hangs below
//! the text it holds**. The one on the last line of a field sits under the
//! field's bottom edge, and a handle drawn inside the field would be clipped
//! away exactly when the reader is dragging it. Putting handles in the overlay
//! is not a convenience -- it is the only place they fit.
//!
//! The toolbar is there for the matching reason at the other edge: it goes
//! *above* the selection when there is room, which for a selection on the first
//! line is above the field.
//!
//! # Three entries, not one
//!
//! Upstream keeps the start handle, the end handle and the toolbar as separate
//! overlay entries, and so does this. They come and go independently --
//! `hideToolbar` leaves the handles up, a collapsed selection has one handle
//! rather than two -- and an entry each is what lets them.
//!
//! # Global in, overlay-local out
//!
//! Every position here arrives in global coordinates: the endpoints come from
//! the field, the editing region is the field's own rectangle. An entry is laid
//! out in the overlay's. The conversion is
//! [`RenderRef::global_to_local`](crate::render::RenderRef::global_to_local),
//! done once per placement -- see [`crate::magnifier_host`], which has the same
//! seam for the same reason.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::engine::Rect;
use crate::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent, many};
use crate::magnifier_host::{MagnifierHost, MagnifierStyle, show_magnifier};
use crate::render::{Offset, RenderConstrainedBox, RenderRef, RenderStack, Size, StackPosition};
use crate::text_selection::{SelectionOverlay, TextSelectionToolbarLayoutDelegate};
use crate::text_selection_controls::{
    TextSelectionControls, TextSelectionHandleType, TextSelectionToolbarAnchors,
};
use crate::theatre::{EntryRefresh, OverlayHandle};

/// Where a selection's edge is, and how tall the line under it is.
///
/// Upstream's `TextSelectionPoint`: a point plus the direction of the text at
/// it. The line height rides along because it is what sizes the handle --
/// upstream reads it off `renderObject.preferredLineHeight` at the same moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionEndpoint {
    /// The **bottom** of the selection at this edge, in global coordinates.
    /// Upstream's point is the baseline-ish bottom for the same reason a handle
    /// hangs below: that is where the handle's tip goes.
    pub point: Offset,
    pub line_height: f32,
}

impl SelectionEndpoint {
    pub fn new(point: Offset, line_height: f32) -> SelectionEndpoint {
        SelectionEndpoint { point, line_height }
    }
}

/// Where a handle's box goes, given the endpoint it holds.
///
/// The anchor is **subtracted**: `handle_anchor` says where inside the handle
/// its point is, so the handle's own top-left is that far back from the
/// endpoint. Exactly the arithmetic `Draggable::feedback_position` does for a
/// drag anchor, and for the same reason -- a handle placed by its corner would
/// sit beside the character it is holding rather than on it.
pub fn handle_position(
    controls: &dyn TextSelectionControls,
    kind: TextSelectionHandleType,
    endpoint: SelectionEndpoint,
) -> Offset {
    let anchor = controls.handle_anchor(kind, endpoint.line_height);
    Offset::new(endpoint.point.dx - anchor.dx, endpoint.point.dy - anchor.dy)
}

/// Which of the two handles this is. Upstream chooses the *type* from the text
/// direction, not from which end it is -- a right-to-left selection's start
/// handle is the right-hand one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleEnd {
    Start,
    End,
}

/// What the entries are showing. One cell per thing that can move, so a drag
/// repositions a handle without touching the toolbar.
#[derive(Clone, Default)]
struct OverlayGeometry {
    start_at: Rc<Cell<Offset>>,
    end_at: Rc<Cell<Offset>>,
    toolbar_at: Rc<Cell<Offset>>,
    start_size: Rc<Cell<Size>>,
    end_size: Rc<Cell<Size>>,
    handles_visible: Rc<Cell<bool>>,
    collapsed: Rc<Cell<bool>>,
    toolbar_visible: Rc<Cell<bool>>,
    /// What tells the three entries the cells above changed. One between them,
    /// because a handle moving and the toolbar moving are the same frame's
    /// work and there is nothing to gain from waking them separately.
    refresh: EntryRefresh,
}

impl OverlayGeometry {
    fn touch(&self) {
        self.refresh.refresh();
    }
}

/// One handle's entry.
struct HandleEntry {
    end: HandleEnd,
    geometry: OverlayGeometry,
    /// The hit id the handle answers to, so a gesture layer can find it. The
    /// handles are the one part of a selection overlay that **is** hit-testable
    /// -- the whole point of them is to be dragged.
    hit_id: u64,
}

impl StatefulComponent for HandleEntry {
    type State = u64;

    fn initial_state(&self) -> u64 {
        self.geometry.refresh.revision()
    }

    fn build(
        &self,
        _state: &u64,
        handle: StateHandle<u64>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        self.geometry.refresh.attach(handle);
        let shown = self.geometry.handles_visible.get()
            // A collapsed selection has one handle. Upstream draws the
            // `Collapsed` type at the caret and no second one, because two
            // handles on the same point would be two grab targets for one
            // position and the reader could not tell which they had.
            && !(self.end == HandleEnd::End && self.geometry.collapsed.get());
        if !shown {
            return crate::framework::leaf(|| RenderConstrainedBox::tight(0.0, 0.0));
        }

        let (at, size) = match self.end {
            HandleEnd::Start => (self.geometry.start_at.get(), self.geometry.start_size.get()),
            HandleEnd::End => (self.geometry.end_at.get(), self.geometry.end_size.get()),
        };
        let hit_id = self.hit_id;
        crate::framework::leaf(move || {
            RenderStack::new().push_positioned(
                crate::render::RenderPointerRegion::new(
                    hit_id,
                    RenderConstrainedBox::tight(size.width, size.height),
                )
                .with_behavior(crate::render::HitTestBehavior::Opaque),
                StackPosition {
                    left: Some(at.dx),
                    top: Some(at.dy),
                    ..StackPosition::default()
                },
            )
        })
    }
}

/// The toolbar's entry.
struct ToolbarEntry {
    geometry: OverlayGeometry,
    toolbar: Rc<dyn Fn() -> AnyWidget>,
}

impl StatefulComponent for ToolbarEntry {
    type State = u64;

    fn initial_state(&self) -> u64 {
        self.geometry.refresh.revision()
    }

    fn build(
        &self,
        _state: &u64,
        handle: StateHandle<u64>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        self.geometry.refresh.attach(handle);
        if !self.geometry.toolbar_visible.get() {
            return crate::framework::leaf(|| RenderConstrainedBox::tight(0.0, 0.0));
        }
        let at = self.geometry.toolbar_at.get();
        many(vec![(self.toolbar)()], move |mut rendered| {
            RenderStack::new().push_positioned_boxed(
                rendered.pop().expect("the selection toolbar"),
                StackPosition {
                    left: Some(at.dx),
                    top: Some(at.dy),
                    ..StackPosition::default()
                },
            )
        })
    }
}

/// A live selection overlay: two handles and a toolbar, in three entries.
///
/// Upstream's `SelectionOverlay`, whose [`SelectionOverlay`] configuration this
/// carries verbatim -- the visibility rules are not re-decided here, they are
/// read from it.
pub struct SelectionHost {
    /// The ported decisions. Public because every rule about what is shown
    /// lives there and this is only the hosting.
    pub config: SelectionOverlay,
    controls: Rc<dyn TextSelectionControls>,
    overlay: Rc<OverlayHandle>,
    entries: Vec<u64>,
    geometry: OverlayGeometry,
    /// What the last placement decided for the toolbar, kept so a caller can
    /// ask whether it ended up above or below without repeating the layout.
    toolbar_above: Cell<bool>,
    /// The hit ids the two handles answer to.
    handle_ids: (u64, u64),
    /// Upstream's `_magnifierController`, which `SelectionOverlay` owns
    /// alongside the handles and the toolbar. `None` here is upstream's
    /// `overlayEntry == null`.
    magnifier: Option<MagnifierHost>,
}

impl SelectionHost {
    /// The hit ids of the start and end handles.
    pub fn handle_ids(&self) -> (u64, u64) {
        self.handle_ids
    }

    /// Where the start handle's box is, in the overlay's coordinates.
    pub fn start_handle_at(&self) -> Offset {
        self.geometry.start_at.get()
    }

    pub fn end_handle_at(&self) -> Offset {
        self.geometry.end_at.get()
    }

    pub fn toolbar_at(&self) -> Offset {
        self.geometry.toolbar_at.get()
    }

    /// Whether the toolbar's last placement put it above the selection.
    pub fn toolbar_is_above(&self) -> bool {
        self.toolbar_above.get()
    }

    /// Moves the handles to a new selection.
    ///
    /// `collapsed` is upstream's caret case: one handle, of the `Collapsed`
    /// type, rather than two.
    pub fn set_selection(
        &mut self,
        start: SelectionEndpoint,
        end: SelectionEndpoint,
        collapsed: bool,
        overlay: &RenderRef,
    ) {
        let (start_kind, end_kind) = if collapsed {
            (
                TextSelectionHandleType::Collapsed,
                TextSelectionHandleType::Collapsed,
            )
        } else {
            (
                TextSelectionHandleType::Left,
                TextSelectionHandleType::Right,
            )
        };

        let start_global = handle_position(self.controls.as_ref(), start_kind, start);
        let end_global = handle_position(self.controls.as_ref(), end_kind, end);

        self.geometry
            .start_at
            .set(overlay.global_to_local(start_global, None));
        self.geometry
            .end_at
            .set(overlay.global_to_local(end_global, None));
        self.geometry
            .start_size
            .set(self.controls.handle_size(start.line_height));
        self.geometry
            .end_size
            .set(self.controls.handle_size(end.line_height));
        self.geometry.collapsed.set(collapsed);
        self.geometry
            .handles_visible
            .set(self.config.handles_visible);
        self.geometry.touch();
    }

    /// Places the toolbar against a selection rectangle.
    ///
    /// The two anchors come from
    /// [`TextSelectionToolbarAnchors::from_selection`] and the choice between
    /// them from [`TextSelectionToolbarLayoutDelegate::position_for_child`] --
    /// neither decision is remade here. What this adds is that the delegate
    /// works in the overlay's coordinates, so both anchors are converted
    /// *before* it runs rather than its answer being converted after: the
    /// above-or-below test compares an anchor against the toolbar's height, and
    /// comparing a global anchor against a local height would pick the wrong
    /// side on any overlay that is not at the window's origin.
    pub fn place_toolbar(
        &mut self,
        selection_rect: Rect,
        editing_region: Rect,
        toolbar_size: Size,
        overlay: &RenderRef,
        overlay_size: Size,
    ) {
        let anchors = TextSelectionToolbarAnchors::from_selection(selection_rect, editing_region);
        let above = overlay.global_to_local(anchors.primary_anchor, None);
        let below = anchors
            .secondary_anchor
            .map(|anchor| overlay.global_to_local(anchor, None))
            .unwrap_or(above);

        let delegate =
            TextSelectionToolbarLayoutDelegate::new((above.dx, above.dy), (below.dx, below.dy));
        let (x, y) = delegate.position_for_child(
            (overlay_size.width, overlay_size.height),
            (toolbar_size.width, toolbar_size.height),
        );
        self.toolbar_above.set(above.dy >= toolbar_size.height);
        self.geometry.toolbar_at.set(Offset::new(x, y));
        self.geometry
            .toolbar_visible
            .set(self.config.toolbar_visible);
        self.geometry.touch();
    }

    /// Upstream's `showToolbar` / `hideToolbar`, which change the
    /// configuration and let the entries follow.
    pub fn set_toolbar_visible(&mut self, visible: bool) {
        self.config.set_toolbar_visible(visible);
        self.geometry.toolbar_visible.set(visible);
        self.geometry.touch();
    }

    /// Upstream's `showHandles` / `hideHandles`.
    pub fn set_handles_visible(&mut self, visible: bool) {
        self.config.handles_visible = visible;
        self.geometry.handles_visible.set(visible);
        self.geometry.touch();
    }

    /// Upstream's `hide`, which takes **both** away -- a toolbar without
    /// handles acts on a selection whose edges the reader can no longer see.
    pub fn hide(&mut self) {
        self.config.hide();
        self.geometry.handles_visible.set(false);
        self.geometry.toolbar_visible.set(false);
        self.geometry.touch();
    }

    // -- The magnifier ------------------------------------------------------

    /// Upstream's `SelectionOverlay.magnifierExists`:
    /// `_magnifierController.overlayEntry != null`.
    ///
    /// The question both of upstream's guards ask, and the one this host could
    /// not answer at all until now.
    pub fn magnifier_exists(&self) -> bool {
        self.magnifier.as_ref().is_some_and(MagnifierHost::exists)
    }

    /// Upstream's `SelectionOverlay.magnifierIsVisible`:
    /// `_magnifierController.shown`.
    ///
    /// Upstream's doc for the pair is the whole distinction: *"This differs
    /// from [magnifierIsVisible] in that the magnifier may exist in the
    /// overlay, but not be shown."*
    pub fn magnifier_is_visible(&self) -> bool {
        self.magnifier.as_ref().is_some_and(MagnifierHost::is_shown)
    }

    /// Upstream's `SelectionOverlay.showMagnifier`.
    ///
    /// ```dart
    /// if (_magnifierController.overlayEntry != null) { return; }
    /// if (toolbarIsVisible) { hideToolbar(); }
    /// _magnifierInfo.value = initialMagnifierInfo;
    /// final Widget? builtMagnifier = magnifierConfiguration.magnifierBuilder(...);
    /// if (builtMagnifier == null) { return; }
    /// _magnifierController.show(...);
    /// ```
    ///
    /// Four things happen in that order and three of them are load-bearing.
    ///
    /// * **The guard is on exists, not on shown.** A loupe that hid itself
    ///   mid-drag is still in the overlay, and showing a second one would
    ///   leave the first behind.
    /// * **The toolbar goes first.** A selection toolbar and a magnifier are
    ///   two things over the same text competing for the same space, and the
    ///   magnifier is the one following the reader's finger. Note that
    ///   [`SelectionHost::hide_magnifier`] does **not** bring it back: the
    ///   toolbar comes back when the gesture that raises it happens again,
    ///   not when the magnifier leaves.
    /// * **The builder is asked before anything is inserted.** A `None` style
    ///   is upstream's null builder, which is how `showMagnifier` is "safe to
    ///   call on platforms not mobile" -- desktop has no magnifier and no
    ///   entry appears. Inserting first and asking after would leave an empty
    ///   entry in the overlay on every desktop long press.
    ///
    /// Returns whether a magnifier went up.
    pub fn show_magnifier(&mut self, style: Option<MagnifierStyle>) -> bool {
        if self.magnifier_exists() {
            return false;
        }
        if self.config.toolbar_visible {
            self.set_toolbar_visible(false);
        }
        let Some(style) = style else {
            return false;
        };
        // A host whose entry was taken out is not a host to keep: upstream
        // replaces `_overlayEntry` outright here.
        self.magnifier = show_magnifier(Rc::clone(&self.overlay), style);
        self.magnifier.is_some()
    }

    /// Upstream's `SelectionOverlay.hideMagnifier`, whose guard carries the
    /// comment that explains the pair above it:
    ///
    /// > This cannot be a check on `MagnifierController.shown`, since it's
    /// > possible that the magnifier is still in the overlay, but not shown in
    /// > cases where the magnifier hides itself.
    ///
    /// The entry goes -- upstream's `hide()` defaults to `removeFromOverlay:
    /// true` -- so a later `showMagnifier` builds a fresh one.
    ///
    /// Upstream's `if (overlayEntry == null) return;` is not written out here.
    /// [`MagnifierHost::remove_from_overlay`] answers false rather than
    /// removing something twice, so the guard changes nothing and a mutation
    /// deleting it stays green. The shape is above, in the quote, rather than
    /// in a line that reads like a rule and decides nothing.
    pub fn hide_magnifier(&mut self) {
        if let Some(magnifier) = self.magnifier.as_mut() {
            magnifier.hide(true);
        }
    }

    /// Upstream's `SelectionOverlay.updateMagnifier`, which is the same guard
    /// and then the move.
    ///
    /// ```dart
    /// if (_magnifierController.overlayEntry == null) { return; }
    /// _magnifierInfo.value = magnifierInfo;
    /// ```
    ///
    /// The guard is one level down: [`MagnifierHost::update`] already refuses
    /// to place an entry that is gone, and a mutation deleting this one stayed
    /// green. Two guards for one rule means one of them is never the reason
    /// anything happens.
    pub fn update_magnifier(
        &mut self,
        info: crate::magnifier::MagnifierInfo,
        screen: Rect,
        overlay: &crate::render::RenderRef,
    ) {
        if let Some(magnifier) = self.magnifier.as_mut() {
            magnifier.update(info, screen, overlay);
        }
    }

    /// Upstream's `dispose`: all three entries go, and the magnifier with them.
    pub fn dismiss(mut self) {
        if let Some(magnifier) = self.magnifier.as_mut() {
            magnifier.remove_from_overlay();
        }
        for entry in self.entries {
            self.overlay.remove(entry);
        }
    }
}

/// Puts a selection overlay up: two handle entries and a toolbar entry.
///
/// Everything starts hidden, because there is no selection to hold before
/// [`SelectionHost::set_selection`] has been called and a handle at the origin
/// would flash in the corner for a frame.
pub fn show_selection_overlay(
    overlay: Rc<OverlayHandle>,
    controls: Rc<dyn TextSelectionControls>,
    toolbar: impl Fn() -> AnyWidget + 'static,
) -> Option<SelectionHost> {
    let geometry = OverlayGeometry::default();
    let handle_ids = (
        crate::theatre::next_surface_id(),
        crate::theatre::next_surface_id(),
    );
    let toolbar: Rc<dyn Fn() -> AnyWidget> = Rc::new(toolbar);

    let mut entries = Vec::new();
    for (end, hit_id) in [
        (HandleEnd::Start, handle_ids.0),
        (HandleEnd::End, handle_ids.1),
    ] {
        let geometry = geometry.clone();
        entries.push(overlay.insert(move || {
            crate::framework::stateful(HandleEntry {
                end,
                geometry: geometry.clone(),
                hit_id,
            })
        })?);
    }
    entries.push({
        let geometry = geometry.clone();
        let toolbar = Rc::clone(&toolbar);
        overlay.insert(move || {
            crate::framework::stateful(ToolbarEntry {
                geometry: geometry.clone(),
                toolbar: Rc::clone(&toolbar),
            })
        })?
    });

    Some(SelectionHost {
        magnifier: None,
        config: SelectionOverlay::new(),
        controls,
        overlay,
        entries,
        geometry,
        toolbar_above: Cell::new(false),
        handle_ids,
    })
}

/// A drag on a handle, remembering where in it the finger landed.
///
/// [`crate::text_selection::TextSelectionOverlay`] holds this decision; this is
/// the live half, which needs to know *which* handle was grabbed, and that is a
/// hit id.
#[derive(Default)]
pub struct HandleDrag {
    grabbed: RefCell<Option<(u64, Offset)>>,
}

impl HandleDrag {
    pub fn new() -> HandleDrag {
        HandleDrag::default()
    }

    /// Records the grab. `within` is where in the handle the finger landed,
    /// which is what stops the selection jumping to wherever the finger is.
    pub fn begin(&self, hit_id: u64, within: Offset) {
        *self.grabbed.borrow_mut() = Some((hit_id, within));
    }

    /// Which handle is being dragged.
    pub fn handle(&self) -> Option<u64> {
        self.grabbed.borrow().map(|(id, _)| id)
    }

    /// Where the selection edge goes for a finger now at `position`.
    pub fn edge_at(&self, position: Offset) -> Offset {
        match *self.grabbed.borrow() {
            Some((_, within)) => Offset::new(position.dx - within.dx, position.dy - within.dy),
            None => position,
        }
    }

    pub fn end(&self) {
        *self.grabbed.borrow_mut() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{Component, ElementTree};
    use crate::render::{
        BoxConstraints, EdgeInsets, HitTestResult, RenderBox, RenderPadding, RenderPointerRegion,
    };
    use crate::text_selection_controls::MaterialTextSelectionControls;
    use crate::theatre::{RenderTheatre, overlay};

    /// The overlay sits 40 across and 30 down from the window's origin, so a
    /// global answer and an overlay-local one are different numbers.
    const OVERLAY_ORIGIN: Offset = Offset { dx: 40.0, dy: 30.0 };
    const OVERLAY_SIZE: Size = Size {
        width: 760.0,
        height: 570.0,
    };

    /// The window, for magnifier placements.
    const SCREEN: Rect = Rect {
        left: 0.0,
        top: 0.0,
        right: 800.0,
        bottom: 600.0,
    };

    fn controls() -> Rc<dyn TextSelectionControls> {
        Rc::new(MaterialTextSelectionControls)
    }

    fn mounted() -> (ElementTree, Rc<OverlayHandle>) {
        let slot: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));

        struct Grab {
            slot: Rc<RefCell<Option<Rc<OverlayHandle>>>>,
        }
        impl Component for Grab {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.slot.borrow_mut() = OverlayHandle::of(context);
                crate::framework::leaf(|| RenderConstrainedBox::tight(400.0, 300.0))
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![overlay(crate::framework::component(Grab {
                slot: Rc::clone(&slot),
            }))],
            |mut rendered| {
                RenderPadding::new(
                    EdgeInsets::only(OVERLAY_ORIGIN.dx, OVERLAY_ORIGIN.dy, 0.0, 0.0),
                    rendered.pop().expect("the overlay"),
                )
            },
        ));
        tree.build_render_tree();
        let handle = slot.borrow().clone().expect("an overlay in scope");
        (tree, handle)
    }

    fn laid_out(tree: &mut ElementTree) -> RenderRef {
        let root = tree.build_render_tree().expect("a mounted root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        let mut discard = HitTestResult::new();
        root.hit_test(Offset::new(1.0, 1.0), &mut discard);
        root
    }

    fn theatre_of(root: &RenderRef) -> RenderRef {
        fn walk(handle: &RenderRef, found: &mut Option<RenderRef>) {
            if found.is_some() {
                return;
            }
            let kids: Vec<RenderRef> = handle.with(|object| {
                if object.as_any().downcast_ref::<RenderTheatre>().is_some() {
                    *found = Some(handle.clone());
                }
                let mut kids = Vec::new();
                object.visit_children(&mut |child, _| {
                    if let Some(child) = child.as_any().downcast_ref::<RenderRef>() {
                        kids.push(child.clone());
                    }
                });
                kids
            });
            for child in kids {
                walk(&child, found);
            }
        }
        let mut found = None;
        walk(root, &mut found);
        found.expect("a theatre under the root")
    }

    /// Whether `id` is under `at`, globally. This is what "on the screen and
    /// grabbable" means, and a visibility flag is not that.
    fn pressable(root: &RenderRef, id: u64, at: Offset) -> bool {
        let mut result = HitTestResult::new();
        root.hit_test(at, &mut result);
        result.path.iter().any(|entry| entry.target == id)
    }

    /// A toolbar that is a hit target, so hiding it is observable.
    fn toolbar(id: u64, size: Size) -> impl Fn() -> AnyWidget + 'static {
        move || {
            crate::framework::leaf(move || {
                RenderPointerRegion::new(id, RenderConstrainedBox::tight(size.width, size.height))
                    .with_behavior(crate::render::HitTestBehavior::Opaque)
            })
        }
    }

    const TOOLBAR_ID: u64 = 5150;
    const TOOLBAR_SIZE: Size = Size {
        width: 200.0,
        height: 44.0,
    };

    fn host(tree: &mut ElementTree, overlay: Rc<OverlayHandle>) -> SelectionHost {
        let host = show_selection_overlay(overlay, controls(), toolbar(TOOLBAR_ID, TOOLBAR_SIZE))
            .expect("an overlay to put it in");
        tree.rebuild_dirty();
        host
    }

    // -- Where a handle goes --------------------------------------------------------

    #[test]
    fn each_handles_body_falls_outside_the_selection_and_not_over_it() {
        // The asymmetry the Material anchors encode: a left handle anchors at
        // its right edge and a right handle at its left, so each one's square
        // corner meets the text and its round body hangs outward. Handles that
        // both anchored the same way would put one of them over the words it
        // is holding.
        let size = MaterialTextSelectionControls::HANDLE_SIZE;
        let start = SelectionEndpoint::new(Offset::new(200.0, 300.0), 16.0);
        let end = SelectionEndpoint::new(Offset::new(320.0, 300.0), 16.0);

        let left = handle_position(
            &MaterialTextSelectionControls,
            TextSelectionHandleType::Left,
            start,
        );
        let right = handle_position(
            &MaterialTextSelectionControls,
            TextSelectionHandleType::Right,
            end,
        );

        assert_eq!(
            left.dx + size,
            200.0,
            "the left handle ends where the text starts"
        );
        assert_eq!(right.dx, 320.0, "and the right one begins where it ends");
        assert!(
            left.dx < 200.0 && right.dx + size > 320.0,
            "both bodies are outside the selection"
        );
    }

    #[test]
    fn a_handle_can_be_drawn_outside_the_field_it_belongs_to() {
        // The reason the handles are in the overlay at all. A selection at the
        // very left of a field puts the left handle's box past the field's own
        // edge, where anything drawn inside the field would be clipped away --
        // and the clipped part is exactly what the reader has to grab.
        let field_left = 100.0;
        let at = handle_position(
            &MaterialTextSelectionControls,
            TextSelectionHandleType::Left,
            SelectionEndpoint::new(Offset::new(field_left, 300.0), 16.0),
        );
        assert!(at.dx < field_left, "the handle starts outside the field");
    }

    #[test]
    fn the_anchor_is_subtracted_so_the_handle_points_at_the_character() {
        let endpoint = SelectionEndpoint::new(Offset::new(200.0, 300.0), 16.0);
        let anchor =
            MaterialTextSelectionControls.handle_anchor(TextSelectionHandleType::Left, 16.0);
        let at = handle_position(
            &MaterialTextSelectionControls,
            TextSelectionHandleType::Left,
            endpoint,
        );
        assert_eq!(at, Offset::new(200.0 - anchor.dx, 300.0 - anchor.dy));
    }

    #[test]
    fn the_handles_are_placed_in_the_overlays_coordinates() {
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_handles_visible(true);
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        let start = SelectionEndpoint::new(Offset::new(200.0, 300.0), 16.0);
        let end = SelectionEndpoint::new(Offset::new(320.0, 300.0), 16.0);
        host.set_selection(start, end, false, &theatre);

        let global = handle_position(
            &MaterialTextSelectionControls,
            TextSelectionHandleType::Left,
            start,
        );
        assert_eq!(
            host.start_handle_at(),
            Offset::new(global.dx - OVERLAY_ORIGIN.dx, global.dy - OVERLAY_ORIGIN.dy)
        );
        assert_ne!(
            host.start_handle_at(),
            global,
            "the two coordinate systems differ, which is why the conversion exists"
        );
    }

    #[test]
    fn the_two_handles_follow_the_two_ends() {
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_handles_visible(true);
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.set_selection(
            SelectionEndpoint::new(Offset::new(200.0, 300.0), 16.0),
            SelectionEndpoint::new(Offset::new(320.0, 300.0), 16.0),
            false,
            &theatre,
        );
        let (start, end) = (host.start_handle_at(), host.end_handle_at());

        host.set_selection(
            SelectionEndpoint::new(Offset::new(200.0, 300.0), 16.0),
            SelectionEndpoint::new(Offset::new(500.0, 300.0), 16.0),
            false,
            &theatre,
        );
        // Each handle moved by what *its own* endpoint moved -- not by the
        // same amount, which is what a test comparing the gap between them
        // would accidentally assert. The gap also carries the two anchors'
        // difference, and they are deliberately not the same.
        assert_eq!(host.start_handle_at(), start, "the start did not move");
        assert_eq!(
            host.end_handle_at().dx - end.dx,
            180.0,
            "and the end moved exactly as far as its endpoint did"
        );
    }

    #[test]
    fn a_taller_line_gets_a_taller_handle() {
        // Upstream sizes a handle from the line height so it stays
        // proportionate to what it is holding.
        let small = MaterialTextSelectionControls.handle_size(12.0);
        let large = MaterialTextSelectionControls.handle_size(40.0);
        // Material's handle is a fixed size, which is upstream's own answer --
        // this pins that rather than assuming it scales.
        assert_eq!(
            small, large,
            "Material's handle does not scale with the line"
        );

        let cupertino = crate::text_selection_controls::CupertinoTextSelectionControls;
        assert!(
            cupertino.handle_size(40.0).height > cupertino.handle_size(12.0).height,
            "Cupertino's does"
        );
    }

    // -- On the screen, not merely flagged ------------------------------------------

    #[test]
    fn a_hidden_handle_cannot_be_grabbed() {
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        let (start_id, _) = host.handle_ids();

        host.set_handles_visible(true);
        host.set_selection(
            SelectionEndpoint::new(Offset::new(200.0, 300.0), 16.0),
            SelectionEndpoint::new(Offset::new(320.0, 300.0), 16.0),
            false,
            &theatre,
        );
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let grab = Offset::new(
            host.start_handle_at().dx + OVERLAY_ORIGIN.dx + 2.0,
            host.start_handle_at().dy + OVERLAY_ORIGIN.dy + 2.0,
        );
        assert!(
            pressable(&root, start_id, grab),
            "a shown handle is grabbable"
        );

        host.set_handles_visible(false);
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        assert!(
            !pressable(&root, start_id, grab),
            "and a hidden one is not there at all"
        );
    }

    #[test]
    fn a_collapsed_selection_has_one_handle_and_not_two() {
        // Two handles on the same point would be two grab targets for one
        // position, and the reader could not tell which they had.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_handles_visible(true);
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        let (start_id, end_id) = host.handle_ids();

        let caret = SelectionEndpoint::new(Offset::new(200.0, 300.0), 16.0);
        host.set_selection(caret, caret, true, &theatre);
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);

        let at = Offset::new(
            host.start_handle_at().dx + OVERLAY_ORIGIN.dx + 2.0,
            host.start_handle_at().dy + OVERLAY_ORIGIN.dy + 2.0,
        );
        assert!(
            pressable(&root, start_id, at),
            "the caret's handle is there"
        );
        assert!(
            !pressable(&root, end_id, at),
            "and there is not a second one under it"
        );
    }

    // -- The toolbar -----------------------------------------------------------------

    #[test]
    fn the_toolbar_sits_above_a_selection_that_has_room() {
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_toolbar_visible(true);
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        // A selection well down the page: the toolbar's 44 fits above it.
        host.place_toolbar(
            Rect::ltrb(200.0, 300.0, 320.0, 320.0),
            Rect::ltrb(100.0, 100.0, 500.0, 500.0),
            TOOLBAR_SIZE,
            &theatre,
            OVERLAY_SIZE,
        );
        assert!(host.toolbar_is_above());
        // Overlay-local: the selection's top is 300 global, 270 local, less the
        // toolbar's own height.
        assert_eq!(host.toolbar_at().dy, 270.0 - TOOLBAR_SIZE.height);
    }

    #[test]
    fn a_selection_at_the_top_puts_the_toolbar_underneath_it() {
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_toolbar_visible(true);
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        // Global top 40 is overlay-local 10, which is less than the toolbar's
        // 44 -- so it goes below.
        host.place_toolbar(
            Rect::ltrb(200.0, 40.0, 320.0, 60.0),
            Rect::ltrb(0.0, 0.0, 500.0, 500.0),
            TOOLBAR_SIZE,
            &theatre,
            OVERLAY_SIZE,
        );
        assert!(!host.toolbar_is_above());
        assert_eq!(
            host.toolbar_at().dy,
            30.0,
            "the selection's bottom, locally"
        );
    }

    #[test]
    fn the_above_or_below_test_is_made_in_the_overlays_frame() {
        // The claim `place_toolbar` documents: comparing a global anchor
        // against a local height picks the wrong side on any overlay that is
        // not at the window's origin. A selection whose top is 60 globally is
        // 30 locally -- above the toolbar's 44 by one measure and below it by
        // the other.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_toolbar_visible(true);
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.place_toolbar(
            Rect::ltrb(200.0, 60.0, 320.0, 80.0),
            Rect::ltrb(0.0, 0.0, 500.0, 500.0),
            TOOLBAR_SIZE,
            &theatre,
            OVERLAY_SIZE,
        );
        assert!(
            !host.toolbar_is_above(),
            "60 global is 30 local, and 30 is not room for a 44-tall toolbar"
        );
    }

    #[test]
    fn a_toolbar_over_a_selection_at_the_edge_is_pulled_back_on_screen() {
        // Upstream's `centerOn`: half off the screen is worse than not quite
        // over the selection.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_toolbar_visible(true);
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.place_toolbar(
            Rect::ltrb(40.0, 300.0, 60.0, 320.0),
            Rect::ltrb(0.0, 0.0, 760.0, 570.0),
            TOOLBAR_SIZE,
            &theatre,
            OVERLAY_SIZE,
        );
        assert_eq!(host.toolbar_at().dx, 0.0, "flush left rather than off it");

        host.place_toolbar(
            Rect::ltrb(780.0, 300.0, 795.0, 320.0),
            Rect::ltrb(0.0, 0.0, 1000.0, 570.0),
            TOOLBAR_SIZE,
            &theatre,
            OVERLAY_SIZE,
        );
        assert_eq!(
            host.toolbar_at().dx,
            OVERLAY_SIZE.width - TOOLBAR_SIZE.width,
            "flush right"
        );
    }

    #[test]
    fn a_hidden_toolbar_is_not_on_the_screen() {
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.set_toolbar_visible(true);
        host.place_toolbar(
            Rect::ltrb(200.0, 300.0, 320.0, 320.0),
            Rect::ltrb(100.0, 100.0, 500.0, 500.0),
            TOOLBAR_SIZE,
            &theatre,
            OVERLAY_SIZE,
        );
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let at = Offset::new(
            host.toolbar_at().dx + OVERLAY_ORIGIN.dx + 4.0,
            host.toolbar_at().dy + OVERLAY_ORIGIN.dy + 4.0,
        );
        assert!(pressable(&root, TOOLBAR_ID, at));

        host.set_toolbar_visible(false);
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        assert!(!pressable(&root, TOOLBAR_ID, at));
    }

    // -- The two go together ----------------------------------------------------------

    #[test]
    fn hiding_the_toolbar_leaves_the_handles_up() {
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_handles_visible(true);
        host.set_toolbar_visible(true);

        host.set_toolbar_visible(false);
        assert!(!host.config.toolbar_visible);
        assert!(
            host.config.handles_visible,
            "the reader can still see and move the selection's edges"
        );
    }

    #[test]
    fn hide_takes_both_away() {
        // A toolbar without handles acts on a selection whose edges the reader
        // can no longer see.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_handles_visible(true);
        host.set_toolbar_visible(true);
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        host.set_selection(
            SelectionEndpoint::new(Offset::new(200.0, 300.0), 16.0),
            SelectionEndpoint::new(Offset::new(320.0, 300.0), 16.0),
            false,
            &theatre,
        );
        host.place_toolbar(
            Rect::ltrb(200.0, 300.0, 320.0, 320.0),
            Rect::ltrb(100.0, 100.0, 500.0, 500.0),
            TOOLBAR_SIZE,
            &theatre,
            OVERLAY_SIZE,
        );
        tree.rebuild_dirty();

        host.hide();
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let (start_id, _) = host.handle_ids();
        let grab = Offset::new(
            host.start_handle_at().dx + OVERLAY_ORIGIN.dx + 2.0,
            host.start_handle_at().dy + OVERLAY_ORIGIN.dy + 2.0,
        );
        let bar = Offset::new(
            host.toolbar_at().dx + OVERLAY_ORIGIN.dx + 4.0,
            host.toolbar_at().dy + OVERLAY_ORIGIN.dy + 4.0,
        );
        assert!(!pressable(&root, start_id, grab));
        assert!(!pressable(&root, TOOLBAR_ID, bar));
    }

    // -- Lifetime ----------------------------------------------------------------------

    #[test]
    fn a_selection_overlay_is_three_entries() {
        let (mut tree, overlay) = mounted();
        let before = overlay.entry_count();
        let host = host(&mut tree, Rc::clone(&overlay));
        assert_eq!(
            overlay.entry_count(),
            before + 3,
            "two handles and a toolbar, which come and go independently"
        );
        host.dismiss();
        assert_eq!(overlay.entry_count(), before);
    }

    #[test]
    fn nothing_is_up_before_there_is_a_selection() {
        let (mut tree, overlay) = mounted();
        let host = host(&mut tree, Rc::clone(&overlay));
        let root = laid_out(&mut tree);
        let (start_id, end_id) = host.handle_ids();
        for id in [start_id, end_id, TOOLBAR_ID] {
            assert!(
                !pressable(&root, id, Offset::new(1.0, 1.0)),
                "a handle at the origin would flash in the corner for a frame"
            );
        }
    }

    // -- The grab point ------------------------------------------------------------------

    #[test]
    fn dragging_a_handle_does_not_jump_the_selection_to_the_finger() {
        // The same reasoning as the drag anchor in `drag_target`: a handle that
        // jumped would read as a different handle.
        let drag = HandleDrag::new();
        assert_eq!(
            drag.edge_at(Offset::new(200.0, 300.0)),
            Offset::new(200.0, 300.0),
            "with nothing grabbed the finger is the answer"
        );

        drag.begin(7, Offset::new(6.0, 10.0));
        assert_eq!(drag.handle(), Some(7));
        assert_eq!(
            drag.edge_at(Offset::new(200.0, 300.0)),
            Offset::new(194.0, 290.0),
            "the grab point is carried for the whole drag"
        );

        drag.end();
        assert_eq!(drag.handle(), None);
        assert_eq!(
            drag.edge_at(Offset::new(200.0, 300.0)),
            Offset::new(200.0, 300.0)
        );
    }

    #[test]
    fn the_drag_knows_which_handle_it_has() {
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_handles_visible(true);
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        host.set_selection(
            SelectionEndpoint::new(Offset::new(200.0, 300.0), 16.0),
            SelectionEndpoint::new(Offset::new(320.0, 300.0), 16.0),
            false,
            &theatre,
        );
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);

        let (start_id, end_id) = host.handle_ids();
        let on_the_end = Offset::new(
            host.end_handle_at().dx + OVERLAY_ORIGIN.dx + 2.0,
            host.end_handle_at().dy + OVERLAY_ORIGIN.dy + 2.0,
        );
        assert!(pressable(&root, end_id, on_the_end));
        assert!(
            !pressable(&root, start_id, on_the_end),
            "the two handles are 120 apart and do not overlap"
        );
    }

    // -- The magnifier, which this host had no plumbing for at all -----------

    /// A finger at `y`, with the caret on a line centred at `line`.
    fn magnifier_info(y: f32, line: f32) -> crate::magnifier::MagnifierInfo {
        crate::magnifier::MagnifierInfo::new(
            Offset::new(200.0, y),
            Rect::ltrb(200.0, line - 8.0, 202.0, line + 8.0),
            Rect::ltrb(50.0, line - 8.0, 400.0, line + 8.0),
            Rect::ltrb(50.0, 100.0, 400.0, 500.0),
        )
    }

    #[test]
    fn showing_a_magnifier_takes_the_toolbar_down_and_hiding_it_does_not_bring_it_back() {
        // Upstream: `if (toolbarIsVisible) { hideToolbar(); }`, and nothing in
        // `hideMagnifier` undoes it. Two things over the same text competing
        // for the same space, and the magnifier is the one following the
        // reader's finger. The toolbar comes back when the gesture that raises
        // it happens again, not when the magnifier leaves.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_toolbar_visible(true);
        assert!(host.config.toolbar_visible);

        assert!(host.show_magnifier(Some(MagnifierStyle::Material)));
        assert!(!host.config.toolbar_visible, "the toolbar went first");

        host.hide_magnifier();
        assert!(
            !host.config.toolbar_visible,
            "and hiding the magnifier does not put it back"
        );
    }

    #[test]
    fn a_platform_with_no_magnifier_puts_nothing_in_the_overlay() {
        // Upstream builds the magnifier before inserting anything and returns
        // on a null builder, which is what makes `showMagnifier` safe to call
        // on a desktop. Inserting first and asking after would leave an empty
        // entry behind on every desktop long press.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        let before = overlay.entry_count();

        assert!(!host.show_magnifier(None));
        assert_eq!(overlay.entry_count(), before);
        assert!(!host.magnifier_exists());

        assert!(host.show_magnifier(Some(MagnifierStyle::Material)));
        assert_eq!(overlay.entry_count(), before + 1);
    }

    #[test]
    fn but_the_toolbar_still_goes_down_on_a_platform_that_has_no_magnifier() {
        // The order in upstream's method: the toolbar is hidden **before** the
        // builder is asked. Moving the guard after it would leave a desktop's
        // toolbar up, which reads like the right answer and is not upstream's.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.set_toolbar_visible(true);
        assert!(!host.show_magnifier(None));
        assert!(!host.config.toolbar_visible);
    }

    #[test]
    fn a_second_show_is_refused_while_one_exists_even_if_it_is_not_showing() {
        // The guard is on `magnifierExists`, not `magnifierIsVisible`. The
        // Cupertino loupe hides itself when the finger goes below the line and
        // keeps its entry, so a guard on visibility would insert a second one
        // every time the finger crossed back.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        assert!(host.show_magnifier(Some(MagnifierStyle::Cupertino)));
        let count = overlay.entry_count();
        host.update_magnifier(magnifier_info(320.0, 300.0), SCREEN, &theatre);
        assert!(host.magnifier_is_visible());

        // Below the line: the loupe hides itself and keeps its entry.
        host.update_magnifier(
            magnifier_info(
                300.0 + crate::magnifier_host::CUPERTINO_HIDE_BELOW + 10.0,
                300.0,
            ),
            SCREEN,
            &theatre,
        );
        assert!(!host.magnifier_is_visible(), "not showing");
        assert!(host.magnifier_exists(), "but still there");

        assert!(
            !host.show_magnifier(Some(MagnifierStyle::Cupertino)),
            "so a second show is refused"
        );
        assert_eq!(overlay.entry_count(), count, "and nothing was inserted");
    }

    #[test]
    fn hiding_the_magnifier_takes_its_entry_out_so_the_next_show_builds_one() {
        // Upstream's `hide()` defaults to `removeFromOverlay: true`, so
        // `hideMagnifier` is the end of that magnifier rather than a pause.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        let before = overlay.entry_count();

        assert!(host.show_magnifier(Some(MagnifierStyle::Material)));
        host.hide_magnifier();
        assert!(!host.magnifier_exists());
        assert_eq!(overlay.entry_count(), before);

        assert!(host.show_magnifier(Some(MagnifierStyle::Material)));
        assert_eq!(overlay.entry_count(), before + 1);
    }

    #[test]
    fn hiding_or_moving_a_magnifier_that_is_not_there_does_nothing() {
        // Both of upstream's methods open with the same `overlayEntry == null`
        // return. A `hideMagnifier` on a field that never raised one is the
        // ordinary case, not an error.
        let (mut tree, overlay) = mounted();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        let before = overlay.entry_count();

        host.hide_magnifier();
        host.update_magnifier(magnifier_info(320.0, 300.0), SCREEN, &theatre);
        assert_eq!(overlay.entry_count(), before);
        assert!(!host.magnifier_exists());
        assert!(!host.magnifier_is_visible());
    }

    #[test]
    fn dismissing_the_overlay_takes_the_magnifier_with_it() {
        let (mut tree, overlay) = mounted();
        let before = overlay.entry_count();
        let mut host = host(&mut tree, Rc::clone(&overlay));
        host.show_magnifier(Some(MagnifierStyle::Material));
        assert!(overlay.entry_count() > before);
        host.dismiss();
        assert_eq!(overlay.entry_count(), before);
        drop(tree);
    }
}
