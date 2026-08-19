//! Drags counted one finger at a time -- a port of upstream's
//! `gestures/multidrag.dart`.
//!
//! The ordinary drag recognisers watch *the* pointer: whichever finger got
//! there first owns the gesture, and a second finger is either ignored or
//! folded into a scale. The recognisers here watch each pointer **separately**,
//! so two fingers on a board can drag two pieces at once, and each one is
//! judged, started and ended on its own.
//!
//! The substance of the upstream file is one small state machine repeated per
//! pointer, and the four recognisers differ only in two of its answers:
//!
//! * **when a movement means yes** -- any direction far enough, horizontally
//!   far enough, vertically far enough, or (for the delayed one) *not* having
//!   moved far at all before the delay elapsed;
//! * **when the drag actually begins** -- at once, or parked until the delay
//!   passes.
//!
//! Everything else -- accumulating the movement that happened before anyone
//! decided it was a drag, handing that accumulation to the client in one lump,
//! and clearing the client before telling it anything -- is shared, and is
//! ported here once.
//!
//! ## What is not here
//!
//! Upstream each recogniser joins the gesture arena itself. This crate's arena
//! lives inside [`GestureRouter`](crate::gestures::GestureRouter) and is keyed
//! to the router's own recogniser kinds, so a recogniser written outside it
//! has no seat to take. Instead a state records the verdict it *would* give
//! the arena and a caller drains it with
//! [`MultiDragGestureRecognizer::take_resolution`]. The state machine, which
//! is what the upstream file is, is ported whole.

use crate::gestures::{Disposition, PointerEvent, PointerKind, VelocityTracker, compute_hit_slop};
use crate::render::Offset;
use crate::resampler::Drag;

/// Upstream's `kLongPressTimeout` as the delayed recogniser's default, in
/// microseconds: the delay is the long-press timeout so that holding still to
/// pick something up feels the same as holding still to long-press it.
pub const DEFAULT_MULTI_DRAG_DELAY_MICROS: i64 = crate::gestures::LONG_PRESS_TIMEOUT_MICROS;

/// Which of upstream's four private per-pointer states this one is.
///
/// The four subclasses of `MultiDragPointerState` in the upstream file carry
/// no state of their own beyond the delayed one's timer; they are two method
/// overrides each. An enum says the same thing without four near-identical
/// structs, and keeps the two decisions that actually differ side by side
/// where they can be compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiDragPolicy {
    /// Upstream `_ImmediatePointerState`: any movement past the slop, in any
    /// direction.
    Immediate,
    /// Upstream `_HorizontalPointerState`: horizontal movement past the slop.
    Horizontal,
    /// Upstream `_VerticalPointerState`: vertical movement past the slop.
    Vertical,
    /// Upstream `_DelayedPointerState`: the finger has to stay put until the
    /// delay elapses. Movement past the slop before then is a **rejection**,
    /// the opposite of what it means to the other three.
    Delayed { delay_micros: i64 },
}

/// What a state wants done now that its delay has elapsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayOutcome {
    /// The arena had already accepted and a start was parked: begin the drag.
    StartDrag,
    /// Nobody has accepted yet, so the elapsed delay is this state's argument
    /// that the gesture is its: tell the arena so.
    ResolveAccepted,
}

/// Upstream `MultiDragPointerState`: everything one pointer is doing.
///
/// A recogniser holds one of these per pointer, which is the whole reason
/// this family exists.
pub struct MultiDragPointerState {
    /// Where the pointer touched down, in global coordinates. The drag is
    /// started *here* rather than wherever the finger has drifted to, so a
    /// caller that picks a thing up picks up the thing that was under the
    /// finger when it landed.
    pub initial_position: Offset,
    pub kind: PointerKind,
    policy: MultiDragPolicy,
    velocity: VelocityTracker,
    /// Upstream's `_pendingDelta`: movement that has happened but not yet been
    /// reported, because nobody has decided this is a drag.
    ///
    /// `None` once a client is listening (from then on movement goes straight
    /// out) and once the state is finished. The two are distinguished by
    /// `client`, exactly as upstream distinguishes them.
    pending_delta: Option<Offset>,
    last_pending_event_micros: Option<i64>,
    client: Option<Box<dyn Drag>>,
    in_arena: bool,
    /// Delayed only: when the delay elapses, or `None` once it has elapsed or
    /// been stopped. Upstream this is a `Timer`; frames here are on demand, so
    /// it is a deadline the recogniser's `tick` walks past -- the same shape
    /// the router already uses for long press.
    deadline_micros: Option<i64>,
    /// Delayed only: the arena accepted before the delay elapsed, so the start
    /// is waiting for it. Upstream's `_starter`, which is a callback; here the
    /// recogniser holds the callback end, so only the fact that one is parked
    /// needs recording.
    parked_start: bool,
    resolution: Option<Disposition>,
}

