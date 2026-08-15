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
/// What a pointer event carries besides a position.
///
/// The shell sends a scroll as a hover with a signal rather than as its own
/// change, so the signal has to be looked at first or a wheel turn reads as a
/// mouse being moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SignalKind {
    #[default]
    None,
    Scroll,
    ScrollInertiaCancel,
    Scale,
    /// A signal code the shell sent that this build does not know.
    Unknown,
}

impl SignalKind {
    pub fn from_raw(raw: i32) -> SignalKind {
        match raw {
            0 => SignalKind::None,
            1 => SignalKind::Scroll,
            2 => SignalKind::ScrollInertiaCancel,
            3 => SignalKind::Scale,
            _ => SignalKind::Unknown,
        }
    }
}

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
    pub signal_kind: SignalKind,
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

/// A wheel or trackpad scroll.
///
/// Carries where as well as how far, because the two things a scroll can mean
/// need different halves of it: a list moves by `delta` and does not care
/// where the pointer was, while a zoom has to keep whatever is under the
/// pointer under it and cannot be written without `local_position`.
#[derive(Clone, Copy, Debug)]
pub struct ScrollEvent {
    /// How far to scroll, in logical pixels. Positive `dy` means the content
    /// should move up -- the direction the reader is going, not the direction
    /// the finger went.
    pub delta: Offset,
    /// Where the pointer was, in the target's local coordinates.
    pub local_position: Offset,
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
    /// Fired for a wheel or trackpad scroll over this region.
    pub on_scroll: Option<Rc<dyn Fn(ScrollEvent)>>,
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

    pub fn with_scroll(mut self, handler: impl Fn(ScrollEvent) + 'static) -> Self {
        self.on_scroll = Some(Rc::new(handler));
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
            && self.on_scroll.is_none()
    }

    /// Whether this region wants drags. What tells a scrollable ancestor apart
    /// from a button on the way out of a hit test.
    pub fn wants_drag(&self) -> bool {
        self.on_drag_start.is_some()
            || self.on_drag_update.is_some()
            || self.on_drag_end.is_some()
    }
}

/// One end of a gesture: what was hit, and where it was hit.
#[derive(Clone)]
struct Target {
    handlers: Rc<PointerHandlers>,
    /// Where the press began, in this target's coordinates.
    local_origin: Offset,
}

/// What a single pressed pointer is doing.
///
/// Two targets, not one. A press inside a scrolling list of buttons is both a
/// press on a button and the beginning of a possible scroll, and those are
/// different objects: the innermost thing that can be tapped gets the tap, and
/// the innermost thing that can be dragged gets the drag. Recording only the
/// innermost hit -- which is what this did at first -- means a list whose rows
/// are tappable cannot be scrolled at all, because every press lands on a row.
struct ActivePointer {
    tap: Option<Target>,
    drag: Option<Target>,
    /// Where the press began, in view coordinates.
    origin: Offset,
    /// Movement since the press began.
    total: Offset,
    /// Set once the press travels past the slop.
    ///
    /// Separate from whether anything is being dragged: travelling past the
    /// slop always means the press is no longer a tap, even when nothing
    /// underneath wants a drag. Sliding a finger off a button cancels it
    /// whether or not the button happens to be inside a list.
    past_slop: bool,
    /// Whether the tap target has been told it is pressed.
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
        // A scroll arrives as a hover carrying a signal rather than as its own
        // change, which is the shell's encoding, so it has to be checked before
        // the change is.
        if event.signal_kind == SignalKind::Scroll {
            return self.on_scroll(root, event);
        }
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

    /// Gives a scroll to the innermost region under the pointer that wants one.
    ///
    /// Innermost-first, so a scrollable inside a scrollable takes the wheel;
    /// falling outward is what lets the page scroll when the wheel is over a
    /// row that does not scroll itself.
    fn on_scroll(&mut self, root: &dyn RenderBox, event: &PointerEvent) -> bool {
        let mut result = HitTestResult::new();
        root.hit_test(event.position, &mut result);
        for entry in &result.path {
            let Some(handlers) = &entry.handlers else { continue };
            if let Some(scroll) = &handlers.on_scroll {
                scroll(ScrollEvent {
                    delta: event.scroll_delta,
                    local_position: entry.local_position,
                });
                return true;
            }
        }
        false
    }

