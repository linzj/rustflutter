// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The thing in the air: upstream's `_DragAvatar`, hosted in the overlay.
//!
//! `drag_target.rs` ports the decisions -- where the feedback sits relative to
//! the finger, which gestures count as a lift, what a target does with what is
//! offered. What it could not port is the part that needs a live tree: the
//! feedback has to be *drawn somewhere*, above everything, following the
//! pointer, and the targets under the pointer have to be *found*, which is a
//! hit test.
//!
//! Both are here.
//!
//! # Two offsets, and they are not the same offset
//!
//! Upstream's `updateDrag` keeps two, and this is the detail most worth having
//! written down:
//!
//! ```dart
//! _lastOffset = globalPosition - dragStartPoint;
//! final Offset overlaySpaceOffset = box.globalToLocal(globalPosition);
//! _overlayOffset = overlaySpaceOffset - dragStartPoint;
//! ```
//!
//! `_lastOffset` is **global** and is what the callbacks are told -- a drop
//! reports where on the screen the thing was let go. `_overlayOffset` is
//! **overlay-local** and is where the feedback is actually painted. They agree
//! only while the overlay's origin is the screen's, and an overlay that is not
//! the whole window -- one inside a panel, one under a title bar -- makes them
//! differ by exactly the overlay's own offset. Using one where the other
//! belongs puts the feedback under the finger on a full-screen overlay and
//! nowhere near it on any other, which is the kind of bug that looks like it
//! only happens on someone else's machine.
//!
//! [`RenderRef::global_to_local`](crate::render::RenderRef::global_to_local)
//! is the L0 half of that, and it is the reason this module needed L0 rather
//! than only a portal.
//!
//! # The feedback must not be hit-testable
//!
//! It sits directly under the pointer, on top of everything, which is exactly
//! where the drag target it is looking for is. Upstream wraps it in
//! `IgnorePointer` for that reason; so does this.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::drag_target::{DragAnchorStrategy, Draggable};
use crate::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent, many};
use crate::render::{
    HitTestResult, Offset, RenderBox, RenderIgnorePointer, RenderRef, RenderStack, StackPosition,
};
use crate::theatre::{EntryRefresh, OverlayHandle};

/// What a drag target is told, and what it answers.
///
/// Upstream reaches the target's `State` through a `RenderMetaData` payload on
/// the hit-test entry; here the payload is the id and this is what the id is
/// looked up in. Same shape: the hit carries an identity, and the identity
/// finds the callbacks, so nothing is looked up in a tree that may already have
/// been replaced by the next frame's.
pub struct TargetCallbacks {
    /// Upstream's `isExpectedDataType` plus `onWillAcceptWithDetails`: whether
    /// this target would take *this* item. Asked on every move, so it is
    /// expected to be cheap.
    pub will_accept: Rc<dyn Fn(u64) -> bool>,
    /// Upstream's `onAcceptWithDetails`, on the drop.
    pub on_accept: Rc<dyn Fn(u64, Offset)>,
    /// Upstream's `onMove`.
    pub on_move: Rc<dyn Fn(u64, Offset)>,
    /// Upstream's `onLeave`.
    pub on_leave: Rc<dyn Fn(u64)>,
}

impl TargetCallbacks {
    /// A target that takes anything and does nothing with it, for a caller who
    /// only wants the enter/leave bookkeeping.
    pub fn accepting() -> TargetCallbacks {
        TargetCallbacks {
            will_accept: Rc::new(|_| true),
            on_accept: Rc::new(|_, _| {}),
            on_move: Rc::new(|_, _| {}),
            on_leave: Rc::new(|_| {}),
        }
    }

    /// A target that refuses everything. It is still entered and still hears
    /// about moves -- upstream's `rejectedData` list exists so a target can
    /// say "not this one" rather than merely failing to light up.
    pub fn refusing() -> TargetCallbacks {
        TargetCallbacks {
            will_accept: Rc::new(|_| false),
            ..TargetCallbacks::accepting()
        }
    }
}

