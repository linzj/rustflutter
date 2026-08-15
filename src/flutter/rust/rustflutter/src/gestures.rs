// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Pointer events and the recognisers that turn them into gestures.
//!
//! The shell hands over a stream of pointer events in physical pixels. This
//! module converts them to logical pixels, hit-tests each one against the
//! render tree that was painted last, and decides which of them add up to a
//! tap or a drag.
//!
//! # Where the callbacks live
//!
//! A widget tree is rebuilt every frame, so a handler cannot be stored beside
//! the widget that declared it -- by the time the pointer arrives, that widget
//! is gone. It lives on the *render object* instead ([`crate::render::
//! RenderPointerRegion`]), which is what hit testing finds, and it is an
//! `Rc<dyn Fn>` rather than a `FnMut` so that finding it does not require a
//! mutable borrow of the tree that is being searched.
//!
//! What the handler mutates is a [`crate::framework::StateHandle`] it captured
//! at build time. That is the whole loop: build captures a handle, the pointer
//! finds the handler, the handler calls `set_state`, the element is marked
//! dirty, the next frame rebuilds that subtree.
//!
//! # What is recognised
//!
//! Tap and drag, with the arbitration rule that matters: a press that moves
//! further than [`TOUCH_SLOP`] stops being a candidate tap and becomes a drag.
//! Upstream this is a `GestureArena` where several recognisers compete and one
//! wins; with two recognisers the arena collapses into a single distance test,
//! and pretending otherwise would be more machinery than decision.

use std::rc::Rc;

use crate::render::{HitTestResult, Offset, RenderBox};

/// How far a pointer may move and still count as a tap, in logical pixels.
/// Upstream's `kTouchSlop`.
pub const TOUCH_SLOP: f32 = 18.0;

/// What happened to a pointer. Mirrors `flutter::PointerData::Change`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerChange {
    Cancel,
    Add,
    Remove,
    Hover,
    Down,
    Move,
    Up,
    PanZoomStart,
    PanZoomUpdate,
    PanZoomEnd,
    /// A change code the shell sent that this build does not know.
    Unknown,
}

