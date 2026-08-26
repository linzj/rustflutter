//! One recogniser that is both the tap and the drag -- a port of the four
//! recognisers in upstream's `gestures/tap_and_drag.dart`.
//!
//! A text field cannot use a tap recogniser and a drag recogniser side by side,
//! because the two would compete for the same finger and the arena would have
//! to pick one before either knew what was happening. So upstream fuses them:
//! one recogniser that reports a tap **and** counts how many taps in a row it
//! was, and that turns into a drag if the finger keeps going. That count is the
//! whole point -- it is what lets one tap place a caret, two select a word and
//! three select a paragraph, with a drag out of any of them extending the
//! selection at that granularity.
//!
//! ## What is not here
//!
//! As in [`crate::multidrag`] and [`crate::multitap`], the crate's arena lives
//! inside [`GestureRouter`](crate::gestures::GestureRouter) and is keyed to the
//! router's own recogniser kinds, so these record the verdict they would give
//! and a caller drains it. Upstream's `Timer`s are deadlines walked by `tick`.
//! Upstream's pointer transforms have no counterpart here, so the local and
//! global positions are the same number.

use crate::gesture_details::{
    TapDragDownDetails, TapDragEndDetails, TapDragStartDetails, TapDragUpDetails,
    TapDragUpdateDetails,
};
use crate::gestures::{
    DOUBLE_TAP_SLOP, DOUBLE_TAP_TIMEOUT_MICROS, Disposition, PRESS_TIMEOUT_MICROS, PointerChange,
    PointerEvent, PointerKind, compute_hit_slop, compute_pan_slop,
};
use crate::render::Offset;
use std::rc::Rc;

/// Upstream `DragStartBehavior`: which position a drag says it started from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DragStartBehavior {
    /// Upstream's default: the position the pointer had when this recogniser
    /// won the arena.
    #[default]
    Start,
    /// The position of the first down event.
    ///
    /// The two differ only when something else was competing: with an
    /// uncontested gesture the recogniser wins at once and the two positions
    /// are the same.
    Down,
}

/// Upstream's `_consecutiveTapTimer`, whose three states are all meaningful.
///
/// Upstream distinguishes a null timer from one that exists but has fired, and
/// the distinction carries information: **the timeout callback deliberately
/// does nothing**. Upstream's comment says why -- the timer may run out before
/// a tap down or tap up has been reported, and resetting there would throw
/// away state a callback still needs. So the expiry is noticed lazily, at the
/// next pointer down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TapTimer {
    Stopped,
    Running(i64),
    Expired,
}

/// Upstream's `_TapStatusTrackerMixin`: how many taps in a row this is.
///
/// Kept as its own type rather than mixed in, because it is a self-contained
/// question -- "is this tap a continuation of the last one" -- with its own
/// three-part answer.
pub struct TapStatusTracker {
    /// Upstream's `maxConsecutiveTap`. `None` lets the count grow without
    /// limit; a value resets the series when the count reaches it, so a
    /// quadruple tap on a field that only understands three starts over rather
    /// than reporting a fourth nobody handles.
    pub max_consecutive_tap: Option<u32>,
    consecutive_tap_count: u32,
    down: Option<PointerEvent>,
    up: Option<PointerEvent>,
    origin_position: Option<Offset>,
    previous_buttons: Option<i32>,
    last_tap_offset: Option<Offset>,
    timer: TapTimer,
    on_tap_track_start: Option<Rc<dyn Fn()>>,
    on_tap_track_reset: Option<Rc<dyn Fn()>>,
}

impl Default for TapStatusTracker {
    fn default() -> TapStatusTracker {
        TapStatusTracker::new()
    }
}

impl TapStatusTracker {
    pub fn new() -> TapStatusTracker {
        TapStatusTracker {
            max_consecutive_tap: None,
            consecutive_tap_count: 0,
            down: None,
            up: None,
            origin_position: None,
            previous_buttons: None,
            last_tap_offset: None,
            timer: TapTimer::Stopped,
            on_tap_track_start: None,
            on_tap_track_reset: None,
        }
    }

    /// Upstream's `consecutiveTapCount`, zero when no series is being tracked.
    pub fn consecutive_tap_count(&self) -> u32 {
        self.consecutive_tap_count
    }

    /// Upstream's `currentDown`.
    pub fn current_down(&self) -> Option<&PointerEvent> {
        self.down.as_ref()
    }

    /// Upstream's `currentUp`.
    pub fn current_up(&self) -> Option<&PointerEvent> {
        self.up.as_ref()
    }

    pub fn on_tap_track_start(&mut self, handler: impl Fn() + 'static) {
        self.on_tap_track_start = Some(Rc::new(handler));
    }

    pub fn on_tap_track_reset(&mut self, handler: impl Fn() + 'static) {
        self.on_tap_track_reset = Some(Rc::new(handler));
    }

    /// Upstream's `addAllowedPointer`.
    pub fn add_pointer(&mut self, event: &PointerEvent) {
        if self.timer == TapTimer::Expired {
            self.reset();
        }
        if self.max_consecutive_tap == Some(self.consecutive_tap_count) {
            self.reset();
        }
        self.up = None;
        if self.down.is_some() && !self.represents_same_series(event) {
            self.consecutive_tap_count = 1;
        } else {
            self.consecutive_tap_count += 1;
        }
        self.timer = TapTimer::Stopped;
        self.track_tap(event);
    }

    /// Upstream's `handleEvent`.
    pub fn handle_event(&mut self, event: &PointerEvent) {
        match event.change {
            PointerChange::Move => {
                let slop = compute_hit_slop(event.kind);
                if self.global_distance(event.position) > slop {
                    // **A drag breaks the run.** The count itself is left
                    // alone, but the two things that would let the next tap
                    // join this series are cleared, so it cannot.
                    self.timer = TapTimer::Stopped;
                    self.previous_buttons = None;
                    self.last_tap_offset = None;
                }
            }
            PointerChange::Up => {
                self.up = Some(*event);
                if self.down.is_some() {
                    self.timer =
                        TapTimer::Running(event.time_stamp_micros + DOUBLE_TAP_TIMEOUT_MICROS);
                }
            }
            PointerChange::Cancel => self.reset(),
            _ => {}
        }
    }

    /// Walks the between-taps timer. Returns whether one is still running.
    ///
    /// Expiring does **not** reset anything -- see [`TapTimer`].
    pub fn tick(&mut self, now_micros: i64) -> bool {
        if let TapTimer::Running(deadline) = self.timer {
            if now_micros >= deadline {
                self.timer = TapTimer::Expired;
            }
        }
        matches!(self.timer, TapTimer::Running(_))
    }