impl MultiDragPointerState {
    pub fn new(
        initial_position: Offset,
        kind: PointerKind,
        policy: MultiDragPolicy,
        now_micros: i64,
    ) -> MultiDragPointerState {
        MultiDragPointerState {
            initial_position,
            kind,
            policy,
            velocity: VelocityTracker::new(),
            pending_delta: Some(Offset::ZERO),
            last_pending_event_micros: None,
            client: None,
            in_arena: false,
            deadline_micros: match policy {
                MultiDragPolicy::Delayed { delay_micros } => Some(now_micros + delay_micros),
                _ => None,
            },
            parked_start: false,
            resolution: None,
        }
    }

    /// Upstream's `pendingDelta` getter.
    pub fn pending_delta(&self) -> Option<Offset> {
        self.pending_delta
    }

    /// Whether a client is listening -- whether, in upstream's terms, the drag
    /// has started.
    pub fn has_client(&self) -> bool {
        self.client.is_some()
    }

    /// Upstream's `_setArenaEntry`.
    pub fn set_arena_entry(&mut self) {
        self.in_arena = true;
    }

    /// Upstream's `resolve`: what this state tells the arena.
    pub fn resolve(&mut self, disposition: Disposition) {
        if self.in_arena {
            self.resolution = Some(disposition);
        }
    }

    /// Takes the verdict this state has for the arena, if it has one.
    pub fn take_resolution(&mut self) -> Option<Disposition> {
        self.resolution.take()
    }

    /// Upstream's `_move`.
    ///
    /// Two quite different jobs behind one name. Once a client is listening
    /// the movement goes straight out. Before that it *accumulates*, because
    /// the movement between touching down and being recognised is real
    /// movement that the eventual client will want -- see [`Self::start_drag`].
    pub fn handle_move(&mut self, event: &PointerEvent) {
        self.velocity
            .add_position(event.time_stamp_micros, event.position);
        if let Some(client) = self.client.as_mut() {
            client.update(event.delta);
            return;
        }
        let pending = match self.pending_delta {
            Some(pending) => pending.plus(event.delta),
            None => return,
        };
        self.pending_delta = Some(pending);
        self.last_pending_event_micros = Some(event.time_stamp_micros);
        self.check_for_resolution_after_move();
    }

    /// Upstream's `checkForResolutionAfterMove`, all four overrides together.
    ///
    /// The first three read "far enough to be a drag"; the fourth reads "too
    /// far to still be a hold". Same measurement, opposite verdicts, and the
    /// difference is the whole reason a reorderable list can use the delayed
    /// one inside a scroll view: there, moving early means the reader is
    /// scrolling, and the recogniser has to get out of the way.
    fn check_for_resolution_after_move(&mut self) {
        let Some(pending) = self.pending_delta else {
            return;
        };
        let slop = compute_hit_slop(self.kind);
        match self.policy {
            MultiDragPolicy::Immediate => {
                if pending.distance() > slop {
                    self.resolve(Disposition::Accepted);
                }
            }
            MultiDragPolicy::Horizontal => {
                if pending.dx.abs() > slop {
                    self.resolve(Disposition::Accepted);
                }
            }
            MultiDragPolicy::Vertical => {
                if pending.dy.abs() > slop {
                    self.resolve(Disposition::Accepted);
                }
            }
            MultiDragPolicy::Delayed { .. } => {
                // Upstream's comment is about the case this guard covers: once
                // the timer has been stopped but the drag never started, we
                // keep being asked and there is nothing left to decide.
                if self.deadline_micros.is_none() {
                    return;
                }
                if pending.distance() > slop {
                    self.resolve(Disposition::Rejected);
                    self.deadline_micros = None;
                }
            }
        }
    }

    /// Moves the delayed variant's clock forward; upstream's `_delayPassed`.
    ///
    /// Returns what the recogniser should do about it, if anything. Whether
    /// the delay or the arena's acceptance comes first is genuinely
    /// undetermined, and this is the half of the handshake that notices it was
    /// second.
    pub fn tick(&mut self, now_micros: i64) -> Option<DelayOutcome> {
        let deadline = self.deadline_micros?;
        if now_micros < deadline {
            return None;
        }
        self.deadline_micros = None;
        if self.parked_start {
            self.parked_start = false;
            Some(DelayOutcome::StartDrag)
        } else {
            self.resolve(Disposition::Accepted);
            Some(DelayOutcome::ResolveAccepted)
        }
    }

    /// Whether this state is still waiting on a deadline.
    pub fn awaits_deadline(&self) -> bool {
        self.deadline_micros.is_some()
    }