impl PointerChange {
    // Called only from the C ABI in app.rs, which is compiled out under
    // cfg(test) -- see engine_test_stubs.rs for why.
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn from_code(code: i32) -> PointerChange {
        match code {
            0 => PointerChange::Cancel,
            1 => PointerChange::Add,
            2 => PointerChange::Remove,
            3 => PointerChange::Hover,
            4 => PointerChange::Down,
            5 => PointerChange::Move,
            6 => PointerChange::Up,
            7 => PointerChange::PanZoomStart,
            8 => PointerChange::PanZoomUpdate,
            9 => PointerChange::PanZoomEnd,
            _ => PointerChange::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Touch,
    Mouse,
    Stylus,
    InvertedStylus,
    Trackpad,
    Unknown,
}

impl PointerKind {
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn from_code(code: i32) -> PointerKind {
        match code {
            0 => PointerKind::Touch,
            1 => PointerKind::Mouse,
            2 => PointerKind::Stylus,
            3 => PointerKind::InvertedStylus,
            4 => PointerKind::Trackpad,
            _ => PointerKind::Unknown,
        }
    }
}

/// One pointer event, in logical pixels relative to the view.
#[derive(Clone, Copy, Debug)]
pub struct PointerEvent {
    pub view_id: i64,
    pub device: i64,
    pub pointer_id: i64,
    pub change: PointerChange,
    pub kind: PointerKind,
    /// Bitmask; bit 0 is the primary button.
    pub buttons: i32,
    pub time_stamp_micros: i64,
    /// Where the pointer is, in the view's logical coordinates.
    pub position: Offset,
    /// Movement since the previous event, in logical pixels.
    pub delta: Offset,
    pub scroll_delta: Offset,
    pub pressure: f64,
    /// Where the pointer is in the coordinates of the object that was hit.
    /// Equal to `position` until a hit test fills it in.
    pub local_position: Offset,
}

impl PointerEvent {
    pub fn is_down(&self) -> bool {
        self.change == PointerChange::Down
    }

    pub fn is_up(&self) -> bool {
        self.change == PointerChange::Up
    }
}

/// A completed tap.
#[derive(Clone, Copy, Debug)]
pub struct TapEvent {
    /// Where the finger came up, in the target's local coordinates.
    pub local_position: Offset,
    pub pointer_id: i64,
}

/// A drag in progress.
#[derive(Clone, Copy, Debug)]
pub struct DragEvent {
    /// Movement since the last drag event.
    pub delta: Offset,
    /// Movement since the press began.
    pub total: Offset,
    pub local_position: Offset,
    pub pointer_id: i64,
}

/// The callbacks a [`crate::render::RenderPointerRegion`] can carry.
///
/// Every one is `Rc<dyn Fn>`: hit testing walks the tree behind a shared
/// reference, so a handler cannot ask for `&mut`. Capture a `StateHandle` and
/// call `set_state` instead.
#[derive(Default, Clone)]
pub struct PointerHandlers {
    pub on_pointer_down: Option<Rc<dyn Fn(&PointerEvent)>>,
    pub on_pointer_move: Option<Rc<dyn Fn(&PointerEvent)>>,
    pub on_pointer_up: Option<Rc<dyn Fn(&PointerEvent)>>,
    /// Fired when the press ends without having travelled past [`TOUCH_SLOP`].
    pub on_tap: Option<Rc<dyn Fn(TapEvent)>>,
    /// Fired once, when a press first travels past [`TOUCH_SLOP`].
    pub on_drag_start: Option<Rc<dyn Fn(DragEvent)>>,
    pub on_drag_update: Option<Rc<dyn Fn(DragEvent)>>,
    pub on_drag_end: Option<Rc<dyn Fn(DragEvent)>>,
    /// Fired when a press begins or ends over this region, so a button can
    /// show that it is being held.
    pub on_press_change: Option<Rc<dyn Fn(bool)>>,
}

impl PointerHandlers {
    pub fn new() -> PointerHandlers {
        PointerHandlers::default()
    }

    pub fn with_tap(mut self, handler: impl Fn(TapEvent) + 'static) -> Self {
        self.on_tap = Some(Rc::new(handler));
        self
    }

    pub fn with_press_change(mut self, handler: impl Fn(bool) + 'static) -> Self {
        self.on_press_change = Some(Rc::new(handler));
        self
    }

    pub fn with_drag_start(mut self, handler: impl Fn(DragEvent) + 'static) -> Self {
        self.on_drag_start = Some(Rc::new(handler));
        self
    }

    pub fn with_drag_update(mut self, handler: impl Fn(DragEvent) + 'static) -> Self {
        self.on_drag_update = Some(Rc::new(handler));
        self
    }

    pub fn with_drag_end(mut self, handler: impl Fn(DragEvent) + 'static) -> Self {
        self.on_drag_end = Some(Rc::new(handler));
        self
    }

    pub fn with_pointer_down(mut self, handler: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_down = Some(Rc::new(handler));
        self
    }

    pub fn with_pointer_move(mut self, handler: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_move = Some(Rc::new(handler));
        self
    }

    pub fn with_pointer_up(mut self, handler: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_up = Some(Rc::new(handler));
        self
    }

    /// Whether anything is listening. A region with nothing attached still
    /// participates in hit testing, so it can shield what is behind it.
    pub fn is_empty(&self) -> bool {
        self.on_pointer_down.is_none()
            && self.on_pointer_move.is_none()
            && self.on_pointer_up.is_none()
            && self.on_tap.is_none()
            && self.on_drag_start.is_none()
            && self.on_drag_update.is_none()
            && self.on_drag_end.is_none()
            && self.on_press_change.is_none()
    }
}

/// What a single pressed pointer is doing.
struct ActivePointer {
    target: u64,
    handlers: Rc<PointerHandlers>,
    /// Where the press began, in view coordinates.
    origin: Offset,
    /// Where the press began, in the target's coordinates.
    local_origin: Offset,
    /// Movement since the press began.
    total: Offset,
    /// Set once the press travels past the slop and becomes a drag.
    dragging: bool,
    /// Whether the target has been told it is pressed.
    pressed: bool,
}

/// Routes pointer events to the render tree and recognises gestures.
///
/// One per view. It holds no borrow of the tree between calls; each dispatch is
/// given the tree that was painted last.
pub struct GestureRouter {
    active: Vec<(i64, ActivePointer)>,
}

impl GestureRouter {
    pub fn new() -> GestureRouter {
        GestureRouter { active: Vec::new() }
    }

    /// How many pointers are currently pressed. Used by tests.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    fn take(&mut self, pointer_id: i64) -> Option<ActivePointer> {
        let index = self.active.iter().position(|(id, _)| *id == pointer_id)?;
        Some(self.active.remove(index).1)
    }

    fn find(&mut self, pointer_id: i64) -> Option<&mut ActivePointer> {
        self.active
            .iter_mut()
            .find(|(id, _)| *id == pointer_id)
            .map(|(_, pointer)| pointer)
    }

    /// Dispatches one event against `root`, the render tree painted last frame.
    ///
    /// Returns whether anything handled it, which is what a caller would use to
    /// decide whether to let the platform have the event instead.
    pub fn dispatch(&mut self, root: &dyn RenderBox, event: &PointerEvent) -> bool {
        match event.change {
            PointerChange::Down => self.on_down(root, event),
            PointerChange::Move => self.on_move(event),
            PointerChange::Up => self.on_up(event),
            PointerChange::Cancel | PointerChange::Remove => self.on_cancel(event),
            // Hover, add, and the pan-zoom family have no recogniser yet. They
            // are accepted so the shell's contract holds.
            _ => false,
        }
    }

    fn on_down(&mut self, root: &dyn RenderBox, event: &PointerEvent) -> bool {
        let mut result = HitTestResult::new();
        root.hit_test(event.position, &mut result);
        let Some(entry) = result.innermost() else {
            return false;
        };
        let Some(handlers) = entry.handlers.clone() else {
            return false;
        };

        let mut local_event = *event;
        local_event.local_position = entry.local_position;
        if let Some(down) = &handlers.on_pointer_down {
            down(&local_event);
        }
        let pressed = handlers.on_press_change.clone();
        if let Some(press_change) = &pressed {
            press_change(true);
        }

        // A second down from the same pointer without an up should not leave
        // the first entry stranded.
        self.take(event.pointer_id);
        self.active.push((
            event.pointer_id,
            ActivePointer {
                target: entry.target,
                handlers,
                origin: event.position,
                local_origin: entry.local_position,
                total: Offset::ZERO,
                dragging: false,
                pressed: pressed.is_some(),
            },
        ));
        true
    }

    fn on_move(&mut self, event: &PointerEvent) -> bool {
        let Some(active) = self.find(event.pointer_id) else {
            return false;
        };
        active.total = active.total.plus(event.delta);
        let travelled = (active.total.dx * active.total.dx + active.total.dy * active.total.dy).sqrt();

        let local = active
            .local_origin
            .plus(event.position.minus(active.origin));
        let drag = DragEvent {
            delta: event.delta,
            total: active.total,
            local_position: local,
            pointer_id: event.pointer_id,
        };
        let handlers = Rc::clone(&active.handlers);
        let starting = !active.dragging && travelled > TOUCH_SLOP;
        if starting {
            active.dragging = true;
        }
        let dragging = active.dragging;
        let was_pressed = active.pressed;

        // Past the slop the press is no longer a tap candidate, so the pressed
        // state comes back off -- the same thing a button does when a finger
        // slides off it.
        if starting && was_pressed {
            if let Some(press_change) = &handlers.on_press_change {
                press_change(false);
            }
            if let Some(active) = self.find(event.pointer_id) {
                active.pressed = false;
            }
        }

        let mut local_event = *event;
        local_event.local_position = local;
        if let Some(moved) = &handlers.on_pointer_move {
            moved(&local_event);
        }
        if starting {
            if let Some(start) = &handlers.on_drag_start {
                start(drag);
            }
        }
        if dragging {
            if let Some(update) = &handlers.on_drag_update {
                update(drag);
            }
        }
        true
    }

    fn on_up(&mut self, event: &PointerEvent) -> bool {
        let Some(active) = self.take(event.pointer_id) else {
            return false;
        };
        let handlers = active.handlers;
        let local = active
            .local_origin
            .plus(event.position.minus(active.origin));

        if active.pressed {
            if let Some(press_change) = &handlers.on_press_change {
                press_change(false);
            }
        }

        let mut local_event = *event;
        local_event.local_position = local;
        if let Some(up) = &handlers.on_pointer_up {
            up(&local_event);
        }

        if active.dragging {
            if let Some(end) = &handlers.on_drag_end {
                end(DragEvent {
                    delta: event.delta,
                    total: active.total,
                    local_position: local,
                    pointer_id: event.pointer_id,
                });
            }
        } else if let Some(tap) = &handlers.on_tap {
            tap(TapEvent { local_position: local, pointer_id: event.pointer_id });
        }
        let _ = active.target;
        true
    }

    fn on_cancel(&mut self, event: &PointerEvent) -> bool {
        let Some(active) = self.take(event.pointer_id) else {
            return false;
        };
        // A cancelled press is not a tap. Only the pressed state is unwound.
        if active.pressed {
            if let Some(press_change) = &active.handlers.on_press_change {
                press_change(false);
            }
        }
        if active.dragging {
            if let Some(end) = &active.handlers.on_drag_end {
                end(DragEvent {
                    delta: Offset::ZERO,
                    total: active.total,
                    local_position: active.local_origin,
                    pointer_id: event.pointer_id,
                });
            }
        }
        true
    }
}

impl Default for GestureRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{
        BoxConstraints, Offset, PaintContext, RenderPointerRegion, RenderStack, Size,
        StackPosition,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Sized(Size);