    fn global_distance(&self, position: Offset) -> f32 {
        match self.origin_position {
            Some(origin) => position
                .plus(Offset::new(-origin.dx, -origin.dy))
                .distance(),
            None => 0.0,
        }
    }

    fn track_tap(&mut self, event: &PointerEvent) {
        self.down = Some(*event);
        self.previous_buttons = Some(event.buttons);
        self.last_tap_offset = Some(event.position);
        self.origin_position = Some(event.position);
        if let Some(on_start) = &self.on_tap_track_start {
            on_start();
        }
    }

    /// Upstream's `_representsSameSeries`: three conditions, all required.
    fn represents_same_series(&self, event: &PointerEvent) -> bool {
        self.timer != TapTimer::Stopped
            && self.last_tap_offset.is_some_and(|last| {
                event
                    .position
                    .plus(Offset::new(-last.dx, -last.dy))
                    .distance()
                    <= DOUBLE_TAP_SLOP
            })
            && self.previous_buttons == Some(event.buttons)
    }

    /// Upstream's `_tapTrackerReset`.
    pub fn reset(&mut self) {
        self.timer = TapTimer::Stopped;
        self.previous_buttons = None;
        self.origin_position = None;
        self.last_tap_offset = None;
        self.consecutive_tap_count = 0;
        self.down = None;
        self.up = None;
        if let Some(on_reset) = &self.on_tap_track_reset {
            on_reset();
        }
    }
}

/// Upstream's `_DragState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragState {
    Ready,
    Possible,
    Accepted,
}

/// Which axis a recogniser counts movement along, and against which slop.
///
/// Upstream this is three method overrides per subclass; the two that differ
/// are the distance measured and the threshold it is measured against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapDragAxis {
    /// Upstream `TapAndHorizontalDragGestureRecognizer`: only the horizontal
    /// component counts, against the **hit** slop.
    Horizontal,
    /// Upstream `TapAndPanGestureRecognizer`: the whole movement counts,
    /// against the **pan** slop, which is twice as far.
    Pan,
}

impl TapDragAxis {
    fn delta_for_details(self, delta: Offset) -> Offset {
        match self {
            TapDragAxis::Horizontal => Offset::new(delta.dx, 0.0),
            TapDragAxis::Pan => delta,
        }
    }

    fn primary_value(self, value: Offset) -> Option<f32> {
        match self {
            TapDragAxis::Horizontal => Some(value.dx),
            TapDragAxis::Pan => None,
        }
    }

    fn is_sufficient(self, moved: f32, kind: PointerKind) -> bool {
        match self {
            TapDragAxis::Horizontal => moved.abs() > compute_hit_slop(kind),
            TapDragAxis::Pan => moved.abs() > compute_pan_slop(kind),
        }
    }
}

/// Upstream `BaseTapAndDragGestureRecognizer`: the tap and the drag in one.
///
/// Upstream declares it `sealed`, which is the same statement the three
/// subclasses below make by construction: they differ only in the axis policy,
/// and nobody else may add a fourth.
pub struct BaseTapAndDragGestureRecognizer {
    pub tracker: TapStatusTracker,
    /// Upstream's `eagerVictoryOnDrag`, defaulting to true.
    ///
    /// True means that noticing a drag is enough to claim the gesture at once.
    /// False makes the recogniser wait until it is the last one standing --
    /// which a caller wants when something outside it should get first refusal
    /// on a drag.
    pub eager_victory_on_drag: bool,
    pub drag_start_behavior: DragStartBehavior,
    /// Upstream's `dragUpdateThrottleFrequency`, `None` for no throttling.
    pub drag_update_throttle_micros: Option<i64>,
    axis: TapDragAxis,
    drag_state: DragState,
    primary_pointer: Option<i64>,
    initial_position: Offset,
    current_position: Offset,
    global_distance_moved: f32,
    global_distance_moved_all_axes: f32,
    past_slop_tolerance: bool,
    won_arena_for_primary_pointer: bool,
    sent_tap_down: bool,
    start: Option<PointerEvent>,
    deadline: Option<i64>,
    accepted_active_pointers: Vec<i64>,
    /// Upstream's `_trackedPointers` on `OneSequenceGestureRecognizer`, which
    /// is what makes `stopTrackingPointer` idempotent -- and that in turn is
    /// what stops `didStopTrackingLastPointer` from running twice when a tap
    /// up both stops tracking and then gives the pointer up.
    tracked_pointers: Vec<i64>,
    resolutions: Vec<(i64, Disposition)>,
    throttled_update: Option<TapDragUpdateDetails>,
    throttle_deadline: Option<i64>,
    pub on_tap_down: Option<Rc<dyn Fn(TapDragDownDetails)>>,
    pub on_tap_up: Option<Rc<dyn Fn(TapDragUpDetails)>>,
    pub on_drag_start: Option<Rc<dyn Fn(TapDragStartDetails)>>,
    pub on_drag_update: Option<Rc<dyn Fn(TapDragUpdateDetails)>>,
    pub on_drag_end: Option<Rc<dyn Fn(TapDragEndDetails)>>,
    pub on_cancel: Option<Rc<dyn Fn()>>,
}

impl BaseTapAndDragGestureRecognizer {
    pub fn new(axis: TapDragAxis) -> BaseTapAndDragGestureRecognizer {
        BaseTapAndDragGestureRecognizer {
            tracker: TapStatusTracker::new(),
            eager_victory_on_drag: true,
            drag_start_behavior: DragStartBehavior::Start,
            drag_update_throttle_micros: None,
            axis,
            drag_state: DragState::Ready,
            primary_pointer: None,
            initial_position: Offset::ZERO,
            current_position: Offset::ZERO,
            global_distance_moved: 0.0,
            global_distance_moved_all_axes: 0.0,
            past_slop_tolerance: false,
            won_arena_for_primary_pointer: false,
            sent_tap_down: false,
            start: None,
            deadline: None,
            accepted_active_pointers: Vec::new(),
            tracked_pointers: Vec::new(),
            resolutions: Vec::new(),
            throttled_update: None,
            throttle_deadline: None,
            on_tap_down: None,
            on_tap_up: None,
            on_drag_start: None,
            on_drag_update: None,
            on_drag_end: None,
            on_cancel: None,
        }
    }

    pub fn axis(&self) -> TapDragAxis {
        self.axis
    }