thread_local! {
    /// Which hit ids are drag targets. Upstream's is the `RenderMetaData`
    /// payload itself; a registry here, because a `HitTestEntry` carries a
    /// `u64` rather than an `Object?`.
    static TARGETS: RefCell<HashMap<u64, Rc<TargetCallbacks>>> = RefCell::new(HashMap::new());
}

/// Makes `id` a drag target. The id is the one a
/// [`RenderMetaData`](crate::render::RenderMetaData) built with the same
/// number will put on the hit-test entry.
pub fn register_target(id: u64, callbacks: TargetCallbacks) {
    TARGETS.with(|targets| targets.borrow_mut().insert(id, Rc::new(callbacks)));
}

/// Takes a target out of the registry, for a target that went away.
pub fn unregister_target(id: u64) -> bool {
    TARGETS.with(|targets| targets.borrow_mut().remove(&id).is_some())
}

fn target_callbacks(id: u64) -> Option<Rc<TargetCallbacks>> {
    TARGETS.with(|targets| targets.borrow().get(&id).cloned())
}

/// How many targets are registered, for tests.
pub fn registered_targets() -> usize {
    TARGETS.with(|targets| targets.borrow().len())
}

/// Where the feedback is, in the overlay's coordinates. Shared with the entry
/// so a move repositions it without rebuilding the feedback widget itself.
#[derive(Clone, Default)]
struct FeedbackPosition {
    at: Rc<Cell<Offset>>,
    /// What tells the entry the offset changed. Writing the cell alone leaves
    /// the entry clean, so it never rebuilds and the feedback never moves.
    refresh: EntryRefresh,
}

/// The overlay entry the feedback lives in.
struct FeedbackEntry {
    position: FeedbackPosition,
    feedback: Rc<dyn Fn() -> AnyWidget>,
    /// Upstream's `ignoringFeedbackPointer`, which is a field rather than a
    /// constant because a caller may want feedback that *is* a target -- a
    /// trash can that follows the finger, say.
    ignoring: bool,
}

impl StatefulComponent for FeedbackEntry {
    type State = u64;

    fn initial_state(&self) -> u64 {
        self.position.refresh.revision()
    }

    fn build(
        &self,
        _state: &u64,
        handle: StateHandle<u64>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        self.position.refresh.attach(handle);
        let at = self.position.at.get();
        let feedback = (self.feedback)();
        let ignoring = self.ignoring;
        many(vec![feedback], move |mut rendered| {
            let mut feedback = rendered.pop().expect("the drag feedback");
            if ignoring {
                // Directly under the pointer, and the pointer is looking for
                // what is *behind* it.
                feedback = RenderRef::new(RenderIgnorePointer::new(feedback));
            }
            RenderStack::new().push_positioned_boxed(
                feedback,
                StackPosition {
                    left: Some(at.dx),
                    top: Some(at.dy),
                    ..StackPosition::default()
                },
            )
        })
    }
}

/// A drag in progress. Upstream's `_DragAvatar`.
///
/// Made by [`start_drag`], fed by [`DragAvatar::update`], ended by
/// [`DragAvatar::finish`] or [`DragAvatar::cancel`].
pub struct DragAvatar {
    /// What is being carried. Upstream's `T data`; a caller-defined id here,
    /// which is what the target's `will_accept` is asked about.
    data: u64,
    /// Where inside the feedback the pointer is. Upstream's `dragStartPoint`,
    /// which is whatever the [`DragAnchorStrategy`] resolved at the lift.
    drag_start_point: Offset,
    /// Upstream's `feedbackOffset`: shifts the point the *hit test* is done
    /// at, not the point the feedback is drawn at. The two are separate on
    /// purpose -- a badge hanging off the fingertip is still asking about what
    /// is under the fingertip.
    /// Upstream's `axis`, kept as the whole [`Draggable`] because the lock is
    /// already ported on it -- see `Draggable::constrain_to_axis`.
    draggable: Draggable,
    /// Where the drag started, which is what an axis lock holds the other
    /// coordinate at.
    origin: Offset,

