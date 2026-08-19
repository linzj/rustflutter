//! Taps counted two ways -- a port of the two recognisers left in upstream's
//! `gestures/multitap.dart`.
//!
//! Both count taps, and they disagree about what a second finger means, which
//! is the whole difference between them:
//!
//! * [`MultiTapGestureRecognizer`] treats **each pointer as its own tap**.
//!   Upstream's example is the clearest statement of it: down-1, down-2, up-1,
//!   up-2 produces *two* taps, one at up-1 and one at up-2. A piano keyboard
//!   wants this.
//! * [`SerialTapGestureRecognizer`] counts taps **in a series** -- first,
//!   second, third -- and a second finger arriving mid-series does not extend
//!   it, it ends it. A text field's word-then-paragraph selection wants this.
//!
//! ## What is not here
//!
//! As in [`crate::multidrag`], the crate's arena lives inside
//! [`GestureRouter`](crate::gestures::GestureRouter) and is keyed to the
//! router's own recogniser kinds, so these record the verdict they would give
//! and a caller drains it. Upstream's `Timer`s are deadlines walked by `tick`,
//! the same shape the router already uses for long press.

use crate::gesture_details::{SerialTapCancelDetails, SerialTapDownDetails, SerialTapUpDetails};
use crate::gestures::{
    DOUBLE_TAP_MIN_TIME_MICROS, DOUBLE_TAP_SLOP, DOUBLE_TAP_TIMEOUT_MICROS, Disposition,
    PointerChange, PointerEvent, PointerKind, TOUCH_SLOP, TapEvent, compute_hit_slop,
};
use crate::render::Offset;
use std::rc::Rc;

/// Upstream's `kDoubleTapTouchSlop`, which is defined as `kTouchSlop`.
///
/// A separate name for the same number, because the two are only equal by
/// present agreement: how far a finger may drift and still be the same tap is
/// not the same question as how far it must travel to be a drag.
pub const DOUBLE_TAP_TOUCH_SLOP: f32 = TOUCH_SLOP;

/// Upstream's `_TapTracker`: one pointer's tap, while it is still undecided.
///
/// Upstream's `_CountdownZoned` -- a timer that exists only to flip a bool --
/// is a deadline here, compared against the clock the caller passes in.
#[derive(Clone, Copy, Debug)]
pub struct TapTracker {
    pub pointer: i64,
    pub initial_global_position: Offset,
    pub initial_buttons: i32,
    pub kind: PointerKind,
    down_micros: i64,
    min_time_micros: i64,
}

impl TapTracker {
    pub fn new(event: &PointerEvent, min_time_micros: i64) -> TapTracker {
        TapTracker {
            pointer: event.pointer_id,
            initial_global_position: event.position,
            initial_buttons: event.buttons,
            kind: event.kind,
            down_micros: event.time_stamp_micros,
            min_time_micros,
        }
    }

    /// Upstream's `isWithinGlobalTolerance`.
    pub fn is_within_global_tolerance(&self, position: Offset, tolerance: f32) -> bool {
        position
            .plus(Offset::new(
                -self.initial_global_position.dx,
                -self.initial_global_position.dy,
            ))
            .distance()
            <= tolerance
    }

    /// Upstream's `hasElapsedMinTime`, and upstream's comment on why there is
    /// a *minimum* at all is worth keeping: touch screens often detect touches
    /// intermittently, so two downs closer together than this are one finger
    /// flickering rather than a reader tapping twice.
    pub fn has_elapsed_min_time(&self, now_micros: i64) -> bool {
        now_micros - self.down_micros >= self.min_time_micros
    }

    /// Upstream's `hasSameButton`.
    pub fn has_same_button(&self, buttons: i32) -> bool {
        buttons == self.initial_buttons
    }
}

// -- One tap per finger -------------------------------------------------------

/// Upstream's `_TapGesture`: what one pointer of a [`MultiTapGestureRecognizer`]
/// is doing.
struct TapGesture {
    tracker: TapTracker,
    /// Upstream's `_wonArena`.
    won_arena: bool,
    /// Where the finger is now. **Not** where it went down: the long-tap
    /// callback reports this one, so a finger that has drifted within the slop
    /// is reported where it actually is.
    last_position: Offset,
    /// Upstream's `_finalPosition`, set when the finger lifts.
    final_position: Option<Offset>,
    /// When the long tap fires, if it ever will. See
    /// [`MultiTapGestureRecognizer::long_tap_delay_micros`] for the surprise.
    long_tap_deadline: Option<i64>,
}