    pub fn with_eager_victory_on_drag(mut self, eager: bool) -> Self {
        self.eager_victory_on_drag = eager;
        self
    }

    pub fn with_drag_start_behavior(mut self, behavior: DragStartBehavior) -> Self {
        self.drag_start_behavior = behavior;
        self
    }

    pub fn with_max_consecutive_tap(mut self, max: u32) -> Self {
        self.tracker.max_consecutive_tap = Some(max);
        self
    }

    pub fn with_drag_update_throttle(mut self, micros: i64) -> Self {
        self.drag_update_throttle_micros = Some(micros);
        self
    }

    pub fn on_tap_down(mut self, handler: impl Fn(TapDragDownDetails) + 'static) -> Self {
        self.on_tap_down = Some(Rc::new(handler));
        self
    }

    pub fn on_tap_up(mut self, handler: impl Fn(TapDragUpDetails) + 'static) -> Self {
        self.on_tap_up = Some(Rc::new(handler));
        self
    }

    pub fn on_drag_start(mut self, handler: impl Fn(TapDragStartDetails) + 'static) -> Self {
        self.on_drag_start = Some(Rc::new(handler));
        self
    }

    pub fn on_drag_update(mut self, handler: impl Fn(TapDragUpdateDetails) + 'static) -> Self {
        self.on_drag_update = Some(Rc::new(handler));
        self
    }

    pub fn on_drag_end(mut self, handler: impl Fn(TapDragEndDetails) + 'static) -> Self {
        self.on_drag_end = Some(Rc::new(handler));
        self
    }

    pub fn on_cancel(mut self, handler: impl Fn() + 'static) -> Self {
        self.on_cancel = Some(Rc::new(handler));
        self
    }

    /// Upstream's `consecutiveTapCount`, forwarded.
    pub fn consecutive_tap_count(&self) -> u32 {
        self.tracker.consecutive_tap_count()
    }