    overlay: Rc<OverlayHandle>,
    entry: u64,
    position: FeedbackPosition,

    /// Upstream's `_lastOffset`, in **global** coordinates: what the callbacks
    /// are told.
    last_offset: Offset,
    /// Upstream's `_enteredTargets`, in hit order, outermost last.
    entered: Vec<u64>,
    /// Upstream's `_activeTarget`: the first entered target that said yes.
    active: Option<u64>,
}

impl DragAvatar {
    /// Where the callbacks are told the thing is. Global.
    pub fn last_offset(&self) -> Offset {
        self.last_offset
    }

    /// Where the feedback is being drawn. Overlay-local, and not the same
    /// number -- see the module docs.
    pub fn overlay_offset(&self) -> Offset {
        self.position.at.get()
    }

    /// The target that has accepted the drag, if one has.
    pub fn active_target(&self) -> Option<u64> {
        self.active
    }

    /// The targets currently entered, outermost last.
    pub fn entered_targets(&self) -> &[u64] {
        &self.entered
    }

    /// Upstream's `updateDrag`: move the feedback, then find out what is under
    /// the pointer now.
    ///
    /// `root` is the render tree the hit test runs against, and `overlay` the
    /// render object of the theatre the feedback is in -- upstream reads the
    /// second off `overlayState.context` and this crate has to be handed it,
    /// which is the only difference.
    pub fn update(&mut self, global_position: Offset, root: &RenderRef, overlay: &RenderRef) {
        let global_position = self
            .draggable
            .constrain_to_axis(self.origin, global_position);
        self.last_offset = global_position.minus(self.drag_start_point);

        // The painted position is the *overlay's* idea of where the finger is,
        // less the same anchor. Not `last_offset`.
        let in_overlay = overlay.global_to_local(global_position, None);
        self.position
            .at
            .set(in_overlay.minus(self.drag_start_point));
        self.position.refresh.refresh();

        let mut result = HitTestResult::new();
        root.hit_test(
            global_position.plus(self.draggable.feedback_offset),
            &mut result,
        );
        self.enter_and_leave(self.targets_in(&result));
    }

    /// Upstream's `_getDragTargets`: the registered targets on the hit path,
    /// innermost first, that would consider this data at all.
    fn targets_in(&self, result: &HitTestResult) -> Vec<u64> {
        result
            .path
            .iter()
            .filter(|entry| target_callbacks(entry.target).is_some())
            .map(|entry| entry.target)
            .collect()
    }

    /// Upstream's enter/leave bookkeeping, including the early return and the
    /// condition on it.
    ///
    /// The condition is upstream's own comment, and it is subtle enough to be
    /// worth repeating: a *prefix* match is enough to bail only once something
    /// has accepted the drag, because targets below the active one are
    /// correctly ignored. With nothing accepted yet, `entered` holds every hit
    /// target, so a longer list means a new one has appeared underneath and
    /// must get its chance -- which is why the lengths must match exactly in
    /// that case.
    fn enter_and_leave(&mut self, targets: Vec<u64>) {
        let prefix_matches = !self.entered.is_empty()
            && targets.len() >= self.entered.len()
            && targets
                .iter()
                .zip(self.entered.iter())
                .all(|(now, before)| now == before);

        if prefix_matches && (self.active.is_some() || targets.len() == self.entered.len()) {
            self.report_moves();
            return;
        }

        self.leave_all_entered();

        let mut new_active = None;
        for id in targets {
            self.entered.push(id);
            let accepted =
                target_callbacks(id).is_some_and(|target| (target.will_accept)(self.data));
            if accepted {
                new_active = Some(id);
                break;
            }
        }
        self.report_moves();
        self.active = new_active;
    }