    impl RenderBox for Sized {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            constraints.constrain(self.0)
        }
        fn size(&self) -> Size {
            self.0
        }
        fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
    }

    fn event(change: PointerChange, x: f32, y: f32, dx: f32, dy: f32) -> PointerEvent {
        PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change,
            kind: PointerKind::Touch,
            buttons: 1,
            time_stamp_micros: 0,
            position: Offset::new(x, y),
            delta: Offset::new(dx, dy),
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: Offset::new(x, y),
        }
    }

    /// A 40x40 region at (10, 10) inside a 100x100 stack.
    fn tree(handlers: PointerHandlers) -> RenderStack {
        let mut stack = RenderStack::new()
            .push(Sized(Size::square(100.0)))
            .push_positioned(
                RenderPointerRegion::new(7, Sized(Size::square(40.0)))
                    .with_handlers(handlers),
                StackPosition { left: Some(10.0), top: Some(10.0), ..Default::default() },
            );
        stack.layout(BoxConstraints::tight(100.0, 100.0));
        stack
    }

    #[test]
    fn a_press_and_release_in_place_is_a_tap() {
        let taps = Rc::new(RefCell::new(Vec::new()));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |tap| {
            sink.borrow_mut().push(tap.local_position);
        }));

        let mut router = GestureRouter::new();
        assert!(router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0)));
        assert_eq!(router.active_count(), 1);
        assert!(router.dispatch(&root, &event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0)));
        assert_eq!(router.active_count(), 0);

        // One tap, reported in the region's own coordinates.
        assert_eq!(taps.borrow().len(), 1);
        assert_eq!(taps.borrow()[0], Offset::new(10.0, 10.0));
    }

    #[test]
    fn a_press_outside_the_region_is_not_a_tap() {
        let taps = Rc::new(RefCell::new(0));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |_| {
            *sink.borrow_mut() += 1;
        }));

        let mut router = GestureRouter::new();
        assert!(!router.dispatch(&root, &event(PointerChange::Down, 80.0, 80.0, 0.0, 0.0)));
        router.dispatch(&root, &event(PointerChange::Up, 80.0, 80.0, 0.0, 0.0));
        assert_eq!(*taps.borrow(), 0);
    }

    #[test]
    fn moving_past_the_slop_turns_the_press_into_a_drag() {
        let log = Rc::new(RefCell::new(Vec::<&'static str>::new()));
        let starts = log.clone();
        let updates = log.clone();
        let ends = log.clone();
        let taps = log.clone();
        let root = tree(
            PointerHandlers::new()
                .with_drag_start(move |_| starts.borrow_mut().push("start"))
                .with_drag_update(move |_| updates.borrow_mut().push("update"))
                .with_drag_end(move |_| ends.borrow_mut().push("end"))
                .with_tap(move |_| taps.borrow_mut().push("tap")),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        // Below the slop: no drag yet.
        router.dispatch(&root, &event(PointerChange::Move, 25.0, 20.0, 5.0, 0.0));
        assert!(log.borrow().is_empty());
        // Past it.
        router.dispatch(&root, &event(PointerChange::Move, 45.0, 20.0, 20.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Up, 45.0, 20.0, 0.0, 0.0));

        assert_eq!(*log.borrow(), vec!["start", "update", "end"]);
    }

    #[test]
    fn a_small_wobble_still_counts_as_a_tap() {
        let taps = Rc::new(RefCell::new(0));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |_| *sink.borrow_mut() += 1));

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Move, 23.0, 22.0, 3.0, 2.0));
        router.dispatch(&root, &event(PointerChange::Up, 23.0, 22.0, 0.0, 0.0));
        assert_eq!(*taps.borrow(), 1);
    }

    #[test]
    fn the_pressed_state_goes_on_and_off() {
        let states = Rc::new(RefCell::new(Vec::new()));
        let sink = states.clone();
        let root = tree(
            PointerHandlers::new().with_press_change(move |pressed| sink.borrow_mut().push(pressed)),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        assert_eq!(*states.borrow(), vec![true]);
        router.dispatch(&root, &event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0));
        assert_eq!(*states.borrow(), vec![true, false]);
    }

    #[test]
    fn sliding_off_clears_the_pressed_state_without_tapping() {
        let states = Rc::new(RefCell::new(Vec::new()));
        let taps = Rc::new(RefCell::new(0));
        let state_sink = states.clone();
        let tap_sink = taps.clone();
        let root = tree(
            PointerHandlers::new()
                .with_press_change(move |pressed| state_sink.borrow_mut().push(pressed))
                .with_tap(move |_| *tap_sink.borrow_mut() += 1),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Move, 60.0, 20.0, 40.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Up, 60.0, 20.0, 0.0, 0.0));

        assert_eq!(*states.borrow(), vec![true, false]);
        assert_eq!(*taps.borrow(), 0);
    }

    #[test]
    fn a_cancelled_press_is_not_a_tap() {
        let taps = Rc::new(RefCell::new(0));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |_| *sink.borrow_mut() += 1));

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Cancel, 20.0, 20.0, 0.0, 0.0));
        assert_eq!(*taps.borrow(), 0);
        assert_eq!(router.active_count(), 0);
    }

    #[test]
    fn an_up_without_a_down_is_ignored() {
        let root = tree(PointerHandlers::new().with_tap(|_| panic!("should not fire")));
        let mut router = GestureRouter::new();
        assert!(!router.dispatch(&root, &event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0)));
    }
}