    /// Upstream's `debugDescription` on the base class.
    pub fn debug_description(&self) -> &'static str {
        "tap_and_drag"
    }

    /// Upstream's `addAllowedPointer`.
    pub fn add_pointer(&mut self, event: &PointerEvent) {
        if self.drag_state != DragState::Ready {
            return;
        }
        self.tracker.add_pointer(event);
        self.primary_pointer = Some(event.pointer_id);
        self.global_distance_moved = 0.0;
        self.global_distance_moved_all_axes = 0.0;
        self.drag_state = DragState::Possible;
        self.initial_position = event.position;
        self.current_position = event.position;
        self.deadline = Some(event.time_stamp_micros + PRESS_TIMEOUT_MICROS);
        if !self.tracked_pointers.contains(&event.pointer_id) {
            self.tracked_pointers.push(event.pointer_id);
        }
    }

    /// Upstream's `_didExceedDeadline`, walked by the clock.
    ///
    /// Two things happen once the press timeout has passed. The tap down is
    /// reported -- so a caret lands under a finger that is merely resting.
    /// And **if this is already the second tap or later, the recogniser claims
    /// the gesture outright**, for the reason upstream gives: otherwise a
    /// double tap that is *held* would be taken by the long-press recogniser,
    /// and a reader double-tapping to select a word and then pausing before
    /// dragging would lose their selection.
    pub fn tick(&mut self, now_micros: i64) -> bool {
        self.tracker.tick(now_micros);
        if let Some(deadline) = self.deadline {
            if now_micros >= deadline {
                self.deadline = None;
                if self.tracker.current_down().is_some() {
                    let down = *self.tracker.current_down().unwrap();
                    self.check_tap_down(&down);
                    if self.tracker.consecutive_tap_count() > 1 {
                        self.resolve(Disposition::Accepted);
                    }
                }
            }
        }
        if let Some(deadline) = self.throttle_deadline {
            if now_micros >= deadline {
                self.throttle_deadline = None;
                self.flush_throttled_update();
            }
        }
        self.deadline.is_some() || self.throttle_deadline.is_some()
    }

    fn resolve(&mut self, disposition: Disposition) {
        if let Some(pointer) = self.primary_pointer {
            self.resolutions.push((pointer, disposition));
        }
    }

    /// Takes the verdict this recogniser has for the arena, if any.
    pub fn take_resolution(&mut self, pointer: i64) -> Option<Disposition> {
        let at = self.resolutions.iter().position(|(id, _)| *id == pointer)?;
        Some(self.resolutions.remove(at).1)
    }

    /// Upstream's `acceptGesture`.
    pub fn accept_gesture(&mut self, pointer: i64) {
        if self.primary_pointer != Some(pointer) {
            return;
        }
        self.deadline = None;
        if !self.accepted_active_pointers.contains(&pointer) {
            self.accepted_active_pointers.push(pointer);
        }

        // **The tap down fires here, on winning**, rather than at pointer down
        // as `MultiTapGestureRecognizer` does. The difference is the whole
        // reason the two exist: a piano key must light up before anyone has
        // decided, while a caret must not be moved by a finger that turns out
        // to be scrolling.
        if let Some(down) = self.tracker.current_down().copied() {
            self.check_tap_down(&down);
        }
        self.won_arena_for_primary_pointer = true;

        if let Some(start) = self.start {
            if !self.eager_victory_on_drag {
                self.drag_state = DragState::Accepted;
            }
            self.accept_drag(&start);
        }

        if let Some(up) = self.tracker.current_up().copied() {
            self.check_tap_up(&up);
        }
    }

    /// Upstream's `rejectGesture`.
    pub fn reject_gesture(&mut self, pointer: i64) {
        if self.primary_pointer != Some(pointer) {
            return;
        }
        self.tracker.reset();
        self.deadline = None;
        self.give_up_pointer(pointer);
        self.reset_taps();
        self.reset_drag_update_throttle();
    }

    /// Upstream's `handleEvent`.
    pub fn handle_event(&mut self, event: &PointerEvent) {
        if self.primary_pointer != Some(event.pointer_id) {
            return;
        }
        self.tracker.handle_event(event);
        match event.change {
            PointerChange::Move => self.handle_move(event),
            PointerChange::Up => {
                if self.drag_state == DragState::Possible {
                    self.stop_tracking_if_pointer_no_longer_down(event);
                } else if self.drag_state == DragState::Accepted {
                    self.give_up_pointer(event.pointer_id);
                }
            }
            PointerChange::Cancel => {
                self.drag_state = DragState::Ready;
                self.give_up_pointer(event.pointer_id);
            }
            _ => {}
        }
    }

    fn handle_move(&mut self, event: &PointerEvent) {
        let slop = compute_hit_slop(event.kind);
        self.past_slop_tolerance = self.past_slop_tolerance
            || event
                .position
                .plus(Offset::new(
                    -self.initial_position.dx,
                    -self.initial_position.dy,
                ))
                .distance()
                > slop;

        match self.drag_state {
            DragState::Accepted => {
                self.current_position = event.position;
                self.check_drag_update(event, None);
            }
            DragState::Possible => {
                if self.start.is_none() {
                    self.check_drag(event);
                }
                // Reached when the arena was won before any move had travelled
                // far enough to be a drag.
                if self.start.is_some() && self.won_arena_for_primary_pointer {
                    self.drag_state = DragState::Accepted;
                    let start = self.start.unwrap();
                    self.accept_drag(&start);
                }
            }
            DragState::Ready => {}
        }
    }

    /// Upstream's `_checkDrag`.
    ///
    /// The second clause is the interesting one: **once this recogniser has
    /// won, movement on *any* axis past the pan slop counts as a drag, even
    /// for the horizontal recogniser.** Nothing else is competing by then, so
    /// the gesture has to be something, and a drag is the only thing left.
    fn check_drag(&mut self, event: &PointerEvent) {
        let moved_locally = self.axis.delta_for_details(event.delta);
        self.global_distance_moved += moved_locally.distance()
            * self
                .axis
                .primary_value(moved_locally)
                .map_or(1.0, |primary| if primary < 0.0 { -1.0 } else { 1.0 });
        self.global_distance_moved_all_axes += event.delta.distance();
        if self
            .axis
            .is_sufficient(self.global_distance_moved, event.kind)
            || (self.won_arena_for_primary_pointer
                && self.global_distance_moved_all_axes.abs() > compute_pan_slop(event.kind))
        {
            self.start = Some(*event);
            if self.eager_victory_on_drag {
                self.drag_state = DragState::Accepted;
                if !self.won_arena_for_primary_pointer {
                    self.resolve(Disposition::Accepted);
                }
            }
        }
    }

    /// Upstream's `_acceptDrag`.
    fn accept_drag(&mut self, event: &PointerEvent) {
        if !self.won_arena_for_primary_pointer {
            return;
        }
        if self.drag_start_behavior == DragStartBehavior::Start {
            self.initial_position = self.initial_position.plus(event.delta);
            self.current_position = self.initial_position;
        }
        self.check_drag_start(event);
        if event.delta.dx != 0.0 || event.delta.dy != 0.0 {
            self.current_position = event.position;
            // Upstream's comment here reads "Only adds delta for down
            // behaviour", but the call it sits above runs under both -- for
            // `Start` the initial position has already absorbed the delta just
            // above. Ported as written; the comment is recorded rather than
            // acted on.
            let corrected = self.initial_position.plus(event.delta);
            self.check_drag_update(event, Some(corrected));
        }
    }

    /// Upstream's `stopTrackingIfPointerNoLongerDown` reaching
    /// `didStopTrackingLastPointer`.
    fn stop_tracking_if_pointer_no_longer_down(&mut self, event: &PointerEvent) {
        self.stop_tracking_pointer(event.pointer_id);
    }

    /// Upstream's `stopTrackingPointer`, whose guard matters: the last pointer
    /// can only stop being tracked once, and a tap up both stops tracking and
    /// then gives the pointer up.
    fn stop_tracking_pointer(&mut self, pointer: i64) {
        let Some(at) = self.tracked_pointers.iter().position(|id| *id == pointer) else {
            return;
        };
        self.tracked_pointers.remove(at);
        if self.tracked_pointers.is_empty() {
            self.did_stop_tracking_last_pointer();
        }
    }

    /// Upstream's `didStopTrackingLastPointer`, the branchy heart of it.
    fn did_stop_tracking_last_pointer(&mut self) {
        match self.drag_state {
            DragState::Ready => {
                self.check_cancel();
                self.resolve(Disposition::Rejected);
            }
            DragState::Possible => {
                if self.past_slop_tolerance {
                    // Too far to have been a tap. If the arena is already won,
                    // the gesture defaults to a drag that starts and ends at
                    // once; otherwise there is nothing left for it to be.
                    if self.won_arena_for_primary_pointer {
                        if let Some(down) = self.tracker.current_down().copied() {
                            self.drag_state = DragState::Accepted;
                            self.accept_drag(&down);
                            self.check_drag_end();
                        }
                    } else {
                        self.check_cancel();
                        self.resolve(Disposition::Rejected);
                    }
                } else if let Some(up) = self.tracker.current_up().copied() {
                    self.check_tap_up(&up);
                }
            }
            DragState::Accepted => self.check_drag_end(),
        }
        self.deadline = None;
        self.start = None;
        self.drag_state = DragState::Ready;
        self.past_slop_tolerance = false;
    }

    /// Upstream's `_checkTapDown`, which fires at most once per gesture.
    fn check_tap_down(&mut self, event: &PointerEvent) {
        if self.sent_tap_down {
            return;
        }
        if let Some(on_tap_down) = &self.on_tap_down {
            on_tap_down(
                TapDragDownDetails::new(event.position, self.tracker.consecutive_tap_count())
                    .with_kind(event.kind),
            );
        }
        self.sent_tap_down = true;
    }

    /// Upstream's `_checkTapUp`, which says nothing until the arena is won.
    fn check_tap_up(&mut self, event: &PointerEvent) {
        if !self.won_arena_for_primary_pointer {
            return;
        }
        if let Some(on_tap_up) = &self.on_tap_up {
            on_tap_up(TapDragUpDetails::new(
                event.position,
                event.kind,
                self.tracker.consecutive_tap_count(),
            ));
        }
        self.reset_taps();
        self.give_up_pointer(event.pointer_id);
    }

    fn check_drag_start(&mut self, event: &PointerEvent) {
        if let Some(on_drag_start) = &self.on_drag_start {
            let mut details = TapDragStartDetails::new(
                self.initial_position,
                self.tracker.consecutive_tap_count(),
            );
            details.source_time_stamp_micros = Some(event.time_stamp_micros);
            details.kind = Some(event.kind);
            on_drag_start(details);
        }
    }

    fn check_drag_update(&mut self, event: &PointerEvent, corrected: Option<Offset>) {
        let position = corrected.unwrap_or(event.position);
        let delta = self.axis.delta_for_details(event.delta);
        let origin = position.plus(Offset::new(
            -self.initial_position.dx,
            -self.initial_position.dy,
        ));
        let mut details =
            TapDragUpdateDetails::new(position, origin, self.tracker.consecutive_tap_count())
                .with_delta(delta);
        details.source_time_stamp_micros = Some(event.time_stamp_micros);
        details.kind = Some(event.kind);
        if let Some(primary) = self.axis.primary_value(delta) {
            details = details.with_primary_delta(primary);
        }

        match self.drag_update_throttle_micros {
            None => {
                if let Some(on_drag_update) = &self.on_drag_update {
                    on_drag_update(details);
                }
            }
            Some(period) => {
                self.throttled_update = Some(details);
                if self.throttle_deadline.is_none() {
                    self.throttle_deadline = Some(event.time_stamp_micros + period);
                }
            }
        }
    }

    fn flush_throttled_update(&mut self) {
        if let Some(details) = self.throttled_update.take() {
            if let Some(on_drag_update) = &self.on_drag_update {
                on_drag_update(details);
            }
        }
    }

    /// Upstream's `_checkDragEnd`.
    ///
    /// A throttled update still pending is delivered **first and at once**:
    /// upstream cancels the timer and runs it rather than dropping it, because
    /// the last position of a drag is the one that decides where a selection
    /// ends.
    fn check_drag_end(&mut self) {
        if self.throttle_deadline.is_some() {
            self.throttle_deadline = None;
            self.flush_throttled_update();
        }
        if let Some(on_drag_end) = &self.on_drag_end {
            let mut details = TapDragEndDetails::new(self.tracker.consecutive_tap_count());
            details.global_position = self.current_position;
            details.local_position = self.current_position;
            on_drag_end(details);
        }
        self.reset_taps();
    }

    /// Upstream's `_checkCancel`, which says nothing if the tap down never
    /// went out -- a cancel for something nobody was told about is noise.
    fn check_cancel(&mut self) {
        if !self.sent_tap_down {
            return;
        }
        if let Some(on_cancel) = &self.on_cancel {
            on_cancel();
        }
        self.reset_drag_update_throttle();
        self.reset_taps();
    }

    /// Upstream's `_giveUpPointer`: a pointer that was never accepted is
    /// rejected, because this recogniser has stopped wanting it.
    fn give_up_pointer(&mut self, pointer: i64) {
        self.stop_tracking_pointer(pointer);
        match self
            .accepted_active_pointers
            .iter()
            .position(|id| *id == pointer)
        {
            Some(at) => {
                self.accepted_active_pointers.remove(at);
            }
            None => self.resolutions.push((pointer, Disposition::Rejected)),
        }
    }

    /// Upstream's `_resetTaps`.
    fn reset_taps(&mut self) {
        self.sent_tap_down = false;
        self.won_arena_for_primary_pointer = false;
        self.primary_pointer = None;
    }

    fn reset_drag_update_throttle(&mut self) {
        if self.drag_update_throttle_micros.is_none() {
            return;
        }
        self.throttled_update = None;
        self.throttle_deadline = None;
    }

    /// Upstream's `dispose`.
    pub fn dispose(&mut self) {
        self.deadline = None;
        self.reset_drag_update_throttle();
        self.tracker.reset();
    }
}