    /// Upstream's `accepted`, reported rather than called.
    ///
    /// Upstream is handed a `starter` callback and either calls it now or
    /// stores it for later. Here the recogniser owns the callback end -- it is
    /// the recogniser that has `on_start` -- so the state answers **whether**
    /// to start instead of starting. The ordering is upstream's exactly: the
    /// three immediate policies start now, and the delayed one starts now only
    /// if its delay has already elapsed.
    pub fn accepted(&mut self) -> bool {
        match self.policy {
            MultiDragPolicy::Delayed { .. } => {
                if self.deadline_micros.is_none() {
                    true
                } else {
                    self.parked_start = true;
                    false
                }
            }
            _ => true,
        }
    }

    /// Upstream's `rejected`.
    pub fn rejected(&mut self) {
        self.pending_delta = None;
        self.last_pending_event_micros = None;
        self.in_arena = false;
        self.deadline_micros = None;
    }

    /// Upstream's `_startDrag`.
    ///
    /// The client is told, as its very first update, about **all the movement
    /// that happened before it existed**. A finger that travelled thirty
    /// pixels before anyone agreed it was dragging has moved the thing thirty
    /// pixels, and without this lump the thing would jump by that much on the
    /// next ordinary move instead.
    pub fn start_drag(&mut self, client: Box<dyn Drag>) {
        let pending = self.pending_delta.take().unwrap_or(Offset::ZERO);
        self.last_pending_event_micros = None;
        self.client = Some(client);
        // Upstream: "Call client last to avoid reentrancy." The client may do
        // anything, including tearing down whatever owns this state, so this
        // has to be the last thing touched.
        self.client.as_mut().unwrap().update(pending);
    }

    /// Upstream's `_up`.
    pub fn handle_up(&mut self, now_micros: i64) {
        // Upstream clears `_client` *before* calling `end` on it, for the same
        // reentrancy reason. Taking it does both at once.
        if let Some(mut client) = self.client.take() {
            let velocity = self.velocity.fling_velocity(now_micros, self.kind);
            client.end(velocity);
        } else {
            self.pending_delta = None;
            self.last_pending_event_micros = None;
        }
    }

    /// Upstream's `_cancel`.
    pub fn handle_cancel(&mut self) {
        if let Some(mut client) = self.client.take() {
            client.cancel();
        } else {
            self.pending_delta = None;
            self.last_pending_event_micros = None;
        }
    }

    /// Upstream's `dispose`: a state that goes away while still in the arena
    /// tells the arena it is out.
    pub fn dispose(&mut self) {
        if self.in_arena {
            self.resolution = Some(Disposition::Rejected);
            self.in_arena = false;
        }
        self.pending_delta = None;
        self.deadline_micros = None;
    }
}

/// Upstream `MultiDragGestureRecognizer`: one drag per pointer.
///
/// Not meant to be used directly upstream either -- the four wrappers below
/// are what a caller names -- but all of the machinery is here, and they each
/// hold one.
pub struct MultiDragGestureRecognizer {
    policy: MultiDragPolicy,
    /// Upstream's `onStart`. Returning `None` means "not interested in this
    /// one", and the pointer is dropped rather than tracked for nothing.
    pub on_start: Option<Box<dyn FnMut(Offset) -> Option<Box<dyn Drag>>>>,
    /// Upstream's `_pointers` map, `None` once disposed.
    pointers: Option<Vec<(i64, MultiDragPointerState)>>,
    /// Upstream's `allowedButtonsFilter`, whose default here is upstream's
    /// `_defaultButtonAcceptBehavior`: the primary button and nothing else,
    /// not merely *including* it. A right-drag is not a drag.
    pub allowed_buttons: fn(i32) -> bool,
}

fn default_button_accept_behavior(buttons: i32) -> bool {
    buttons == crate::gestures::PRIMARY_BUTTON
}

impl MultiDragGestureRecognizer {
    pub fn new(policy: MultiDragPolicy) -> MultiDragGestureRecognizer {
        MultiDragGestureRecognizer {
            policy,
            on_start: None,
            pointers: Some(Vec::new()),
            allowed_buttons: default_button_accept_behavior,
        }
    }

    pub fn on_start(
        mut self,
        on_start: impl FnMut(Offset) -> Option<Box<dyn Drag>> + 'static,
    ) -> Self {
        self.on_start = Some(Box::new(on_start));
        self
    }

    pub fn policy(&self) -> MultiDragPolicy {
        self.policy
    }

    /// How many pointers are being tracked -- the number this family exists to
    /// let exceed one.
    pub fn tracked_pointers(&self) -> usize {
        self.pointers.as_ref().map_or(0, |pointers| pointers.len())
    }