    /// Every entered target hears about the move, accepted or not.
    fn report_moves(&self) {
        for id in &self.entered {
            if let Some(target) = target_callbacks(*id) {
                (target.on_move)(self.data, self.last_offset);
            }
        }
    }

    fn leave_all_entered(&mut self) {
        for id in std::mem::take(&mut self.entered) {
            if let Some(target) = target_callbacks(id) {
                (target.on_leave)(self.data);
            }
        }
    }

    /// Upstream's `finishDrag(_DragEndKind.dropped)`: the active target, if
    /// there is one, is told; everything entered is left; the entry goes.
    ///
    /// Answers whether it was accepted.
    pub fn finish(mut self) -> bool {
        let accepted = match self.active.take() {
            Some(id) => {
                if let Some(target) = target_callbacks(id) {
                    (target.on_accept)(self.data, self.last_offset);
                }
                // Upstream removes the active target from `_enteredTargets`
                // before leaving the rest: a target that took the thing is not
                // also told the thing left.
                self.entered.retain(|entered| *entered != id);
                true
            }
            None => false,
        };
        self.leave_all_entered();
        self.overlay.remove(self.entry);
        accepted
    }

    /// Upstream's `finishDrag(_DragEndKind.canceled)`: nobody is told they
    /// took anything.
    pub fn cancel(mut self) {
        self.active = None;
        self.leave_all_entered();
        self.overlay.remove(self.entry);
    }
}

/// Lifts `data` into the air: puts `feedback` in the overlay and returns the
/// drag that follows the pointer.
///
/// `child_origin` is where the widget being dragged is on screen, which is what
/// [`DragAnchorStrategy::Child`] needs to keep the finger where it was within
/// it. `global_position` is the lift.
pub fn start_drag(
    overlay: Rc<OverlayHandle>,
    draggable: &Draggable,
    data: u64,
    global_position: Offset,
    child_origin: Offset,
    feedback: impl Fn() -> AnyWidget + 'static,
) -> Option<DragAvatar> {
    let position = FeedbackPosition::default();
    let feedback: Rc<dyn Fn() -> AnyWidget> = Rc::new(feedback);

    let entry = {
        let position = position.clone();
        let feedback = Rc::clone(&feedback);
        let ignoring = draggable.ignoring_feedback_pointer;
        overlay.insert(move || {
            crate::framework::stateful(FeedbackEntry {
                position: position.clone(),
                feedback: Rc::clone(&feedback),
                ignoring,
            })
        })?
    };

    Some(DragAvatar {
        data,
        // Upstream's `dragStartPoint`, which is the *anchor* -- where in the
        // feedback the pointer sits -- and not `Draggable::feedback_position`,
        // which is that anchor already subtracted from a pointer position.
        // `updateDrag` subtracts it itself, once per move.
        drag_start_point: draggable
            .anchor_strategy
            .resolve(global_position, child_origin),
        draggable: draggable.clone(),
        origin: global_position,
        overlay,
        entry,
        position,
        last_offset: Offset::ZERO,
        entered: Vec::new(),
        active: None,
    })
}