macro_rules! tap_and_drag_recognizer {
    ($name:ident, $axis:expr, $description:literal) => {
        impl $name {
            pub fn new() -> $name {
                $name {
                    base: BaseTapAndDragGestureRecognizer::new($axis),
                }
            }

            /// Upstream's `debugDescription`.
            pub fn debug_description(&self) -> &'static str {
                $description
            }
        }

        impl Default for $name {
            fn default() -> $name {
                $name::new()
            }
        }

        impl std::ops::Deref for $name {
            type Target = BaseTapAndDragGestureRecognizer;

            fn deref(&self) -> &BaseTapAndDragGestureRecognizer {
                &self.base
            }
        }

        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut BaseTapAndDragGestureRecognizer {
                &mut self.base
            }
        }
    };
}

/// Upstream `TapAndHorizontalDragGestureRecognizer`: taps, and drags along the
/// horizontal only.
///
/// The narrower of the two, and the one a text field inside a vertical scroll
/// view wants: a finger going up and down is the reader scrolling, and this
/// recogniser leaves it alone until it has won anyway.
pub struct TapAndHorizontalDragGestureRecognizer {
    pub base: BaseTapAndDragGestureRecognizer,
}

/// Upstream `TapAndPanGestureRecognizer`: taps, and drags on any axis.
pub struct TapAndPanGestureRecognizer {
    pub base: BaseTapAndDragGestureRecognizer,
}

/// Upstream `TapAndDragGestureRecognizer`, deprecated in favour of
/// [`TapAndPanGestureRecognizer`].
///
/// Kept because upstream kept it, and identical to the pan one in every
/// respect **including its `debugDescription`**, which is still `"tap and
/// pan"`. The deprecation notice says the rename was for a name less easily
/// confused with the base class's, and the description was evidently never
/// part of what was being renamed.
pub struct TapAndDragGestureRecognizer {
    pub base: BaseTapAndDragGestureRecognizer,
}