    fn state(&mut self, pointer: i64) -> Option<&mut MultiDragPointerState> {
        self.pointers
            .as_mut()?
            .iter_mut()
            .find(|(id, _)| *id == pointer)
            .map(|(_, state)| state)
    }

    /// Upstream's `addAllowedPointer`, with the button filter folded in.
    ///
    /// Returns whether the pointer was taken up.
    pub fn add_pointer(&mut self, event: &PointerEvent) -> bool {
        if !(self.allowed_buttons)(event.buttons) {
            return false;
        }
        let policy = self.policy;
        let Some(pointers) = self.pointers.as_mut() else {
            return false;
        };
        if pointers.iter().any(|(id, _)| *id == event.pointer_id) {
            return false;
        }
        let mut state =
            MultiDragPointerState::new(event.position, event.kind, policy, event.time_stamp_micros);
        state.set_arena_entry();
        pointers.push((event.pointer_id, state));
        true
    }

    /// Upstream's `_handleEvent`, for a pointer this recogniser has taken up.
    pub fn handle_event(&mut self, event: &PointerEvent) {
        use crate::gestures::PointerChange;
        match event.change {
            PointerChange::Move => {
                if let Some(state) = self.state(event.pointer_id) {
                    state.handle_move(event);
                }
            }
            PointerChange::Up => {
                let now = event.time_stamp_micros;
                if let Some(state) = self.state(event.pointer_id) {
                    state.handle_up(now);
                }
                self.remove_state(event.pointer_id);
            }
            PointerChange::Cancel => {
                if let Some(state) = self.state(event.pointer_id) {
                    state.handle_cancel();
                }
                self.remove_state(event.pointer_id);
            }
            _ => {}
        }
    }

    /// Upstream's `acceptGesture`.
    ///
    /// A pointer that is no longer here is not a mistake: upstream's own
    /// comment says the drag may already have been cancelled if the up arrived
    /// before the acceptance.
    pub fn accept_gesture(&mut self, pointer: i64) {
        let Some(state) = self.state(pointer) else {
            return;
        };
        if state.accepted() {
            self.start_drag(pointer);
        }
    }

    /// Upstream's `_startDrag`: ask the caller for a client, and drop the
    /// pointer if it does not want one.
    fn start_drag(&mut self, pointer: i64) {
        let Some(state) = self.state(pointer) else {
            return;
        };
        let initial_position = state.initial_position;
        let drag = self
            .on_start
            .as_mut()
            .and_then(|on_start| on_start(initial_position));
        match drag {
            Some(drag) => {
                if let Some(state) = self.state(pointer) {
                    state.start_drag(drag);
                }
            }
            None => self.remove_state(pointer),
        }
    }

    /// Upstream's `rejectGesture`.
    pub fn reject_gesture(&mut self, pointer: i64) {
        if let Some(state) = self.state(pointer) {
            state.rejected();
        }
        self.remove_state(pointer);
    }

    /// Moves the delayed variant's clocks forward, and returns whether any
    /// pointer is still waiting on one.
    pub fn tick(&mut self, now_micros: i64) -> bool {
        let Some(pointers) = self.pointers.as_mut() else {
            return false;
        };
        let mut starting = Vec::new();
        for (id, state) in pointers.iter_mut() {
            if state.tick(now_micros) == Some(DelayOutcome::StartDrag) {
                starting.push(*id);
            }
        }
        for pointer in starting {
            self.start_drag(pointer);
        }
        self.pointers
            .as_ref()
            .is_some_and(|pointers| pointers.iter().any(|(_, state)| state.awaits_deadline()))
    }

    /// Takes the verdict one pointer's state has for the arena, if any.
    pub fn take_resolution(&mut self, pointer: i64) -> Option<Disposition> {
        self.state(pointer)
            .and_then(|state| state.take_resolution())
    }

    fn remove_state(&mut self, pointer: i64) {
        let Some(pointers) = self.pointers.as_mut() else {
            return;
        };
        if let Some(at) = pointers.iter().position(|(id, _)| *id == pointer) {
            pointers[at].1.dispose();
            pointers.remove(at);
        }
    }

    /// Upstream's `dispose`.
    pub fn dispose(&mut self) {
        if let Some(mut pointers) = self.pointers.take() {
            for (_, state) in pointers.iter_mut() {
                state.dispose();
            }
        }
    }
}

/// Upstream `ImmediateMultiDragGestureRecognizer`: movement in any direction,
/// as soon as it passes the slop.
///
/// The plain one. Against `PanGestureRecognizer` the only difference is that
/// several fingers may each be dragging something at the same time.
pub struct ImmediateMultiDragGestureRecognizer {
    pub base: MultiDragGestureRecognizer,
}