/// The anchor a bare [`DragAnchorStrategy`] resolves to, for a caller who is
/// placing feedback without a [`Draggable`] to ask.
pub fn anchor_for(
    strategy: DragAnchorStrategy,
    global_position: Offset,
    child_origin: Offset,
) -> Offset {
    strategy.resolve(global_position, child_origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{Component, ElementTree};
    use crate::render::{
        BoxConstraints, EdgeInsets, RenderConstrainedBox, RenderMetaData, RenderPadding,
    };
    use crate::theatre::{RenderTheatre, overlay};

    const TARGET_ID: u64 = 9101;
    const INNER_ID: u64 = 9102;

    /// A page with two drag targets, the inner one inside the outer, so the hit
    /// path has two entries and the enter/leave rules have something to order.
    struct Page;

    impl Component for Page {
        fn build(&self, _context: &mut BuildContext) -> AnyWidget {
            crate::framework::leaf(|| {
                // `translucent` on both, which is what upstream's `DragTarget`
                // passes: a target has to be found under the pointer whatever
                // it happens to be wrapping, and must not stop the target
                // behind it being found too.
                RenderMetaData::new(
                    TARGET_ID,
                    RenderPadding::new(
                        EdgeInsets::only(20.0, 20.0, 0.0, 0.0),
                        // Aligned so the inner target keeps its own 100x100
                        // rather than being stretched by the tight constraints
                        // that reach it through the padding -- a target the
                        // size of the page cannot be dragged off.
                        crate::render::RenderAlign::new(
                            crate::render::Alignment::new(-1.0, -1.0),
                            RenderMetaData::new(
                                INNER_ID,
                                RenderConstrainedBox::tight(100.0, 100.0),
                            )
                            .with_behavior(crate::render::HitTestBehavior::Translucent),
                        ),
                    ),
                )
                .with_behavior(crate::render::HitTestBehavior::Translucent)
            })
        }
    }

    /// The overlay, deliberately **not** at the window's origin: 40 across and
    /// 30 down. Every test that cares about the two offsets needs this, because
    /// at the origin the global and the overlay-local answer are the same
    /// number and the distinction cannot fail.
    const OVERLAY_ORIGIN: Offset = Offset { dx: 40.0, dy: 30.0 };

    fn mounted() -> (ElementTree, Rc<OverlayHandle>) {
        let slot: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));

        struct Grab {
            slot: Rc<RefCell<Option<Rc<OverlayHandle>>>>,
        }
        impl Component for Grab {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.slot.borrow_mut() = OverlayHandle::of(context);
                crate::framework::component(Page)
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

    /// The theatre's own handle, which is what a global position is converted
    /// against.
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

    fn clear_targets() {
        for id in [TARGET_ID, INNER_ID] {
            unregister_target(id);
        }
    }

    fn feedback(width: f32, height: f32) -> impl Fn() -> AnyWidget + 'static {
        move || crate::framework::leaf(move || RenderConstrainedBox::tight(width, height))
    }

    fn pointer_anchored() -> Draggable {
        Draggable::new().with_anchor_strategy(DragAnchorStrategy::Pointer)
    }

    // -- The two offsets --------------------------------------------------------

    #[test]
    fn the_reported_offset_is_global_and_the_painted_one_is_not() {
        // The claim the module rests on. At an overlay origin of (40, 30) they
        // differ by exactly that, and putting one where the other belongs is
        // invisible on a full-screen overlay.
        clear_targets();
        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &pointer_anchored(),
            1,
            Offset::new(100.0, 100.0),
            Offset::ZERO,
            feedback(30.0, 30.0),
        )
        .expect("an overlay to put it in");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        avatar.update(Offset::new(200.0, 150.0), &root, &theatre);

        assert_eq!(
            avatar.last_offset(),
            Offset::new(200.0, 150.0),
            "the callbacks hear where on the screen it is"
        );
        assert_eq!(
            avatar.overlay_offset(),
            Offset::new(160.0, 120.0),
            "the feedback is drawn where the overlay's own coordinates put it"
        );
        assert_ne!(
            avatar.last_offset(),
            avatar.overlay_offset(),
            "and they are not the same number"
        );
    }

    #[test]
    fn the_anchor_is_subtracted_from_both() {
        // A card grabbed by a point 12 across and 8 down keeps the finger
        // there: the feedback's corner is that far back from the pointer, in
        // whichever coordinates it is being expressed.
        clear_targets();
        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &Draggable::new(), // Child strategy, the default.
            1,
            Offset::new(112.0, 108.0),
            Offset::new(100.0, 100.0),
            feedback(30.0, 30.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        avatar.update(Offset::new(300.0, 200.0), &root, &theatre);

        assert_eq!(avatar.last_offset(), Offset::new(288.0, 192.0));
        assert_eq!(
            avatar.overlay_offset(),
            Offset::new(248.0, 162.0),
            "the same anchor, in the overlay's frame"
        );
    }

    // -- Finding what is underneath ---------------------------------------------

    #[test]
    fn the_targets_under_the_pointer_are_entered_innermost_first() {
        clear_targets();
        register_target(TARGET_ID, TargetCallbacks::accepting());
        register_target(INNER_ID, TargetCallbacks::accepting());

        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &pointer_anchored(),
            7,
            Offset::ZERO,
            Offset::ZERO,
            feedback(30.0, 30.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        // Inside the inner target: the page starts at the overlay's origin, the
        // inner one 20 further in again.
        avatar.update(Offset::new(80.0, 70.0), &root, &theatre);

        // Upstream's `firstWhere` adds each target as it asks it and stops at
        // the first that says yes -- so **entering stops at the acceptor** and
        // the targets below it are never entered at all. That is what makes a
        // prefix match sufficient to bail out once something has accepted:
        // there is nothing below the active one that could have appeared.
        assert_eq!(
            avatar.entered_targets(),
            &[INNER_ID],
            "the outer one was never reached, because the inner one took it"
        );
        assert_eq!(
            avatar.active_target(),
            Some(INNER_ID),
            "the first that said yes"
        );
        clear_targets();
    }

    #[test]
    fn a_refusing_target_is_still_entered_and_still_passed_over() {
        // Upstream's `rejectedData` list: a target says "not this one" rather
        // than merely failing to light up, so it is entered, hears the move,
        // and does not become active.
        clear_targets();
        register_target(INNER_ID, TargetCallbacks::refusing());
        register_target(TARGET_ID, TargetCallbacks::accepting());

        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &pointer_anchored(),
            7,
            Offset::ZERO,
            Offset::ZERO,
            feedback(30.0, 30.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        avatar.update(Offset::new(80.0, 70.0), &root, &theatre);

        assert!(
            avatar.entered_targets().contains(&INNER_ID),
            "entered even though it refuses"
        );
        assert_eq!(
            avatar.active_target(),
            Some(TARGET_ID),
            "the refusal passed the offer outwards"
        );
        clear_targets();
    }

    #[test]
    fn the_drop_tells_the_active_target_and_nobody_else() {
        clear_targets();
        let took: Rc<Cell<Option<u64>>> = Rc::new(Cell::new(None));
        let left: Rc<Cell<u32>> = Rc::new(Cell::new(0));

        // The inner one refuses, so both are entered and the outer is the one
        // that takes it -- which is the arrangement where "everybody entered
        // except the taker hears it left" is a claim with two sides.
        let counter = left.clone();
        register_target(
            INNER_ID,
            TargetCallbacks {
                on_leave: Rc::new(move |_| counter.set(counter.get() + 1)),
                ..TargetCallbacks::refusing()
            },
        );
        let recorder = took.clone();
        register_target(
            TARGET_ID,
            TargetCallbacks {
                on_accept: Rc::new(move |data, _| recorder.set(Some(data))),
                on_leave: Rc::new(|_| panic!("the target that took it is not told it left")),
                ..TargetCallbacks::accepting()
            },
        );

        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &pointer_anchored(),
            42,
            Offset::ZERO,
            Offset::ZERO,
            feedback(30.0, 30.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        avatar.update(Offset::new(80.0, 70.0), &root, &theatre);
        assert!(avatar.finish(), "accepted");

        assert_eq!(took.get(), Some(42));
        assert_eq!(
            left.get(),
            1,
            "the one that refused was entered and is told it left"
        );
        clear_targets();
    }

    #[test]
    fn a_cancelled_drag_tells_nobody_they_took_anything() {
        clear_targets();
        let took: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let recorder = took.clone();
        register_target(
            INNER_ID,
            TargetCallbacks {
                on_accept: Rc::new(move |_, _| recorder.set(true)),
                ..TargetCallbacks::accepting()
            },
        );

        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &pointer_anchored(),
            1,
            Offset::ZERO,
            Offset::ZERO,
            feedback(30.0, 30.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        avatar.update(Offset::new(80.0, 70.0), &root, &theatre);
        assert_eq!(
            avatar.active_target(),
            Some(INNER_ID),
            "it would have taken it"
        );
        avatar.cancel();

        assert!(!took.get());
        clear_targets();
    }

    #[test]
    fn dragging_off_every_target_leaves_them_all() {
        clear_targets();
        register_target(INNER_ID, TargetCallbacks::accepting());

        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &pointer_anchored(),
            1,
            Offset::ZERO,
            Offset::ZERO,
            feedback(30.0, 30.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        avatar.update(Offset::new(80.0, 70.0), &root, &theatre);
        assert_eq!(avatar.active_target(), Some(INNER_ID));

        avatar.update(Offset::new(700.0, 500.0), &root, &theatre);
        assert!(avatar.entered_targets().is_empty());
        assert_eq!(avatar.active_target(), None);
        clear_targets();
    }

    // -- The feedback is not what the pointer finds -------------------------------

    #[test]
    fn the_feedback_does_not_shadow_the_target_underneath_it() {
        // It sits directly under the pointer, on top of everything, which is
        // exactly where the target is. Upstream's `ignoringFeedbackPointer`.
        clear_targets();
        register_target(INNER_ID, TargetCallbacks::accepting());

        // Feedback that *would* take the hit: a plain container is not a
        // target at all, so wrapping one in `IgnorePointer` proves nothing.
        // Checked by removing the wrapper and watching this stay green, which
        // is how the first version of this test was found to be vacuous.
        let hungry = || {
            crate::framework::leaf(|| {
                crate::render::RenderPointerRegion::new(
                    7777,
                    RenderConstrainedBox::tight(200.0, 200.0),
                )
                .with_behavior(crate::render::HitTestBehavior::Opaque)
            })
        };

        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &pointer_anchored(),
            1,
            Offset::new(80.0, 70.0),
            Offset::ZERO,
            hungry,
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        avatar.update(Offset::new(80.0, 70.0), &root, &theatre);

        assert_eq!(
            avatar.active_target(),
            Some(INNER_ID),
            "a 200x200 opaque feedback right on top of the target did not swallow the hit"
        );
        clear_targets();
    }

    // -- The prefix rule ----------------------------------------------------------

    #[test]
    fn a_new_target_appearing_underneath_gets_its_chance() {
        // Upstream's own comment, and the reason the bail-out needs an exact
        // length match when nothing has accepted yet: with `_activeTarget`
        // null, `_enteredTargets` holds every hit target, so a longer list
        // means a new one appeared below and must be entered.
        clear_targets();
        register_target(TARGET_ID, TargetCallbacks::refusing());
        register_target(INNER_ID, TargetCallbacks::refusing());

        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &pointer_anchored(),
            1,
            Offset::ZERO,
            Offset::ZERO,
            feedback(10.0, 10.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        // Over the outer target only: the page's padding band.
        avatar.update(Offset::new(45.0, 35.0), &root, &theatre);
        assert_eq!(avatar.entered_targets(), &[TARGET_ID]);
        assert_eq!(avatar.active_target(), None, "everything refused");

        // Now over the inner one too.
        avatar.update(Offset::new(80.0, 70.0), &root, &theatre);
        assert_eq!(
            avatar.entered_targets(),
            &[INNER_ID, TARGET_ID],
            "the newly-appeared target was entered rather than bailed past"
        );
        clear_targets();
    }

    #[test]
    fn a_move_within_the_same_targets_reports_a_move_and_re_enters_nothing() {
        clear_targets();
        let entries: Rc<Cell<u32>> = Rc::new(Cell::new(0));
        let moves: Rc<Cell<u32>> = Rc::new(Cell::new(0));
        let counter = entries.clone();
        let mover = moves.clone();
        register_target(
            INNER_ID,
            TargetCallbacks {
                will_accept: Rc::new(move |_| {
                    counter.set(counter.get() + 1);
                    true
                }),
                on_move: Rc::new(move |_, _| mover.set(mover.get() + 1)),
                ..TargetCallbacks::accepting()
            },
        );

        let (mut tree, overlay) = mounted();
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &pointer_anchored(),
            1,
            Offset::ZERO,
            Offset::ZERO,
            feedback(10.0, 10.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        avatar.update(Offset::new(80.0, 70.0), &root, &theatre);
        assert_eq!(entries.get(), 1);

        avatar.update(Offset::new(90.0, 80.0), &root, &theatre);
        assert_eq!(entries.get(), 1, "still the same target, not asked again");
        assert_eq!(moves.get(), 2, "and told about both moves");
        clear_targets();
    }

    // -- The rules that came from drag_target.rs ----------------------------------

    #[test]
    fn an_axis_locked_drag_does_not_wander_off_its_line() {
        clear_targets();
        let (mut tree, overlay) = mounted();
        let draggable = pointer_anchored().with_axis(crate::render::Axis::Horizontal);
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &draggable,
            1,
            Offset::new(100.0, 100.0),
            Offset::ZERO,
            feedback(10.0, 10.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        avatar.update(Offset::new(300.0, 400.0), &root, &theatre);

        assert_eq!(
            avatar.last_offset(),
            Offset::new(300.0, 100.0),
            "the finger wandered down; the item did not"
        );
    }

    #[test]
    fn the_feedback_offset_moves_the_question_and_not_the_picture() {
        // Upstream hit-tests at `globalPosition + feedbackOffset` while drawing
        // at the anchor: a badge hanging off the fingertip still asks about
        // what is under the fingertip.
        clear_targets();
        register_target(INNER_ID, TargetCallbacks::accepting());

        let (mut tree, overlay) = mounted();
        let draggable = pointer_anchored().with_feedback_offset(Offset::new(40.0, 40.0));
        let mut avatar = start_drag(
            Rc::clone(&overlay),
            &draggable,
            1,
            Offset::ZERO,
            Offset::ZERO,
            feedback(10.0, 10.0),
        )
        .expect("an overlay");
        tree.rebuild_dirty();

        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        // The pointer is in the outer target's padding band; the offset pushes
        // the question into the inner one.
        avatar.update(Offset::new(45.0, 35.0), &root, &theatre);
        assert_eq!(
            avatar.active_target(),
            Some(INNER_ID),
            "the question moved by the feedback offset"
        );
        assert_eq!(
            avatar.overlay_offset(),
            Offset::new(5.0, 5.0),
            "and the picture did not"
        );
        clear_targets();
    }

    // -- The entry's lifetime -----------------------------------------------------

    #[test]
    fn the_feedback_goes_up_at_the_lift_and_down_at_the_drop() {
        clear_targets();
        let (tree, overlay) = mounted();
        let before = overlay.entry_count();

        let avatar = start_drag(
            Rc::clone(&overlay),
            &Draggable::new(),
            1,
            Offset::ZERO,
            Offset::ZERO,
            feedback(10.0, 10.0),
        )
        .expect("an overlay");
        assert_eq!(overlay.entry_count(), before + 1);

        avatar.finish();
        assert_eq!(overlay.entry_count(), before);
        drop(tree);
    }

    #[test]
    fn an_unregistered_target_is_not_a_target() {
        clear_targets();
        assert!(!unregister_target(TARGET_ID));
        register_target(TARGET_ID, TargetCallbacks::accepting());
        assert!(unregister_target(TARGET_ID));
        clear_targets();
    }
}