/// Upstream `MultiTapGestureRecognizer`: taps counted per pointer.
///
/// Upstream's own example says it best: down-1, down-2, up-1, up-2 produces
/// two taps, on up-1 and up-2. Nothing about one finger's tap depends on
/// another's.
pub struct MultiTapGestureRecognizer {
    /// Upstream's `longTapDelay`, defaulting to zero.
    ///
    /// **A zero delay means the long tap never fires, not that it fires at
    /// once.** Upstream's documentation says the opposite -- "defaults to
    /// [Duration.zero], which means [onLongTapDown] is called immediately
    /// after [onTapDown]" -- but its constructor guards the timer with
    /// `if (longTapDelay > Duration.zero)`, so with the default no timer is
    /// ever created and `_dispatchLongTap` is never reached. The code is
    /// ported as written and the disagreement is pinned by a regression line;
    /// of the two, the code is what every existing caller has been running
    /// against.
    pub long_tap_delay_micros: i64,
    pub on_tap_down: Option<Rc<dyn Fn(TapEvent)>>,
    pub on_tap_up: Option<Rc<dyn Fn(TapEvent)>>,
    pub on_tap: Option<Rc<dyn Fn(i64)>>,
    pub on_tap_cancel: Option<Rc<dyn Fn(i64)>>,
    pub on_long_tap_down: Option<Rc<dyn Fn(TapEvent)>>,
    gestures: Vec<(i64, TapGesture)>,
    resolutions: Vec<(i64, Disposition)>,
}

impl Default for MultiTapGestureRecognizer {
    fn default() -> MultiTapGestureRecognizer {
        MultiTapGestureRecognizer::new()
    }
}

impl MultiTapGestureRecognizer {
    pub fn new() -> MultiTapGestureRecognizer {
        MultiTapGestureRecognizer {
            long_tap_delay_micros: 0,
            on_tap_down: None,
            on_tap_up: None,
            on_tap: None,
            on_tap_cancel: None,
            on_long_tap_down: None,
            gestures: Vec::new(),
            resolutions: Vec::new(),
        }
    }

    pub fn with_long_tap_delay(mut self, delay_micros: i64) -> Self {
        self.long_tap_delay_micros = delay_micros;
        self
    }

    pub fn on_tap_down(mut self, handler: impl Fn(TapEvent) + 'static) -> Self {
        self.on_tap_down = Some(Rc::new(handler));
        self
    }

    pub fn on_tap_up(mut self, handler: impl Fn(TapEvent) + 'static) -> Self {
        self.on_tap_up = Some(Rc::new(handler));
        self
    }

    pub fn on_tap(mut self, handler: impl Fn(i64) + 'static) -> Self {
        self.on_tap = Some(Rc::new(handler));
        self
    }

    pub fn on_tap_cancel(mut self, handler: impl Fn(i64) + 'static) -> Self {
        self.on_tap_cancel = Some(Rc::new(handler));
        self
    }

    pub fn on_long_tap_down(mut self, handler: impl Fn(TapEvent) + 'static) -> Self {
        self.on_long_tap_down = Some(Rc::new(handler));
        self
    }