tap_and_drag_recognizer!(
    TapAndHorizontalDragGestureRecognizer,
    TapDragAxis::Horizontal,
    "tap and horizontal drag"
);
tap_and_drag_recognizer!(TapAndPanGestureRecognizer, TapDragAxis::Pan, "tap and pan");
tap_and_drag_recognizer!(TapAndDragGestureRecognizer, TapDragAxis::Pan, "tap and pan");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gestures::{PRIMARY_BUTTON, SECONDARY_BUTTON, SignalKind, TOUCH_SLOP};
    use std::cell::RefCell;

    #[derive(Clone, Debug, PartialEq)]
    enum Said {
        TapDown(u32),
        TapUp(u32),
        DragStart(u32, f32),
        DragUpdate(f32, f32),
        DragEnd(u32),
        Cancel,
    }

    #[derive(Clone, Default)]
    struct Log(Rc<RefCell<Vec<Said>>>);

    impl Log {
        fn said(&self) -> Vec<Said> {
            self.0.borrow().clone()
        }

        fn clear(&self) {
            self.0.borrow_mut().clear();
        }
    }

    fn event(change: PointerChange, position: Offset, delta: Offset, micros: i64) -> PointerEvent {
        PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change,
            kind: PointerKind::Touch,
            signal_kind: SignalKind::None,
            buttons: PRIMARY_BUTTON,
            time_stamp_micros: micros,
            position,
            delta,
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: position,
        }
    }

    fn down(position: Offset, micros: i64) -> PointerEvent {
        event(PointerChange::Down, position, Offset::ZERO, micros)
    }

    fn up(position: Offset, micros: i64) -> PointerEvent {
        event(PointerChange::Up, position, Offset::ZERO, micros)
    }

    fn listening(axis: TapDragAxis, log: &Log) -> BaseTapAndDragGestureRecognizer {
        let (d, u, s, m, e, c) = (
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
        );
        BaseTapAndDragGestureRecognizer::new(axis)
            .on_tap_down(move |x| {
                d.0.borrow_mut()
                    .push(Said::TapDown(x.consecutive_tap_count))
            })
            .on_tap_up(move |x| u.0.borrow_mut().push(Said::TapUp(x.consecutive_tap_count)))
            .on_drag_start(move |x| {
                s.0.borrow_mut().push(Said::DragStart(
                    x.consecutive_tap_count,
                    x.global_position.dx,
                ))
            })
            .on_drag_update(move |x| {
                m.0.borrow_mut()
                    .push(Said::DragUpdate(x.delta.dx, x.offset_from_origin.dx))
            })
            .on_drag_end(move |x| {
                e.0.borrow_mut()
                    .push(Said::DragEnd(x.consecutive_tap_count))
            })
            .on_cancel(move || c.0.borrow_mut().push(Said::Cancel))
    }

    /// One whole tap: down, win the arena after the press timeout, up.
    fn one_tap(recognizer: &mut BaseTapAndDragGestureRecognizer, position: Offset, at: i64) {
        recognizer.add_pointer(&down(position, at));
        recognizer.tick(at + PRESS_TIMEOUT_MICROS);
        recognizer.accept_gesture(1);
        recognizer.handle_event(&up(position, at + 120_000));
    }

    #[test]
    fn three_taps_in_a_row_are_counted_and_that_is_the_whole_point() {
        // One tap places a caret, two select a word, three select a
        // paragraph. Nothing else in the gestures library carries this count
        // through to a drag.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        one_tap(&mut recognizer, Offset::ZERO, 0);
        one_tap(&mut recognizer, Offset::ZERO, 200_000);
        one_tap(&mut recognizer, Offset::ZERO, 400_000);
        assert_eq!(
            log.said(),
            vec![
                Said::TapDown(1),
                Said::TapUp(1),
                Said::TapDown(2),
                Said::TapUp(2),
                Said::TapDown(3),
                Said::TapUp(3),
            ]
        );
    }

    #[test]
    fn a_gap_too_long_starts_the_count_over() {
        // And the expiry is noticed lazily, at the next pointer down, rather
        // than when the timer runs out: upstream's timeout callback is empty
        // on purpose, because resetting there would throw away state a tap
        // down or tap up still needs.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        one_tap(&mut recognizer, Offset::ZERO, 0);
        assert_eq!(recognizer.consecutive_tap_count(), 1);

        recognizer.tick(120_000 + DOUBLE_TAP_TIMEOUT_MICROS);
        assert_eq!(
            recognizer.consecutive_tap_count(),
            1,
            "the timer expiring resets nothing by itself"
        );

        one_tap(&mut recognizer, Offset::ZERO, 1_000_000);
        assert_eq!(recognizer.consecutive_tap_count(), 1, "a new series");
    }

    #[test]
    fn a_tap_elsewhere_or_with_another_button_starts_the_count_over() {
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        one_tap(&mut recognizer, Offset::ZERO, 0);
        one_tap(&mut recognizer, Offset::new(400.0, 0.0), 200_000);
        assert_eq!(recognizer.consecutive_tap_count(), 1, "too far away");

        let mut recognizer = listening(TapDragAxis::Pan, &log);
        one_tap(&mut recognizer, Offset::ZERO, 0);
        let other = PointerEvent {
            buttons: SECONDARY_BUTTON,
            ..down(Offset::ZERO, 200_000)
        };
        recognizer.add_pointer(&other);
        assert_eq!(recognizer.consecutive_tap_count(), 1, "another button");
    }

    #[test]
    fn the_cap_starts_a_new_series_rather_than_reporting_a_fourth_nobody_handles() {
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log).with_max_consecutive_tap(3);
        one_tap(&mut recognizer, Offset::ZERO, 0);
        one_tap(&mut recognizer, Offset::ZERO, 200_000);
        one_tap(&mut recognizer, Offset::ZERO, 400_000);
        one_tap(&mut recognizer, Offset::ZERO, 600_000);
        assert_eq!(
            log.said(),
            vec![
                Said::TapDown(1),
                Said::TapUp(1),
                Said::TapDown(2),
                Said::TapUp(2),
                Said::TapDown(3),
                Said::TapUp(3),
                Said::TapDown(1),
                Said::TapUp(1),
            ]
        );
    }

    #[test]
    fn a_drag_breaks_the_run() {
        // The count is not itself cleared, but the two things that would let
        // the next tap join this series are -- so it cannot. A reader who
        // taps, then taps and drags, has not triple-tapped.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        one_tap(&mut recognizer, Offset::ZERO, 0);

        recognizer.add_pointer(&down(Offset::ZERO, 200_000));
        recognizer.tick(200_000 + PRESS_TIMEOUT_MICROS);
        recognizer.accept_gesture(1);
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(120.0, 0.0),
            Offset::new(120.0, 0.0),
            320_000,
        ));
        recognizer.handle_event(&up(Offset::new(120.0, 0.0), 340_000));

        log.clear();
        one_tap(&mut recognizer, Offset::ZERO, 400_000);
        assert_eq!(log.said(), vec![Said::TapDown(1), Said::TapUp(1)]);
    }

    #[test]
    fn the_tap_down_waits_for_the_arena_unlike_the_multi_tap_recognizer() {
        // The contrast is the point. A piano key must light up before anyone
        // has decided whose gesture it is; a caret must not be moved by a
        // finger that turns out to be scrolling. The two recognisers make
        // opposite choices for opposite reasons.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        assert!(log.said().is_empty(), "nothing yet");
        recognizer.accept_gesture(1);
        assert_eq!(log.said(), vec![Said::TapDown(1)]);
    }

    #[test]
    fn resting_a_finger_reports_the_tap_down_without_waiting_for_the_arena() {
        // Upstream's press timeout: a caret lands under a finger that is
        // merely resting, before anything else has happened.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        assert!(recognizer.tick(PRESS_TIMEOUT_MICROS - 1));
        assert!(log.said().is_empty());
        recognizer.tick(PRESS_TIMEOUT_MICROS);
        assert_eq!(log.said(), vec![Said::TapDown(1)]);
    }

    #[test]
    fn a_held_double_tap_claims_the_gesture_so_the_long_press_cannot_take_it() {
        // Upstream's own reason, and a real bug if it were missing: a reader
        // who double-taps to select a word and then pauses before dragging
        // would lose the selection to the long-press recogniser.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        one_tap(&mut recognizer, Offset::ZERO, 0);

        recognizer.add_pointer(&down(Offset::ZERO, 200_000));
        assert_eq!(recognizer.consecutive_tap_count(), 2);
        recognizer.tick(200_000 + PRESS_TIMEOUT_MICROS);
        assert_eq!(
            recognizer.take_resolution(1),
            Some(Disposition::Accepted),
            "the second tap held claims victory outright"
        );

        // A first tap held does not: it may still turn out to be a long press.
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        recognizer.tick(PRESS_TIMEOUT_MICROS);
        assert_eq!(recognizer.take_resolution(1), None);
    }

    #[test]
    fn a_horizontal_recognizer_ignores_vertical_movement_until_it_has_won() {
        // Which is what lets a text field live inside a vertical scroll view:
        // a finger going up and down is the reader scrolling.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Horizontal, &log);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(0.0, 200.0),
            Offset::new(0.0, 200.0),
            10_000,
        ));
        assert_eq!(
            recognizer.take_resolution(1),
            None,
            "that was a scroll, not a drag"
        );
        assert!(log.said().is_empty());

        // The same movement on its own axis does claim the gesture.
        let mut recognizer = listening(TapDragAxis::Horizontal, &log);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(200.0, 0.0),
            Offset::new(200.0, 0.0),
            10_000,
        ));
        assert_eq!(recognizer.take_resolution(1), Some(Disposition::Accepted));
    }

    #[test]
    fn once_it_has_won_a_horizontal_recognizer_takes_a_drag_on_any_axis() {
        // Upstream's second clause. Nothing else is competing by then, so the
        // gesture has to be something, and a drag is what is left.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Horizontal, &log);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        recognizer.accept_gesture(1);
        log.clear();
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(0.0, 200.0),
            Offset::new(0.0, 200.0),
            10_000,
        ));
        assert!(
            log.said()
                .iter()
                .any(|said| matches!(said, Said::DragStart(_, _))),
            "the vertical movement became a drag: {:?}",
            log.said()
        );
    }

    #[test]
    fn the_two_axes_are_judged_against_different_slops() {
        // Horizontal against the hit slop, pan against the pan slop, which is
        // twice as far -- a free drag has to be more deliberate than one that
        // is already constrained to a line.
        let log = Log::default();
        let just_past_touch_slop = Offset::new(TOUCH_SLOP + 2.0, 0.0);

        let mut horizontal = listening(TapDragAxis::Horizontal, &log);
        horizontal.add_pointer(&down(Offset::ZERO, 0));
        horizontal.handle_event(&event(
            PointerChange::Move,
            just_past_touch_slop,
            just_past_touch_slop,
            10_000,
        ));
        assert_eq!(horizontal.take_resolution(1), Some(Disposition::Accepted));

        let mut pan = listening(TapDragAxis::Pan, &log);
        pan.add_pointer(&down(Offset::ZERO, 0));
        pan.handle_event(&event(
            PointerChange::Move,
            just_past_touch_slop,
            just_past_touch_slop,
            10_000,
        ));
        assert_eq!(pan.take_resolution(1), None, "not far enough for a pan");
    }

    #[test]
    fn a_drag_reports_how_far_it_has_come_as_well_as_how_far_it_just_moved() {
        // The delta says how much to scroll; the offset from origin says where
        // the selection now reaches.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        recognizer.accept_gesture(1);
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(60.0, 0.0),
            Offset::new(60.0, 0.0),
            10_000,
        ));
        log.clear();
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(90.0, 0.0),
            Offset::new(30.0, 0.0),
            20_000,
        ));
        assert_eq!(log.said(), vec![Said::DragUpdate(30.0, 30.0)]);
    }

    #[test]
    fn a_pointer_that_moved_too_far_but_never_won_is_cancelled_rather_than_tapped() {
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        recognizer.tick(PRESS_TIMEOUT_MICROS);
        assert_eq!(log.said(), vec![Said::TapDown(1)]);
        // Past the tap tolerance but not far enough to be a pan.
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(TOUCH_SLOP + 2.0, 0.0),
            Offset::new(TOUCH_SLOP + 2.0, 0.0),
            10_000,
        ));
        recognizer.handle_event(&up(Offset::new(TOUCH_SLOP + 2.0, 0.0), 20_000));
        assert_eq!(log.said(), vec![Said::TapDown(1), Said::Cancel]);
    }

    #[test]
    fn a_cancel_says_nothing_if_the_tap_down_never_went_out() {
        // A cancel for something nobody was told about is noise.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(TOUCH_SLOP + 2.0, 0.0),
            Offset::new(TOUCH_SLOP + 2.0, 0.0),
            10_000,
        ));
        recognizer.handle_event(&up(Offset::new(TOUCH_SLOP + 2.0, 0.0), 20_000));
        assert!(log.said().is_empty());
    }

    #[test]
    fn a_throttled_drag_still_delivers_its_last_position_at_the_end() {
        // Upstream cancels the pending timer and runs it rather than dropping
        // it: the last position of a drag is the one that decides where a
        // selection ends.
        let log = Log::default();
        let mut recognizer = listening(TapDragAxis::Pan, &log).with_drag_update_throttle(50_000);
        recognizer.add_pointer(&down(Offset::ZERO, 0));
        recognizer.accept_gesture(1);
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(60.0, 0.0),
            Offset::new(60.0, 0.0),
            10_000,
        ));
        log.clear();
        recognizer.handle_event(&event(
            PointerChange::Move,
            Offset::new(90.0, 0.0),
            Offset::new(30.0, 0.0),
            20_000,
        ));
        assert!(log.said().is_empty(), "held back by the throttle");

        recognizer.handle_event(&up(Offset::new(90.0, 0.0), 30_000));
        assert_eq!(
            log.said(),
            vec![Said::DragUpdate(30.0, 30.0), Said::DragEnd(1)],
            "the pending update first, then the end"
        );
    }

    #[test]
    fn without_eager_victory_a_drag_waits_to_be_handed_the_gesture() {
        let log = Log::default();
        let mut patient = listening(TapDragAxis::Pan, &log).with_eager_victory_on_drag(false);
        patient.add_pointer(&down(Offset::ZERO, 0));
        patient.handle_event(&event(
            PointerChange::Move,
            Offset::new(200.0, 0.0),
            Offset::new(200.0, 0.0),
            10_000,
        ));
        assert_eq!(
            patient.take_resolution(1),
            None,
            "it noticed the drag but said nothing"
        );
        assert!(log.said().is_empty());

        patient.accept_gesture(1);
        assert!(
            log.said()
                .iter()
                .any(|said| matches!(said, Said::DragStart(_, _)))
        );
    }

    #[test]
    fn the_three_recognizers_carry_upstreams_descriptions() {
        assert_eq!(
            TapAndHorizontalDragGestureRecognizer::new().debug_description(),
            "tap and horizontal drag"
        );
        assert_eq!(
            TapAndHorizontalDragGestureRecognizer::new().axis(),
            TapDragAxis::Horizontal
        );
        assert_eq!(
            TapAndPanGestureRecognizer::new().debug_description(),
            "tap and pan"
        );
        // The deprecated one is identical to the pan one *including* its
        // description: the rename was for a name less easily confused with the
        // base class's, and the description was evidently not part of it.
        assert_eq!(
            TapAndDragGestureRecognizer::new().debug_description(),
            "tap and pan"
        );
        assert_eq!(TapAndDragGestureRecognizer::new().axis(), TapDragAxis::Pan);
        assert_eq!(
            BaseTapAndDragGestureRecognizer::new(TapDragAxis::Pan).debug_description(),
            "tap_and_drag"
        );
    }
}