/// Upstream `HorizontalMultiDragGestureRecognizer`: only drags that *start*
/// horizontally.
pub struct HorizontalMultiDragGestureRecognizer {
    pub base: MultiDragGestureRecognizer,
}

/// Upstream `VerticalMultiDragGestureRecognizer`: only drags that *start*
/// vertically.
pub struct VerticalMultiDragGestureRecognizer {
    pub base: MultiDragGestureRecognizer,
}

/// Upstream `DelayedMultiDragGestureRecognizer`: only drags that start after
/// the finger has held still.
///
/// The one a reorderable list wants. Inside a scroll view an immediate drag
/// and a scroll are the same gesture and direction cannot separate them --
/// the item moves the way the list scrolls -- so time does it instead.
pub struct DelayedMultiDragGestureRecognizer {
    pub base: MultiDragGestureRecognizer,
}

macro_rules! multi_drag_recognizer {
    ($name:ident, $description:literal) => {
        impl Default for $name {
            fn default() -> $name {
                $name::new()
            }
        }

        impl $name {
            /// Upstream's `debugDescription`.
            pub fn debug_description(&self) -> &'static str {
                $description
            }
        }

        impl std::ops::Deref for $name {
            type Target = MultiDragGestureRecognizer;

            fn deref(&self) -> &MultiDragGestureRecognizer {
                &self.base
            }
        }

        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut MultiDragGestureRecognizer {
                &mut self.base
            }
        }
    };
}

impl ImmediateMultiDragGestureRecognizer {
    pub fn new() -> ImmediateMultiDragGestureRecognizer {
        ImmediateMultiDragGestureRecognizer {
            base: MultiDragGestureRecognizer::new(MultiDragPolicy::Immediate),
        }
    }
}

impl HorizontalMultiDragGestureRecognizer {
    pub fn new() -> HorizontalMultiDragGestureRecognizer {
        HorizontalMultiDragGestureRecognizer {
            base: MultiDragGestureRecognizer::new(MultiDragPolicy::Horizontal),
        }
    }
}

impl VerticalMultiDragGestureRecognizer {
    pub fn new() -> VerticalMultiDragGestureRecognizer {
        VerticalMultiDragGestureRecognizer {
            base: MultiDragGestureRecognizer::new(MultiDragPolicy::Vertical),
        }
    }
}

impl DelayedMultiDragGestureRecognizer {
    pub fn new() -> DelayedMultiDragGestureRecognizer {
        DelayedMultiDragGestureRecognizer::with_delay(DEFAULT_MULTI_DRAG_DELAY_MICROS)
    }

    /// Upstream's `delay` argument, which a caller moves when it wants a
    /// different hold.
    pub fn with_delay(delay_micros: i64) -> DelayedMultiDragGestureRecognizer {
        DelayedMultiDragGestureRecognizer {
            base: MultiDragGestureRecognizer::new(MultiDragPolicy::Delayed { delay_micros }),
        }
    }
}