    fn on_down(&mut self, root: &dyn RenderBox, event: &PointerEvent) -> bool {
        let mut result = HitTestResult::new();
        root.hit_test(event.position, &mut result);

        // Innermost first. The first region that listens at all takes the tap;
        // the first that wants drags takes the drag. They are usually different
        // objects -- a row inside a list -- and looking for them separately is
        // what lets a list of buttons scroll.
        let mut tap: Option<Target> = None;
        let mut drag: Option<Target> = None;
        for entry in &result.path {
            let Some(handlers) = entry.handlers.clone() else { continue };
            if drag.is_none() && handlers.wants_drag() {
                drag = Some(Target {
                    handlers: handlers.clone(),
                    local_origin: entry.local_position,
                });
            }
            if tap.is_none() {
                tap = Some(Target {
                    handlers,
                    local_origin: entry.local_position,
                });
            }
            if tap.is_some() && drag.is_some() {
                break;
            }
        }
        if tap.is_none() && drag.is_none() {
            return false;
        }

        let mut pressed = false;
        if let Some(target) = &tap {
            let mut local_event = *event;
            local_event.local_position = target.local_origin;
            if let Some(down) = &target.handlers.on_pointer_down {
                down(&local_event);
            }
            if let Some(press_change) = &target.handlers.on_press_change {
                press_change(true);
                pressed = true;
            }
        }

        // A second down from the same pointer without an up should not leave
        // the first entry stranded.
        self.take(event.pointer_id);
        self.active.push((
            event.pointer_id,
            ActivePointer {
                tap,
                drag,
                origin: event.position,
                total: Offset::ZERO,
                past_slop: false,
                pressed,
            },
        ));
        true
    }