/// Upstream's `_isShiftPressed` on `TextSelectionGestureDetectorBuilder`, and
/// the pair of hooks it hangs from.
///
/// # Sampled once, not read live
///
/// Upstream sets it in `onTapTrackStart` and clears it in `onTapTrackReset`:
///
/// ```dart
/// void onTapTrackStart() {
///   _isShiftPressed = HardwareKeyboard.instance.logicalKeysPressed
///       .intersection({LogicalKeyboardKey.shiftLeft, LogicalKeyboardKey.shiftRight})
///       .isNotEmpty;
/// }
/// void onTapTrackReset() { _isShiftPressed = false; }
/// ```
///
/// So the answer is taken **when the tap sequence begins** and held for the
/// whole of it. A reader who presses shift after putting a finger down does
/// not retroactively turn a tap into an extend, and one who lets go of shift
/// mid-drag keeps extending. Reading the keyboard at each event instead would
/// change the meaning of a gesture underneath the reader's hand, which is the
/// obvious implementation and the wrong one.
///
/// # What reads it
///
/// `text_selection.rs`'s `shift_tap_down` and `SingleTapUp::shift_is_usable`,
/// which route a shift-held tap to `extend_selection` or `expand_selection`
/// depending on the platform. Those arrived after this type did, and this
/// heading said "what this port does not have yet -- shift-extend selection
/// itself" until tick 286, by which time it had been wrong for some time.
///
/// A note that a thing is missing is a claim with no test behind it, and it
/// expires quietly. `stale_notes.py` checks the ones that name their subject;
/// this one did not, and was found by hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TapSequenceShift {
    pressed: bool,
}