    /// Upstream's `debugDescription`.
    pub fn debug_description(&self) -> &'static str {
        "multitap"
    }

    pub fn tracked_pointers(&self) -> usize {
        self.gestures.len()
    }

    fn index_of(&self, pointer: i64) -> Option<usize> {
        self.gestures.iter().position(|(id, _)| *id == pointer)
    }

    /// Upstream's `addAllowedPointer`, which fires `onTapDown` **at once**
    /// rather than on winning the arena -- a key press should light up under
    /// the finger before anyone has decided whose gesture it is.
    pub fn add_pointer(&mut self, event: &PointerEvent) {
        if self.index_of(event.pointer_id).is_some() {
            return;
        }
        let gesture = TapGesture {
            tracker: TapTracker::new(event, DOUBLE_TAP_MIN_TIME_MICROS),
            won_arena: false,
            last_position: event.position,
            final_position: None,
            long_tap_deadline: (self.long_tap_delay_micros > 0)
                .then(|| event.time_stamp_micros + self.long_tap_delay_micros),
        };
        self.gestures.push((event.pointer_id, gesture));
        if let Some(on_tap_down) = &self.on_tap_down {
            on_tap_down(TapEvent {
                local_position: event.position,
                pointer_id: event.pointer_id,
            });
        }
    }

    /// Upstream's `_TapGesture.handleEvent`.
    pub fn handle_event(&mut self, event: &PointerEvent) {
        let Some(at) = self.index_of(event.pointer_id) else {
            return;
        };
        match event.change {
            PointerChange::Move => {
                let slop = compute_hit_slop(event.kind);
                let within = self.gestures[at]
                    .1
                    .tracker
                    .is_within_global_tolerance(event.position, slop);
                if within {
                    self.gestures[at].1.last_position = event.position;
                } else {
                    self.cancel(event.pointer_id);
                }
            }
            PointerChange::Cancel => self.cancel(event.pointer_id),
            PointerChange::Up => {
                self.gestures[at].1.final_position = Some(event.position);
                self.gestures[at].1.long_tap_deadline = None;
                self.check(event.pointer_id);
            }
            _ => {}
        }
    }

    /// Upstream's `acceptGesture`.
    pub fn accept_gesture(&mut self, pointer: i64) {
        let Some(at) = self.index_of(pointer) else {
            return;
        };
        self.gestures[at].1.won_arena = true;
        self.check(pointer);
    }

    /// Upstream's `rejectGesture`, which goes straight to the cancel callback.
    pub fn reject_gesture(&mut self, pointer: i64) {
        self.dispatch_cancel(pointer);
    }

    /// Upstream's `_TapGesture.cancel`.
    ///
    /// Having already won the arena, resolving it again would be a no-op, so
    /// the state is cleaned up directly; otherwise the arena is told, and the
    /// rejection comes back around as [`Self::reject_gesture`].
    fn cancel(&mut self, pointer: i64) {
        let Some(at) = self.index_of(pointer) else {
            return;
        };
        if self.gestures[at].1.won_arena {
            self.dispatch_cancel(pointer);
        } else {
            self.resolutions.push((pointer, Disposition::Rejected));
            self.dispatch_cancel(pointer);
        }
    }

    /// Upstream's `_check`: **both** conditions, in whichever order they
    /// arrive.
    ///
    /// Winning the arena is not enough -- the finger has to have lifted -- and
    /// lifting is not enough either, because until the arena has spoken the
    /// tap may still turn out to belong to a drag.
    fn check(&mut self, pointer: i64) {
        let Some(at) = self.index_of(pointer) else {
            return;
        };
        let (won, final_position) = {
            let gesture = &self.gestures[at].1;
            (gesture.won_arena, gesture.final_position)
        };
        if let (true, Some(position)) = (won, final_position) {
            self.dispatch_tap(pointer, position);
        }
    }

    fn dispatch_cancel(&mut self, pointer: i64) {
        let Some(at) = self.index_of(pointer) else {
            return;
        };
        self.gestures.remove(at);
        if let Some(on_tap_cancel) = &self.on_tap_cancel {
            on_tap_cancel(pointer);
        }
    }

    fn dispatch_tap(&mut self, pointer: i64, position: Offset) {
        let Some(at) = self.index_of(pointer) else {
            return;
        };
        self.gestures.remove(at);
        if let Some(on_tap_up) = &self.on_tap_up {
            on_tap_up(TapEvent {
                local_position: position,
                pointer_id: pointer,
            });
        }
        if let Some(on_tap) = &self.on_tap {
            on_tap(pointer);
        }
    }

    /// Upstream's long-tap `Timer`, walked by the clock.
    ///
    /// Returns whether any pointer is still waiting on one. Note that the long
    /// tap does **not** end the gesture: the finger is still down and the tap
    /// still counts when it lifts.
    pub fn tick(&mut self, now_micros: i64) -> bool {
        let due: Vec<(i64, Offset)> = self
            .gestures
            .iter_mut()
            .filter_map(|(id, gesture)| {
                let deadline = gesture.long_tap_deadline?;
                (now_micros >= deadline).then(|| {
                    gesture.long_tap_deadline = None;
                    (*id, gesture.last_position)
                })
            })
            .collect();
        for (pointer, position) in due {
            if let Some(on_long_tap_down) = &self.on_long_tap_down {
                on_long_tap_down(TapEvent {
                    local_position: position,
                    pointer_id: pointer,
                });
            }
        }
        self.gestures
            .iter()
            .any(|(_, gesture)| gesture.long_tap_deadline.is_some())
    }

    /// Takes the verdict one pointer has for the arena, if any.
    pub fn take_resolution(&mut self, pointer: i64) -> Option<Disposition> {
        let at = self.resolutions.iter().position(|(id, _)| *id == pointer)?;
        Some(self.resolutions.remove(at).1)
    }

    /// Upstream's `dispose`, which cancels every gesture still going.
    pub fn dispose(&mut self) {
        for pointer in self
            .gestures
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<i64>>()
        {
            self.cancel(pointer);
        }
    }
}

// -- Taps counted in a series -------------------------------------------------

/// Upstream `SerialTapGestureRecognizer`: first tap, second tap, third tap.
///
/// Where [`MultiTapGestureRecognizer`] gives each finger its own tap, this one
/// gives a *run* of taps a count, which is what a text field needs to tell a
/// word selection from a paragraph selection.
pub struct SerialTapGestureRecognizer {
    pub on_serial_tap_down: Option<Rc<dyn Fn(SerialTapDownDetails)>>,
    pub on_serial_tap_cancel: Option<Rc<dyn Fn(SerialTapCancelDetails)>>,
    pub on_serial_tap_up: Option<Rc<dyn Fn(SerialTapUpDetails)>>,
    completed: Vec<TapTracker>,
    pending: Option<TapTracker>,
    /// Upstream's `_gestureResolutions`: what the arena has already said about
    /// each pointer, so the recogniser does not tell it something twice.
    gesture_resolutions: Vec<(i64, Disposition)>,
    /// Upstream's `_serialTapTimer`: the series ends if nothing follows.
    series_deadline: Option<i64>,
    resolutions: Vec<(i64, Disposition)>,
}