    fn on_move(&mut self, event: &PointerEvent) -> bool {
        let Some(active) = self.find(event.pointer_id) else {
            return false;
        };
        active.total = active.total.plus(event.delta);
        let travelled =
            (active.total.dx * active.total.dx + active.total.dy * active.total.dy).sqrt();
        let travel = event.position.minus(active.origin);
        let total = active.total;

        let starting = !active.past_slop && travelled > TOUCH_SLOP;
        if starting {
            active.past_slop = true;
        }
        let past_slop = active.past_slop;
        let was_pressed = active.pressed;
        let tap = active.tap.clone();
        let drag_target = active.drag.clone();

        // Past the slop the press is no longer a tap candidate, so the pressed
        // state comes back off -- the same thing a button does when a finger
        // slides off it. This is also what stops a scroll from selecting the row
        // it started on.
        if starting && was_pressed {
            if let Some(target) = &tap {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
            if let Some(active) = self.find(event.pointer_id) {
                active.pressed = false;
            }
        }

        if let Some(target) = &tap {
            if let Some(moved) = &target.handlers.on_pointer_move {
                let mut local_event = *event;
                local_event.local_position = target.local_origin.plus(travel);
                moved(&local_event);
            }
        }

        if let Some(target) = &drag_target {
            let drag = DragEvent {
                delta: event.delta,
                total,
                local_position: target.local_origin.plus(travel),
                pointer_id: event.pointer_id,
            };
            if starting {
                if let Some(start) = &target.handlers.on_drag_start {
                    start(drag);
                }
            }
            if past_slop {
                if let Some(update) = &target.handlers.on_drag_update {
                    update(drag);
                }
            }
        }
        true
    }

    fn on_up(&mut self, event: &PointerEvent) -> bool {
        let Some(active) = self.take(event.pointer_id) else {
            return false;
        };
        let travel = event.position.minus(active.origin);

        if let Some(target) = &active.tap {
            let local = target.local_origin.plus(travel);
            if active.pressed {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
            if let Some(up) = &target.handlers.on_pointer_up {
                let mut local_event = *event;
                local_event.local_position = local;
                up(&local_event);
            }
            // A press that travelled is not a tap, however short the travel.
            if !active.past_slop {
                if let Some(tap) = &target.handlers.on_tap {
                    tap(TapEvent { local_position: local, pointer_id: event.pointer_id });
                }
            }
        }

        if active.past_slop {
            if let Some(target) = &active.drag {
                if let Some(end) = &target.handlers.on_drag_end {
                    end(DragEvent {
                        delta: event.delta,
                        total: active.total,
                        local_position: target.local_origin.plus(travel),
                        pointer_id: event.pointer_id,
                    });
                }
            }
        }
        true
    }

    fn on_cancel(&mut self, event: &PointerEvent) -> bool {
        let Some(active) = self.take(event.pointer_id) else {
            return false;
        };
        // A cancelled press is not a tap. Only the pressed state is unwound.
        if active.pressed {
            if let Some(target) = &active.tap {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
        }
        if active.past_slop {
            if let Some(target) = &active.drag {
                if let Some(end) = &target.handlers.on_drag_end {
                    end(DragEvent {
                        delta: Offset::ZERO,
                        total: active.total,
                        local_position: target.local_origin,
                        pointer_id: event.pointer_id,
                    });
                }
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
            signal_kind: SignalKind::None,
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

    /// A row inside a scrolling list: the row takes the tap, the list takes the
    /// drag. Recording only the innermost hit means the list can never be
    /// scrolled, because every press lands on a row.
    fn nested(inner: PointerHandlers, outer: PointerHandlers) -> RenderStack {
        let mut stack = RenderStack::new().push(
            RenderPointerRegion::new(1, RenderPointerRegion::new(2, Sized(Size::square(80.0)))
                .with_handlers(inner))
                .with_handlers(outer),
        );
        stack.layout(BoxConstraints::tight(100.0, 100.0));
        stack
    }

    #[test]
    fn a_drag_reaches_the_scrollable_above_the_row_it_started_on() {
        let taps = Rc::new(RefCell::new(0));
        let dragged = Rc::new(RefCell::new(0.0_f32));
        let tap_sink = taps.clone();
        let drag_sink = dragged.clone();

        let root = nested(
            PointerHandlers::new().with_tap(move |_| *tap_sink.borrow_mut() += 1),
            PointerHandlers::new()
                .with_drag_update(move |drag| *drag_sink.borrow_mut() += drag.delta.dy),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 40.0, 40.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Move, 40.0, 70.0, 0.0, 30.0));
        router.dispatch(&root, &event(PointerChange::Up, 40.0, 70.0, 0.0, 0.0));

        assert_eq!(*dragged.borrow(), 30.0, "the list should have been scrolled");
        assert_eq!(*taps.borrow(), 0, "a scroll is not a tap on the row it began on");
    }

    #[test]
    fn a_press_that_does_not_travel_still_taps_the_row() {
        let taps = Rc::new(RefCell::new(0));
        let dragged = Rc::new(RefCell::new(0.0_f32));
        let tap_sink = taps.clone();
        let drag_sink = dragged.clone();

        let root = nested(
            PointerHandlers::new().with_tap(move |_| *tap_sink.borrow_mut() += 1),
            PointerHandlers::new()
                .with_drag_update(move |drag| *drag_sink.borrow_mut() += drag.delta.dy),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 40.0, 40.0, 0.0, 0.0));
        // Inside the slop, so it is a tap and not a scroll.
        router.dispatch(&root, &event(PointerChange::Move, 40.0, 44.0, 0.0, 4.0));
        router.dispatch(&root, &event(PointerChange::Up, 40.0, 44.0, 0.0, 0.0));

        assert_eq!(*taps.borrow(), 1);
        assert_eq!(*dragged.borrow(), 0.0);
    }

    #[test]
    fn a_scroll_finds_the_innermost_region_that_wants_one() {
        let inner = Rc::new(RefCell::new(0.0_f32));
        let outer = Rc::new(RefCell::new(0.0_f32));
        let inner_sink = inner.clone();
        let outer_sink = outer.clone();

        let root = nested(
            PointerHandlers::new().with_scroll(move |s| *inner_sink.borrow_mut() += s.delta.dy),
            PointerHandlers::new().with_scroll(move |s| *outer_sink.borrow_mut() += s.delta.dy),
        );

        let mut router = GestureRouter::new();
        let mut wheel = event(PointerChange::Hover, 40.0, 40.0, 0.0, 0.0);
        wheel.signal_kind = SignalKind::Scroll;
        wheel.scroll_delta = Offset::new(0.0, 53.0);
        assert!(router.dispatch(&root, &wheel));

        assert_eq!(*inner.borrow(), 53.0);
        assert_eq!(*outer.borrow(), 0.0, "the inner one took it");
    }

    #[test]
    fn a_scroll_falls_outward_when_the_row_under_it_does_not_scroll() {
        let outer = Rc::new(RefCell::new(0.0_f32));
        let sink = outer.clone();

        let root = nested(
            // A tappable row, which is not a scrollable.
            PointerHandlers::new().with_tap(|_| {}),
            PointerHandlers::new().with_scroll(move |s| *sink.borrow_mut() += s.delta.dy),
        );

        let mut router = GestureRouter::new();
        let mut wheel = event(PointerChange::Hover, 40.0, 40.0, 0.0, 0.0);
        wheel.signal_kind = SignalKind::Scroll;
        wheel.scroll_delta = Offset::new(0.0, -20.0);
        assert!(router.dispatch(&root, &wheel));

        assert_eq!(*outer.borrow(), -20.0);
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