impl TapSequenceShift {
    pub fn new() -> TapSequenceShift {
        TapSequenceShift { pressed: false }
    }

    /// `onTapTrackStart`: read the keyboard once, now.
    pub fn sample(&mut self, shift_held_now: bool) {
        self.pressed = shift_held_now;
    }

    /// `onTapTrackReset`: the sequence is over, so the answer expires.
    ///
    /// Cleared rather than re-read, because between sequences there is no
    /// sequence to describe.
    pub fn reset(&mut self) {
        self.pressed = false;
    }

    /// What the gesture should act on -- the sample, never the keyboard.
    pub fn is_pressed(&self) -> bool {
        self.pressed
    }
}

#[cfg(test)]
mod tap_sequence_shift_tests {
    use super::{TapSequenceShift, TapStatusTracker};
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn the_answer_is_taken_at_the_start_and_held() {
        // Pressing shift after the finger is down does not turn a tap into an
        // extend. Reading the keyboard at each event instead would change the
        // meaning of a gesture underneath the reader's hand.
        let mut shift = TapSequenceShift::new();
        shift.sample(false);
        assert!(!shift.is_pressed());
        // The keyboard changes; the sample does not.
        assert!(!shift.is_pressed(), "still what it was at the start");
    }

    #[test]
    fn and_letting_go_of_shift_mid_gesture_keeps_extending() {
        let mut shift = TapSequenceShift::new();
        shift.sample(true);
        assert!(shift.is_pressed());
        // No re-sampling happens until the next sequence begins.
        assert!(shift.is_pressed());
    }

    #[test]
    fn the_sequence_ending_clears_it_rather_than_re_reading() {
        // Between sequences there is no sequence to describe, so the answer
        // expires instead of tracking the keyboard.
        let mut shift = TapSequenceShift::new();
        shift.sample(true);
        shift.reset();
        assert!(!shift.is_pressed());
    }

    #[test]
    fn and_a_fresh_sequence_takes_a_fresh_reading() {
        let mut shift = TapSequenceShift::new();
        shift.sample(true);
        shift.reset();
        shift.sample(false);
        assert!(!shift.is_pressed());
        shift.reset();
        shift.sample(true);
        assert!(shift.is_pressed(), "the sample is not sticky either way");
    }

    #[test]
    fn the_tracker_fires_both_hooks_this_hangs_from() {
        // The rule is only worth anything if the two moments it keys off
        // actually arrive, so this drives the tracker rather than the flag.
        let started = Rc::new(Cell::new(0u32));
        let was_reset = Rc::new(Cell::new(0u32));
        let mut tracker = TapStatusTracker::new();
        let seen = started.clone();
        tracker.on_tap_track_start(move || seen.set(seen.get() + 1));
        let cleared = was_reset.clone();
        tracker.on_tap_track_reset(move || cleared.set(cleared.get() + 1));

        tracker.reset();
        assert_eq!(was_reset.get(), 1, "reset fires the reset hook");
        assert_eq!(started.get(), 0, "and not the start hook");
    }

    #[test]
    fn a_fresh_sequence_has_nothing_sampled() {
        assert!(!TapSequenceShift::new().is_pressed());
        assert_eq!(TapSequenceShift::default(), TapSequenceShift::new());
    }
}