multi_drag_recognizer!(ImmediateMultiDragGestureRecognizer, "multidrag");
multi_drag_recognizer!(HorizontalMultiDragGestureRecognizer, "horizontal multidrag");
multi_drag_recognizer!(VerticalMultiDragGestureRecognizer, "vertical multidrag");
multi_drag_recognizer!(DelayedMultiDragGestureRecognizer, "long multidrag");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gestures::{
        PRIMARY_BUTTON, PointerChange, SECONDARY_BUTTON, SignalKind, TOUCH_SLOP,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    /// What a drag client was told, in order.
    #[derive(Clone, Debug, PartialEq)]
    enum Told {
        Update(f32, f32),
        End,
        Cancel,
    }

    #[derive(Clone, Default)]
    struct Log(Rc<RefCell<Vec<(u32, Told)>>>);

    impl Log {
        fn entries(&self) -> Vec<(u32, Told)> {
            self.0.borrow().clone()
        }

        fn for_thing(&self, thing: u32) -> Vec<Told> {
            self.entries()
                .into_iter()
                .filter(|(id, _)| *id == thing)
                .map(|(_, told)| told)
                .collect()
        }
    }

    /// A client that writes down what it was told and which thing it is.
    struct Recorder {
        thing: u32,
        log: Log,
    }

    impl Drag for Recorder {
        fn update(&mut self, delta: Offset) {
            self.log
                .0
                .borrow_mut()
                .push((self.thing, Told::Update(delta.dx, delta.dy)));
        }

        fn end(&mut self, _velocity: Offset) {
            self.log.0.borrow_mut().push((self.thing, Told::End));
        }

        fn cancel(&mut self) {
            self.log.0.borrow_mut().push((self.thing, Told::Cancel));
        }
    }

    fn event(change: PointerChange, pointer: i64, position: Offset, micros: i64) -> PointerEvent {
        PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: pointer,
            change,
            kind: PointerKind::Touch,
            signal_kind: SignalKind::None,
            buttons: PRIMARY_BUTTON,
            time_stamp_micros: micros,
            position,
            delta: Offset::ZERO,
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: position,
        }
    }

    fn moved(pointer: i64, position: Offset, delta: Offset, micros: i64) -> PointerEvent {
        PointerEvent {
            delta,
            ..event(PointerChange::Move, pointer, position, micros)
        }
    }

    /// A recogniser that hands every accepted pointer its own client, named by
    /// the order it was asked in.
    fn recognizing(policy: MultiDragPolicy, log: &Log) -> MultiDragGestureRecognizer {
        let log = log.clone();
        let next = RefCell::new(0u32);
        MultiDragGestureRecognizer::new(policy).on_start(move |_position| {
            let thing = *next.borrow();
            *next.borrow_mut() += 1;
            Some(Box::new(Recorder {
                thing,
                log: log.clone(),
            }) as Box<dyn Drag>)
        })
    }

    #[test]
    fn two_fingers_drag_two_things_at_once() {
        // The whole reason this family exists. A `PanGestureRecognizer` would
        // give the second finger to the first drag or ignore it; here each
        // pointer gets judged, started and updated entirely on its own.
        let log = Log::default();
        let mut recognizer = recognizing(MultiDragPolicy::Immediate, &log);

        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::new(0.0, 0.0), 0));
        recognizer.add_pointer(&event(PointerChange::Down, 2, Offset::new(200.0, 0.0), 0));
        assert_eq!(recognizer.tracked_pointers(), 2);

        recognizer.accept_gesture(1);
        recognizer.accept_gesture(2);
        recognizer.handle_event(&moved(
            1,
            Offset::new(5.0, 0.0),
            Offset::new(5.0, 0.0),
            1_000,
        ));
        recognizer.handle_event(&moved(
            2,
            Offset::new(200.0, 9.0),
            Offset::new(0.0, 9.0),
            1_000,
        ));

        assert_eq!(
            log.for_thing(0),
            vec![Told::Update(0.0, 0.0), Told::Update(5.0, 0.0)]
        );
        assert_eq!(
            log.for_thing(1),
            vec![Told::Update(0.0, 0.0), Told::Update(0.0, 9.0)]
        );
    }

    #[test]
    fn the_movement_before_recognition_arrives_in_one_lump() {
        // A finger travels past the slop before anyone agrees it is dragging.
        // That travel is real, and the client's very first update is all of it
        // at once -- without it the thing would sit still and then jump by
        // that much on the next ordinary move.
        let log = Log::default();
        let mut recognizer = recognizing(MultiDragPolicy::Immediate, &log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));

        recognizer.handle_event(&moved(
            1,
            Offset::new(12.0, 0.0),
            Offset::new(12.0, 0.0),
            1_000,
        ));
        recognizer.handle_event(&moved(
            1,
            Offset::new(12.0, 14.0),
            Offset::new(0.0, 14.0),
            2_000,
        ));
        // Neither move passes the slop on its own -- 12 and 14 against 18 --
        // and together they do, which is the accumulation doing its work.
        assert_eq!(recognizer.take_resolution(1), Some(Disposition::Accepted));

        recognizer.accept_gesture(1);
        assert_eq!(log.for_thing(0), vec![Told::Update(12.0, 14.0)]);
    }

    #[test]
    fn a_movement_short_of_the_slop_decides_nothing() {
        let log = Log::default();
        let mut recognizer = recognizing(MultiDragPolicy::Immediate, &log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.handle_event(&moved(
            1,
            Offset::new(TOUCH_SLOP - 1.0, 0.0),
            Offset::new(TOUCH_SLOP - 1.0, 0.0),
            1_000,
        ));
        assert_eq!(recognizer.take_resolution(1), None);
    }

    #[test]
    fn the_axis_recognizers_only_count_movement_along_their_axis() {
        // Which is what makes a horizontal multidrag usable inside a vertical
        // list: a finger that is scrolling never accumulates enough sideways
        // travel to claim the gesture.
        let log = Log::default();
        let far = TOUCH_SLOP + 2.0;

        let mut horizontal = recognizing(MultiDragPolicy::Horizontal, &log);
        horizontal.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        horizontal.handle_event(&moved(
            1,
            Offset::new(0.0, far),
            Offset::new(0.0, far),
            1_000,
        ));
        assert_eq!(horizontal.take_resolution(1), None, "that was a scroll");
        horizontal.handle_event(&moved(
            1,
            Offset::new(far, far),
            Offset::new(far, 0.0),
            2_000,
        ));
        assert_eq!(horizontal.take_resolution(1), Some(Disposition::Accepted));

        let mut vertical = recognizing(MultiDragPolicy::Vertical, &log);
        vertical.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        vertical.handle_event(&moved(
            1,
            Offset::new(far, 0.0),
            Offset::new(far, 0.0),
            1_000,
        ));
        assert_eq!(vertical.take_resolution(1), None);
        vertical.handle_event(&moved(
            1,
            Offset::new(far, far),
            Offset::new(0.0, far),
            2_000,
        ));
        assert_eq!(vertical.take_resolution(1), Some(Disposition::Accepted));
    }

    #[test]
    fn moving_early_means_yes_to_three_of_them_and_no_to_the_fourth() {
        // The same measurement, read in opposite directions. For the immediate
        // recogniser travel past the slop is the evidence that this is a drag;
        // for the delayed one it is the evidence that it is a scroll, and the
        // recogniser gets out of the way.
        let log = Log::default();

        let mut immediate = recognizing(MultiDragPolicy::Immediate, &log);
        immediate.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        immediate.handle_event(&moved(
            1,
            Offset::new(30.0, 0.0),
            Offset::new(30.0, 0.0),
            1_000,
        ));
        assert_eq!(immediate.take_resolution(1), Some(Disposition::Accepted));

        let mut delayed = recognizing(
            MultiDragPolicy::Delayed {
                delay_micros: DEFAULT_MULTI_DRAG_DELAY_MICROS,
            },
            &log,
        );
        delayed.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        delayed.handle_event(&moved(
            1,
            Offset::new(30.0, 0.0),
            Offset::new(30.0, 0.0),
            1_000,
        ));
        assert_eq!(delayed.take_resolution(1), Some(Disposition::Rejected));
    }

    #[test]
    fn holding_still_long_enough_is_the_delayed_recognizers_whole_claim() {
        let log = Log::default();
        let mut delayed = recognizing(
            MultiDragPolicy::Delayed {
                delay_micros: DEFAULT_MULTI_DRAG_DELAY_MICROS,
            },
            &log,
        );
        delayed.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));

        assert!(delayed.tick(DEFAULT_MULTI_DRAG_DELAY_MICROS - 1));
        assert_eq!(delayed.take_resolution(1), None, "not yet");

        assert!(!delayed.tick(DEFAULT_MULTI_DRAG_DELAY_MICROS));
        assert_eq!(delayed.take_resolution(1), Some(Disposition::Accepted));
    }

    #[test]
    fn the_delay_and_the_acceptance_may_arrive_in_either_order() {
        // Nothing orders them: the arena can hand the gesture over while the
        // finger is still being held, or the hold can finish first. Whichever
        // is second is what actually starts the drag, and the client is told
        // exactly once either way.
        let delay = DEFAULT_MULTI_DRAG_DELAY_MICROS;

        // Acceptance first: the start is parked and the delay releases it.
        let accept_first = Log::default();
        let mut recognizer = recognizing(
            MultiDragPolicy::Delayed {
                delay_micros: delay,
            },
            &accept_first,
        );
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.accept_gesture(1);
        assert!(
            accept_first.entries().is_empty(),
            "the finger has not held still yet"
        );
        recognizer.tick(delay);
        assert_eq!(accept_first.for_thing(0), vec![Told::Update(0.0, 0.0)]);

        // Delay first: the acceptance starts it at once.
        let delay_first = Log::default();
        let mut recognizer = recognizing(
            MultiDragPolicy::Delayed {
                delay_micros: delay,
            },
            &delay_first,
        );
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.tick(delay);
        assert!(delay_first.entries().is_empty(), "nobody has accepted yet");
        recognizer.accept_gesture(1);
        assert_eq!(delay_first.for_thing(0), vec![Told::Update(0.0, 0.0)]);
    }

    #[test]
    fn a_caller_that_wants_no_client_gets_the_pointer_dropped() {
        // Upstream returns null from `onStart` to mean "not this one", and the
        // per-pointer state goes away rather than being tracked for a drag
        // that will never be reported anywhere.
        let mut recognizer =
            MultiDragGestureRecognizer::new(MultiDragPolicy::Immediate).on_start(|_position| None);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        assert_eq!(recognizer.tracked_pointers(), 1);
        recognizer.accept_gesture(1);
        assert_eq!(recognizer.tracked_pointers(), 0);
    }

    #[test]
    fn an_acceptance_that_arrives_after_the_finger_lifted_is_not_an_error() {
        // Upstream's own comment: the drag may already have been cancelled if
        // the up came before the accept. A recogniser that treated it as a
        // mistake would crash on an ordinary quick tap.
        let log = Log::default();
        let mut recognizer = recognizing(MultiDragPolicy::Immediate, &log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.handle_event(&event(PointerChange::Up, 1, Offset::ZERO, 1_000));
        assert_eq!(recognizer.tracked_pointers(), 0);

        recognizer.accept_gesture(1);
        assert!(log.entries().is_empty());
    }

    #[test]
    fn a_started_drag_is_ended_when_the_finger_lifts_and_cancelled_when_it_is_taken_away() {
        let lifted = Log::default();
        let mut recognizer = recognizing(MultiDragPolicy::Immediate, &lifted);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.accept_gesture(1);
        recognizer.handle_event(&event(PointerChange::Up, 1, Offset::ZERO, 1_000));
        assert_eq!(lifted.for_thing(0), vec![Told::Update(0.0, 0.0), Told::End]);

        let taken = Log::default();
        let mut recognizer = recognizing(MultiDragPolicy::Immediate, &taken);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.accept_gesture(1);
        recognizer.handle_event(&event(PointerChange::Cancel, 1, Offset::ZERO, 1_000));
        assert_eq!(
            taken.for_thing(0),
            vec![Told::Update(0.0, 0.0), Told::Cancel]
        );
    }

    #[test]
    fn a_rejected_pointer_is_forgotten_and_says_nothing_more() {
        let log = Log::default();
        let mut recognizer = recognizing(MultiDragPolicy::Immediate, &log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.reject_gesture(1);
        assert_eq!(recognizer.tracked_pointers(), 0);
        // And a movement for a pointer nobody is tracking is not a crash.
        recognizer.handle_event(&moved(
            1,
            Offset::new(40.0, 0.0),
            Offset::new(40.0, 0.0),
            1_000,
        ));
        assert!(log.entries().is_empty());
    }

    #[test]
    fn only_the_primary_button_drags_and_only_by_itself() {
        // Upstream's `_defaultButtonAcceptBehavior` is an equality, not a mask
        // test: the primary button *and nothing else*. A right-drag is not a
        // drag, and neither is a left-and-right-together drag.
        let log = Log::default();
        let mut recognizer = recognizing(MultiDragPolicy::Immediate, &log);
        let secondary = PointerEvent {
            buttons: SECONDARY_BUTTON,
            ..event(PointerChange::Down, 1, Offset::ZERO, 0)
        };
        assert!(!recognizer.add_pointer(&secondary));

        let both = PointerEvent {
            buttons: PRIMARY_BUTTON | SECONDARY_BUTTON,
            ..event(PointerChange::Down, 2, Offset::ZERO, 0)
        };
        assert!(!recognizer.add_pointer(&both));

        assert!(recognizer.add_pointer(&event(PointerChange::Down, 3, Offset::ZERO, 0)));
    }

    #[test]
    fn disposing_tells_the_arena_the_pointers_are_out() {
        let log = Log::default();
        let mut recognizer = recognizing(MultiDragPolicy::Immediate, &log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.dispose();
        // Disposed, so nothing is tracked and nothing more is taken up.
        assert_eq!(recognizer.tracked_pointers(), 0);
        assert!(!recognizer.add_pointer(&event(PointerChange::Down, 2, Offset::ZERO, 0)));
    }

    #[test]
    fn the_four_wrappers_carry_their_policy_and_upstreams_description() {
        assert_eq!(
            ImmediateMultiDragGestureRecognizer::new().policy(),
            MultiDragPolicy::Immediate
        );
        assert_eq!(
            ImmediateMultiDragGestureRecognizer::new().debug_description(),
            "multidrag"
        );
        assert_eq!(
            HorizontalMultiDragGestureRecognizer::new().policy(),
            MultiDragPolicy::Horizontal
        );
        assert_eq!(
            HorizontalMultiDragGestureRecognizer::new().debug_description(),
            "horizontal multidrag"
        );
        assert_eq!(
            VerticalMultiDragGestureRecognizer::new().policy(),
            MultiDragPolicy::Vertical
        );
        assert_eq!(
            VerticalMultiDragGestureRecognizer::new().debug_description(),
            "vertical multidrag"
        );
        assert_eq!(
            DelayedMultiDragGestureRecognizer::new().policy(),
            MultiDragPolicy::Delayed {
                delay_micros: DEFAULT_MULTI_DRAG_DELAY_MICROS
            },
            "the default hold is the long-press timeout"
        );
        assert_eq!(
            DelayedMultiDragGestureRecognizer::new().debug_description(),
            "long multidrag"
        );
        assert_eq!(
            DelayedMultiDragGestureRecognizer::with_delay(50_000).policy(),
            MultiDragPolicy::Delayed {
                delay_micros: 50_000
            }
        );
    }
}