impl Default for SerialTapGestureRecognizer {
    fn default() -> SerialTapGestureRecognizer {
        SerialTapGestureRecognizer::new()
    }
}

impl SerialTapGestureRecognizer {
    pub fn new() -> SerialTapGestureRecognizer {
        SerialTapGestureRecognizer {
            on_serial_tap_down: None,
            on_serial_tap_cancel: None,
            on_serial_tap_up: None,
            completed: Vec::new(),
            pending: None,
            gesture_resolutions: Vec::new(),
            series_deadline: None,
            resolutions: Vec::new(),
        }
    }

    pub fn on_serial_tap_down(mut self, handler: impl Fn(SerialTapDownDetails) + 'static) -> Self {
        self.on_serial_tap_down = Some(Rc::new(handler));
        self
    }

    pub fn on_serial_tap_cancel(
        mut self,
        handler: impl Fn(SerialTapCancelDetails) + 'static,
    ) -> Self {
        self.on_serial_tap_cancel = Some(Rc::new(handler));
        self
    }

    pub fn on_serial_tap_up(mut self, handler: impl Fn(SerialTapUpDetails) + 'static) -> Self {
        self.on_serial_tap_up = Some(Rc::new(handler));
        self
    }

    /// Upstream's `debugDescription`.
    pub fn debug_description(&self) -> &'static str {
        "serial tap"
    }

    /// Upstream's `isTrackingPointer`: `onSerialTapDown` has fired and neither
    /// of the other two has.
    pub fn is_tracking_pointer(&self) -> bool {
        self.pending.is_some()
    }

    /// How many taps of the current series have completed.
    pub fn completed_taps(&self) -> usize {
        self.completed.len()
    }

    /// Upstream's `isPointerAllowed`: **a recogniser with nothing to say does
    /// not compete.**
    ///
    /// Without this it would join every arena and could win one, taking the
    /// gesture away from a recogniser that would actually have done something
    /// with it.
    pub fn is_pointer_allowed(&self) -> bool {
        self.on_serial_tap_down.is_some()
            || self.on_serial_tap_cancel.is_some()
            || self.on_serial_tap_up.is_some()
    }

    /// Upstream's `_representsSameSeries`: three conditions, all required.
    fn represents_same_series(&self, tap: &TapTracker, event: &PointerEvent) -> bool {
        tap.has_elapsed_min_time(event.time_stamp_micros)
            && tap.has_same_button(event.buttons)
            && tap.is_within_global_tolerance(event.position, DOUBLE_TAP_SLOP)
    }

    /// Upstream's `addAllowedPointer`.
    ///
    /// A pointer arriving while another is still down ends the series rather
    /// than extending it: two fingers are not a double tap.
    pub fn add_pointer(&mut self, event: &PointerEvent) {
        if !self.is_pointer_allowed() {
            return;
        }
        let breaks_series = match self.completed.last() {
            Some(last) => !self.represents_same_series(last, event),
            None => false,
        };
        if breaks_series || self.pending.is_some() {
            self.reset();
        }
        self.track_tap(event);
    }

    fn track_tap(&mut self, event: &PointerEvent) {
        self.series_deadline = None;
        if let Some(on_down) = &self.on_serial_tap_down {
            on_down(
                SerialTapDownDetails::new(
                    event.position,
                    event.kind,
                    self.completed.len() as u32 + 1,
                )
                .with_buttons(event.buttons),
            );
        }
        self.pending = Some(TapTracker::new(event, DOUBLE_TAP_MIN_TIME_MICROS));
    }

    /// Upstream's `_handleEvent`.
    pub fn handle_event(&mut self, event: &PointerEvent) {
        let Some(pending) = self.pending else {
            return;
        };
        if pending.pointer != event.pointer_id {
            return;
        }
        match event.change {
            PointerChange::Up => self.register_tap(event, pending),
            PointerChange::Move => {
                if !pending.is_within_global_tolerance(event.position, DOUBLE_TAP_TOUCH_SLOP) {
                    self.reset();
                }
            }
            PointerChange::Cancel => self.reset(),
            _ => {}
        }
    }

    /// Upstream's `acceptGesture`, which only *records* the verdict: the tap
    /// is reported when the finger lifts, not when the arena speaks.
    pub fn accept_gesture(&mut self, pointer: i64) {
        self.record_resolution(pointer, Disposition::Accepted);
    }

    /// Upstream's `rejectGesture`, which ends the series.
    pub fn reject_gesture(&mut self, pointer: i64) {
        self.record_resolution(pointer, Disposition::Rejected);
        self.reset();
    }

    fn record_resolution(&mut self, pointer: i64, disposition: Disposition) {
        match self
            .gesture_resolutions
            .iter_mut()
            .find(|(id, _)| *id == pointer)
        {
            Some(slot) => slot.1 = disposition,
            None => self.gesture_resolutions.push((pointer, disposition)),
        }
    }

    fn resolved(&self, pointer: i64) -> bool {
        self.gesture_resolutions
            .iter()
            .any(|(id, _)| *id == pointer)
    }

    /// Upstream's `_registerTap`, whose statement order upstream flags as
    /// important and which is kept exactly.
    ///
    /// `_pendingTap` is cleared, *then* the up is reported, *then* the tap
    /// joins the completed list -- so the count in the up details is the count
    /// of this tap rather than of the next one.
    fn register_tap(&mut self, event: &PointerEvent, tracker: TapTracker) {
        self.start_series_timer(event.time_stamp_micros);
        if !self.resolved(event.pointer_id) {
            self.resolutions
                .push((event.pointer_id, Disposition::Accepted));
            self.record_resolution(event.pointer_id, Disposition::Accepted);
        }
        self.pending = None;
        self.check_up(event, &tracker);
        self.completed.push(tracker);
    }

    /// Upstream's `_rejectPendingTap`, whose ordering upstream also flags.
    ///
    /// The cancel is reported *before* the arena is told, because telling the
    /// arena can re-enter `reset` and then the completed count -- which is
    /// what the cancel reports -- would already have been cleared.
    fn reject_pending_tap(&mut self) {
        let Some(tracker) = self.pending.take() else {
            return;
        };
        self.check_cancel(self.completed.len() as u32 + 1);
        if !self.resolved(tracker.pointer) {
            self.resolutions
                .push((tracker.pointer, Disposition::Rejected));
        }
    }

    /// Upstream's `_reset`.
    pub fn reset(&mut self) {
        if self.pending.is_some() {
            self.reject_pending_tap();
        }
        self.pending = None;
        self.completed.clear();
        self.gesture_resolutions.clear();
        self.series_deadline = None;
    }

    fn start_series_timer(&mut self, now_micros: i64) {
        if self.series_deadline.is_none() {
            self.series_deadline = Some(now_micros + DOUBLE_TAP_TIMEOUT_MICROS);
        }
    }

    /// Walks the series timer. Returns whether one is still running.
    pub fn tick(&mut self, now_micros: i64) -> bool {
        if let Some(deadline) = self.series_deadline {
            if now_micros >= deadline {
                self.reset();
            }
        }
        self.series_deadline.is_some()
    }

    fn check_up(&self, event: &PointerEvent, tracker: &TapTracker) {
        if let Some(on_up) = &self.on_serial_tap_up {
            let mut details =
                SerialTapUpDetails::new(event.position, self.completed.len() as u32 + 1);
            details.kind = Some(tracker.kind);
            on_up(details);
        }
    }

    fn check_cancel(&self, count: u32) {
        if let Some(on_cancel) = &self.on_serial_tap_cancel {
            on_cancel(SerialTapCancelDetails::new(count));
        }
    }

    /// Takes the verdict one pointer has for the arena, if any.
    pub fn take_resolution(&mut self, pointer: i64) -> Option<Disposition> {
        let at = self.resolutions.iter().position(|(id, _)| *id == pointer)?;
        Some(self.resolutions.remove(at).1)
    }

    /// Upstream's `dispose`.
    pub fn dispose(&mut self) {
        self.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gestures::{PRIMARY_BUTTON, SECONDARY_BUTTON, SignalKind};
    use std::cell::RefCell;

    #[derive(Clone, Debug, PartialEq)]
    enum Said {
        Down(i64),
        LongDown(i64, f32),
        Up(i64),
        Tap(i64),
        Cancel(i64),
    }

    #[derive(Clone, Default)]
    struct Log(Rc<RefCell<Vec<Said>>>);

    impl Log {
        fn said(&self) -> Vec<Said> {
            self.0.borrow().clone()
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

    fn listening(log: &Log) -> MultiTapGestureRecognizer {
        let (down, long, up, tap, cancel) = (
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
        );
        MultiTapGestureRecognizer::new()
            .on_tap_down(move |e| down.0.borrow_mut().push(Said::Down(e.pointer_id)))
            .on_long_tap_down(move |e| {
                long.0
                    .borrow_mut()
                    .push(Said::LongDown(e.pointer_id, e.local_position.dx))
            })
            .on_tap_up(move |e| up.0.borrow_mut().push(Said::Up(e.pointer_id)))
            .on_tap(move |pointer| tap.0.borrow_mut().push(Said::Tap(pointer)))
            .on_tap_cancel(move |pointer| cancel.0.borrow_mut().push(Said::Cancel(pointer)))
    }

    #[test]
    fn two_fingers_down_then_both_up_makes_two_taps() {
        // Upstream's own example, and the whole reason this recogniser is not
        // TapGestureRecognizer: down-1, down-2, up-1, up-2 produces two taps,
        // one at up-1 and one at up-2. A piano keyboard needs exactly this.
        let log = Log::default();
        let mut recognizer = listening(&log);

        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.add_pointer(&event(
            PointerChange::Down,
            2,
            Offset::new(80.0, 0.0),
            1_000,
        ));
        recognizer.accept_gesture(1);
        recognizer.accept_gesture(2);
        recognizer.handle_event(&event(PointerChange::Up, 1, Offset::ZERO, 2_000));
        recognizer.handle_event(&event(PointerChange::Up, 2, Offset::new(80.0, 0.0), 3_000));

        assert_eq!(
            log.said(),
            vec![
                Said::Down(1),
                Said::Down(2),
                Said::Up(1),
                Said::Tap(1),
                Said::Up(2),
                Said::Tap(2),
            ]
        );
    }

    #[test]
    fn the_key_lights_up_before_anyone_has_decided_whose_gesture_it_is() {
        // onTapDown fires at pointer-down rather than on winning the arena.
        // Waiting for the arena would mean a key that lights up after the
        // finger has already left it.
        let log = Log::default();
        let mut recognizer = listening(&log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        assert_eq!(log.said(), vec![Said::Down(1)]);
    }

    #[test]
    fn a_tap_needs_both_the_arena_and_the_lift_in_whichever_order() {
        // Winning is not enough -- the finger is still down. Lifting is not
        // enough either, because until the arena speaks the gesture may still
        // turn out to have been a drag.
        let lift_first = Log::default();
        let mut recognizer = listening(&lift_first);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.handle_event(&event(PointerChange::Up, 1, Offset::ZERO, 1_000));
        assert_eq!(
            lift_first.said(),
            vec![Said::Down(1)],
            "nobody has accepted"
        );
        recognizer.accept_gesture(1);
        assert_eq!(
            lift_first.said(),
            vec![Said::Down(1), Said::Up(1), Said::Tap(1)]
        );

        let win_first = Log::default();
        let mut recognizer = listening(&win_first);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.accept_gesture(1);
        assert_eq!(win_first.said(), vec![Said::Down(1)], "still holding");
        recognizer.handle_event(&event(PointerChange::Up, 1, Offset::ZERO, 1_000));
        assert_eq!(
            win_first.said(),
            vec![Said::Down(1), Said::Up(1), Said::Tap(1)]
        );
    }

    #[test]
    fn a_finger_that_wanders_off_cancels_its_own_tap_and_nobody_elses() {
        let log = Log::default();
        let mut recognizer = listening(&log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.add_pointer(&event(PointerChange::Down, 2, Offset::new(80.0, 0.0), 0));

        // Within the slop: still a tap.
        recognizer.handle_event(&event(
            PointerChange::Move,
            1,
            Offset::new(TOUCH_SLOP - 1.0, 0.0),
            1_000,
        ));
        assert_eq!(recognizer.tracked_pointers(), 2);

        // Past it: gone.
        recognizer.handle_event(&event(
            PointerChange::Move,
            1,
            Offset::new(60.0, 0.0),
            2_000,
        ));
        assert_eq!(recognizer.take_resolution(1), Some(Disposition::Rejected));
        assert_eq!(recognizer.tracked_pointers(), 1);
        assert_eq!(
            log.said(),
            vec![Said::Down(1), Said::Down(2), Said::Cancel(1)]
        );

        // The other finger still taps.
        recognizer.accept_gesture(2);
        recognizer.handle_event(&event(PointerChange::Up, 2, Offset::new(80.0, 0.0), 3_000));
        assert!(log.said().contains(&Said::Tap(2)));
    }

    #[test]
    fn a_zero_long_tap_delay_means_never_rather_than_at_once() {
        // Upstream's documentation says a zero delay means onLongTapDown "is
        // called immediately after onTapDown". Its constructor says otherwise:
        // the timer is created only `if (longTapDelay > Duration.zero)`, so
        // with the default no timer exists and the callback is unreachable.
        // Ported as written -- the code is what every existing caller has been
        // running against -- and pinned here so the disagreement is not
        // mistaken for a porting slip.
        let log = Log::default();
        let mut recognizer = listening(&log);
        assert_eq!(recognizer.long_tap_delay_micros, 0);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        assert!(!recognizer.tick(0));
        assert!(!recognizer.tick(10_000_000));
        assert_eq!(log.said(), vec![Said::Down(1)], "no long tap, ever");
    }

    #[test]
    fn a_long_tap_reports_where_the_finger_is_now_and_does_not_end_the_tap() {
        // Two things at once. The position is the *last* one rather than the
        // initial one, so a finger that has drifted within the slop is
        // reported where it actually is; and the gesture carries on, so the
        // ordinary tap still happens when the finger lifts.
        let log = Log::default();
        let mut recognizer = listening(&log).with_long_tap_delay(500_000);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.handle_event(&event(
            PointerChange::Move,
            1,
            Offset::new(9.0, 0.0),
            100_000,
        ));

        assert!(recognizer.tick(400_000), "not yet");
        assert_eq!(log.said(), vec![Said::Down(1)]);
        assert!(!recognizer.tick(500_000));
        assert_eq!(log.said(), vec![Said::Down(1), Said::LongDown(1, 9.0)]);

        recognizer.accept_gesture(1);
        recognizer.handle_event(&event(PointerChange::Up, 1, Offset::new(9.0, 0.0), 600_000));
        assert_eq!(
            log.said(),
            vec![
                Said::Down(1),
                Said::LongDown(1, 9.0),
                Said::Up(1),
                Said::Tap(1)
            ]
        );
    }

    #[test]
    fn disposing_cancels_every_finger_still_down() {
        let log = Log::default();
        let mut recognizer = listening(&log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.add_pointer(&event(PointerChange::Down, 2, Offset::new(80.0, 0.0), 0));
        recognizer.dispose();
        assert_eq!(recognizer.tracked_pointers(), 0);
        assert!(log.said().contains(&Said::Cancel(1)));
        assert!(log.said().contains(&Said::Cancel(2)));
    }

    // -- Serial taps ----------------------------------------------------------

    #[derive(Clone, Default)]
    struct SerialLog {
        down: Rc<RefCell<Vec<u32>>>,
        up: Rc<RefCell<Vec<u32>>>,
        cancel: Rc<RefCell<Vec<u32>>>,
    }

    fn serial(log: &SerialLog) -> SerialTapGestureRecognizer {
        let (down, up, cancel) = (log.down.clone(), log.up.clone(), log.cancel.clone());
        SerialTapGestureRecognizer::new()
            .on_serial_tap_down(move |d| down.borrow_mut().push(d.count))
            .on_serial_tap_up(move |d| up.borrow_mut().push(d.count))
            .on_serial_tap_cancel(move |d| cancel.borrow_mut().push(d.count))
    }

    fn tap(recognizer: &mut SerialTapGestureRecognizer, position: Offset, at: i64) {
        recognizer.add_pointer(&event(PointerChange::Down, 1, position, at));
        recognizer.handle_event(&event(PointerChange::Up, 1, position, at + 50_000));
    }

    #[test]
    fn three_taps_in_a_row_are_counted_one_two_three() {
        let log = SerialLog::default();
        let mut recognizer = serial(&log);
        tap(&mut recognizer, Offset::ZERO, 0);
        tap(&mut recognizer, Offset::ZERO, 100_000);
        tap(&mut recognizer, Offset::ZERO, 200_000);
        assert_eq!(*log.down.borrow(), vec![1, 2, 3]);
        assert_eq!(*log.up.borrow(), vec![1, 2, 3]);
        assert!(log.cancel.borrow().is_empty());
    }

    #[test]
    fn a_recognizer_with_nothing_to_say_does_not_compete() {
        // Without this it would join every arena and could win one, taking the
        // gesture away from a recogniser that would have done something.
        let silent = SerialTapGestureRecognizer::new();
        assert!(!silent.is_pointer_allowed());
        let mut silent = silent;
        silent.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        assert!(!silent.is_tracking_pointer());

        let speaking = SerialTapGestureRecognizer::new().on_serial_tap_cancel(|_| {});
        assert!(
            speaking.is_pointer_allowed(),
            "any one of the three is enough"
        );
    }

    #[test]
    fn a_tap_somewhere_else_starts_a_new_series() {
        // Upstream's kDoubleTapSlop, which is generous but not unbounded: two
        // taps at opposite ends of a page are two first taps.
        let log = SerialLog::default();
        let mut recognizer = serial(&log);
        tap(&mut recognizer, Offset::ZERO, 0);
        tap(&mut recognizer, Offset::new(400.0, 0.0), 100_000);
        assert_eq!(*log.down.borrow(), vec![1, 1]);
        assert_eq!(*log.up.borrow(), vec![1, 1]);
    }

    #[test]
    fn a_tap_with_a_different_button_starts_a_new_series() {
        let log = SerialLog::default();
        let mut recognizer = serial(&log);
        tap(&mut recognizer, Offset::ZERO, 0);
        let secondary = PointerEvent {
            buttons: SECONDARY_BUTTON,
            ..event(PointerChange::Down, 1, Offset::ZERO, 100_000)
        };
        recognizer.add_pointer(&secondary);
        assert_eq!(
            *log.down.borrow(),
            vec![1, 1],
            "a right-click is not a second left-click"
        );
    }

    #[test]
    fn two_taps_too_close_together_in_time_are_one_finger_flickering() {
        // Upstream's comment on hasElapsedMinTime: touch screens often detect
        // touches intermittently, so a second down inside kDoubleTapMinTime is
        // hardware noise rather than a reader tapping twice.
        let log = SerialLog::default();
        let mut recognizer = serial(&log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.handle_event(&event(PointerChange::Up, 1, Offset::ZERO, 1_000));
        // Well inside the 40ms minimum.
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 5_000));
        assert_eq!(*log.down.borrow(), vec![1, 1]);
    }

    #[test]
    fn a_second_finger_ends_the_series_rather_than_extending_it() {
        // Two fingers are not a double tap.
        let log = SerialLog::default();
        let mut recognizer = serial(&log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        assert!(recognizer.is_tracking_pointer());
        recognizer.add_pointer(&event(PointerChange::Down, 2, Offset::ZERO, 10_000));
        assert_eq!(*log.cancel.borrow(), vec![1], "the first tap is taken back");
        assert_eq!(*log.down.borrow(), vec![1, 1], "and the second starts over");
    }

    #[test]
    fn the_series_ends_when_nothing_follows_it() {
        let log = SerialLog::default();
        let mut recognizer = serial(&log);
        tap(&mut recognizer, Offset::ZERO, 0);
        assert_eq!(recognizer.completed_taps(), 1);
        assert!(recognizer.tick(50_000 + DOUBLE_TAP_TIMEOUT_MICROS - 1));
        assert_eq!(recognizer.completed_taps(), 1);
        assert!(!recognizer.tick(50_000 + DOUBLE_TAP_TIMEOUT_MICROS));
        assert_eq!(recognizer.completed_taps(), 0);

        tap(&mut recognizer, Offset::ZERO, 1_000_000);
        assert_eq!(*log.down.borrow(), vec![1, 1], "the count started over");
    }

    #[test]
    fn a_cancelled_third_tap_is_reported_as_the_third() {
        // Upstream flags the statement order in _rejectPendingTap: the cancel
        // is reported before the arena is told, because telling the arena can
        // re-enter reset and clear the completed count the cancel reports.
        let log = SerialLog::default();
        let mut recognizer = serial(&log);
        tap(&mut recognizer, Offset::ZERO, 0);
        tap(&mut recognizer, Offset::ZERO, 100_000);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 200_000));
        assert_eq!(*log.down.borrow(), vec![1, 2, 3]);
        recognizer.handle_event(&event(PointerChange::Cancel, 1, Offset::ZERO, 210_000));
        assert_eq!(*log.cancel.borrow(), vec![3]);
        assert_eq!(*log.up.borrow(), vec![1, 2], "the third never came up");
    }

    #[test]
    fn a_finger_that_slides_too_far_ends_the_series() {
        let log = SerialLog::default();
        let mut recognizer = serial(&log);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 0));
        recognizer.handle_event(&event(
            PointerChange::Move,
            1,
            Offset::new(DOUBLE_TAP_TOUCH_SLOP - 1.0, 0.0),
            10_000,
        ));
        assert!(recognizer.is_tracking_pointer(), "still a tap");
        recognizer.handle_event(&event(
            PointerChange::Move,
            1,
            Offset::new(200.0, 0.0),
            20_000,
        ));
        assert!(!recognizer.is_tracking_pointer());
        assert_eq!(*log.cancel.borrow(), vec![1]);
    }

    #[test]
    fn a_rejected_pointer_ends_the_whole_series_and_not_just_its_own_tap() {
        let log = SerialLog::default();
        let mut recognizer = serial(&log);
        tap(&mut recognizer, Offset::ZERO, 0);
        recognizer.add_pointer(&event(PointerChange::Down, 1, Offset::ZERO, 100_000));
        assert_eq!(*log.down.borrow(), vec![1, 2]);
        recognizer.reject_gesture(1);
        assert_eq!(*log.cancel.borrow(), vec![2]);
        assert_eq!(recognizer.completed_taps(), 0, "the first tap went with it");
    }

    #[test]
    fn the_recognizers_carry_upstreams_descriptions() {
        assert_eq!(
            MultiTapGestureRecognizer::new().debug_description(),
            "multitap"
        );
        assert_eq!(
            SerialTapGestureRecognizer::new().debug_description(),
            "serial tap"
        );
    }
}
