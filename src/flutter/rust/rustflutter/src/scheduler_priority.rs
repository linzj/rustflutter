//! Ports of `scheduler/priority.dart`, the performance-mode half of
//! `scheduler/binding.dart`, and `gestures/binding.dart`'s
//! `FlutterErrorDetailsForPointerEventDispatcher`.
//!
//! Framework plumbing, and three small designs worth keeping.

/// Upstream `Priority`, a task priority for `SchedulerBinding.scheduleTask`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Priority {
    value: i64,
}

impl Priority {
    /// *"A task to run after all other tasks, when no animations are running."*
    pub const IDLE: Priority = Priority { value: 0 };
    /// *"A task to run even when animations are running."*
    pub const ANIMATION: Priority = Priority { value: 100000 };
    /// *"A task to run even when the user is interacting with the device."*
    pub const TOUCH: Priority = Priority { value: 200000 };

    /// Upstream `kMaxOffset`, *"Maximum offset by which to clamp relative
    /// priorities."*
    ///
    /// The three named priorities sit 100,000 apart and this is 10,000, so
    /// **the gap between neighbours is ten times the largest single step.** One
    /// relative offset can never carry a task from idle up into animation
    /// territory; the spacing was picked so that it cannot.
    ///
    /// And the doc does not pretend otherwise about what it is:
    ///
    /// > It is still possible to have priorities that are offset by more than
    /// > this amount **by repeatedly taking relative offsets**, but that is
    /// > generally discouraged.
    ///
    /// **A speed bump, not a wall** -- ten hops of the maximum will get you
    /// there -- and the class says so rather than implying a guarantee it does
    /// not have.
    pub const MAX_OFFSET: i64 = 10000;

    pub fn value(&self) -> i64 {
        self.value
    }

    /// Upstream `operator +`, where a positive offset means **higher** priority.
    ///
    /// Note what is clamped: the offset, not the result. Which is exactly why
    /// repeating works.
    pub fn raise(self, offset: i64) -> Priority {
        let offset = if offset.abs() > Priority::MAX_OFFSET {
            Priority::MAX_OFFSET * offset.signum()
        } else {
            offset
        };
        Priority {
            value: self.value + offset,
        }
    }

    /// Upstream `operator -`, defined as `this + (-offset)`.
    ///
    /// **One implementation and an alias**, rather than two clamps written out
    /// -- so the two operators cannot drift apart, which is more than can be
    /// said for several of the duplicated blocks this sweep has turned up.
    pub fn lower(self, offset: i64) -> Priority {
        self.raise(-offset)
    }
}

/// Upstream `DartPerformanceMode`, as far as the request logic cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DartPerformanceMode {
    Balanced,
    Latency,
    Throughput,
    Memory,
}

/// Upstream `PerformanceModeRequestHandle`.
///
/// A handle whose only method is `dispose`, and whose doc says *"This method
/// must only be called once per object."* Upstream nulls its cleanup callback
/// after running it, so a second call trips an assert in debug and dereferences
/// null in release.
///
/// **The handle is the request.** Holding it is what keeps the mode; letting it
/// go is how you withdraw. The same shape as `KeepAliveHandle` -- an object with
/// no content, whose whole meaning is that it has not been disposed yet.
#[derive(Debug, PartialEq, Eq)]
pub struct PerformanceModeRequestHandle {
    live: bool,
}

impl PerformanceModeRequestHandle {
    fn new() -> PerformanceModeRequestHandle {
        PerformanceModeRequestHandle { live: true }
    }

    pub fn is_live(&self) -> bool {
        self.live
    }

    /// Returns whether the call was the legitimate first one. Upstream asserts
    /// instead.
    pub fn dispose(&mut self) -> bool {
        if !self.live {
            return false;
        }
        self.live = false;
        true
    }
}

/// The performance-mode half of upstream `SchedulerBinding`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SchedulerBinding {
    performance_mode: Option<DartPerformanceMode>,
    request_count: usize,
}

impl SchedulerBinding {
    pub fn new() -> SchedulerBinding {
        SchedulerBinding::default()
    }

    /// Upstream `requestPerformanceMode`, whose signature already tells you the
    /// design: it returns `PerformanceModeRequestHandle?`.
    ///
    /// ```dart
    /// // conflicting requests are not allowed.
    /// if (_performanceMode != null && _performanceMode != mode) {
    ///   return null;
    /// }
    /// ```
    ///
    /// Three outcomes. Nothing set, and you take it. The same mode set, and the
    /// count goes up so the mode holds until the last handle is gone. **A
    /// different mode set, and you get null** -- not an exception, not an
    /// override, not a queue. **First request wins, and a later disagreeing one
    /// is simply refused.**
    ///
    /// There is no way to force it, which is the point: two parts of an
    /// application asking for opposite engine behaviour cannot both be right,
    /// and quietly letting the second win would make the result depend on
    /// startup order.
    pub fn request_performance_mode(
        &mut self,
        mode: DartPerformanceMode,
    ) -> Option<PerformanceModeRequestHandle> {
        match self.performance_mode {
            Some(current) if current != mode => None,
            Some(_) => {
                self.request_count += 1;
                Some(PerformanceModeRequestHandle::new())
            }
            None => {
                self.performance_mode = Some(mode);
                self.request_count = 1;
                Some(PerformanceModeRequestHandle::new())
            }
        }
    }

    /// Upstream `_disposePerformanceModeRequest`: the mode is released only when
    /// the last outstanding handle goes.
    pub fn dispose_performance_mode_request(&mut self) {
        self.request_count = self.request_count.saturating_sub(1);
        if self.request_count == 0 {
            self.performance_mode = None;
        }
    }

    pub fn performance_mode(&self) -> Option<DartPerformanceMode> {
        self.performance_mode
    }

    pub fn request_count(&self) -> usize {
        self.request_count
    }
}

/// Upstream `FlutterErrorDetailsForPointerEventDispatcher`.
///
/// A `FlutterErrorDetails` with two extra fields, and both answer the question
/// an exception from a gesture handler otherwise leaves open: **which one?** A
/// pointer route can have a dozen handlers on it, so "an exception in
/// handleEvent" is not a report until you know which event was in flight and
/// which target threw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlutterErrorDetailsForPointerEventDispatcher {
    pub exception: String,
    pub library: Option<String>,
    /// *"The pointer event that was being routed when the exception was
    /// raised."*
    pub event: Option<PointerEventKind>,
    /// *"May be null if no hit test entry is associated with the event (e.g.
    /// `PointerHoverEvent`s, `PointerAddedEvent`s, and `PointerRemovedEvent`s)."*
    ///
    /// The nullability is not defensive. Those three are **routed without a hit
    /// test at all**, so there is no entry to name -- the field is null for a
    /// whole category of events rather than for a failure.
    pub hit_test_entry: Option<u64>,
}

/// The pointer events that matter to the field above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEventKind {
    Down,
    Move,
    Up,
    Cancel,
    Hover,
    Added,
    Removed,
}

impl PointerEventKind {
    /// Whether an event of this kind is routed through a hit test, and so
    /// whether [`FlutterErrorDetailsForPointerEventDispatcher::hit_test_entry`]
    /// can be filled in.
    pub fn has_hit_test_entry(self) -> bool {
        !matches!(
            self,
            PointerEventKind::Hover | PointerEventKind::Added | PointerEventKind::Removed
        )
    }
}

impl FlutterErrorDetailsForPointerEventDispatcher {
    pub fn new(exception: impl Into<String>) -> FlutterErrorDetailsForPointerEventDispatcher {
        FlutterErrorDetailsForPointerEventDispatcher {
            exception: exception.into(),
            library: Some("gesture library".to_string()),
            event: None,
            hit_test_entry: None,
        }
    }

    pub fn for_event(
        exception: impl Into<String>,
        event: PointerEventKind,
        target: u64,
    ) -> FlutterErrorDetailsForPointerEventDispatcher {
        FlutterErrorDetailsForPointerEventDispatcher {
            event: Some(event),
            hit_test_entry: event.has_hit_test_entry().then_some(target),
            ..FlutterErrorDetailsForPointerEventDispatcher::new(exception)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- A speed bump, not a wall -----------------------------------------------------

    #[test]
    fn one_step_can_never_carry_a_task_from_idle_into_animation() {
        let bumped = Priority::IDLE.raise(1_000_000);
        assert_eq!(bumped.value(), Priority::MAX_OFFSET);
        assert!(bumped < Priority::ANIMATION);
    }

    #[test]
    fn but_the_doc_admits_that_repeating_gets_you_there() {
        // The clamp is on the offset, not the result, which is why.
        let mut priority = Priority::IDLE;
        for _ in 0..10 {
            priority = priority.raise(Priority::MAX_OFFSET);
        }
        assert_eq!(priority, Priority::ANIMATION);
    }

    #[test]
    fn the_named_priorities_are_ten_maximum_steps_apart() {
        assert_eq!(
            Priority::ANIMATION.value() - Priority::IDLE.value(),
            Priority::MAX_OFFSET * 10
        );
        assert_eq!(
            Priority::TOUCH.value() - Priority::ANIMATION.value(),
            Priority::MAX_OFFSET * 10
        );
    }

    #[test]
    fn lowering_is_raising_by_a_negative_so_the_two_cannot_drift_apart() {
        assert_eq!(Priority::TOUCH.lower(500), Priority::TOUCH.raise(-500));
        assert_eq!(
            Priority::TOUCH.lower(1_000_000),
            Priority::TOUCH.raise(-Priority::MAX_OFFSET),
            "and the clamp applies through the alias"
        );
    }

    #[test]
    fn a_positive_offset_means_higher_and_a_positive_lowering_means_lower() {
        assert!(Priority::IDLE.raise(5) > Priority::IDLE);
        assert!(Priority::ANIMATION.lower(5) < Priority::ANIMATION);
    }

    // -- First request wins ------------------------------------------------------------

    #[test]
    fn a_conflicting_request_is_refused_rather_than_overriding_or_queueing() {
        let mut binding = SchedulerBinding::new();
        assert!(
            binding
                .request_performance_mode(DartPerformanceMode::Latency)
                .is_some()
        );
        assert_eq!(
            binding.performance_mode(),
            Some(DartPerformanceMode::Latency)
        );

        assert!(
            binding
                .request_performance_mode(DartPerformanceMode::Throughput)
                .is_none(),
            "no exception, no override, no queue"
        );
        assert_eq!(
            binding.performance_mode(),
            Some(DartPerformanceMode::Latency),
            "and the first request is untouched"
        );
    }

    #[test]
    fn the_same_mode_is_shared_and_counted() {
        let mut binding = SchedulerBinding::new();
        binding.request_performance_mode(DartPerformanceMode::Latency);
        binding.request_performance_mode(DartPerformanceMode::Latency);
        assert_eq!(binding.request_count(), 2);

        binding.dispose_performance_mode_request();
        assert_eq!(
            binding.performance_mode(),
            Some(DartPerformanceMode::Latency),
            "one holder left, so the mode holds"
        );

        binding.dispose_performance_mode_request();
        assert_eq!(binding.performance_mode(), None);
    }

    #[test]
    fn and_once_released_a_different_mode_may_have_it() {
        let mut binding = SchedulerBinding::new();
        binding.request_performance_mode(DartPerformanceMode::Latency);
        binding.dispose_performance_mode_request();
        assert!(
            binding
                .request_performance_mode(DartPerformanceMode::Throughput)
                .is_some()
        );
        assert_eq!(
            binding.performance_mode(),
            Some(DartPerformanceMode::Throughput)
        );
    }

    #[test]
    fn the_handle_is_the_request_and_may_only_be_let_go_once() {
        let mut binding = SchedulerBinding::new();
        let mut handle = binding
            .request_performance_mode(DartPerformanceMode::Latency)
            .expect("nothing else holds it");
        assert!(handle.is_live());

        assert!(handle.dispose());
        assert!(!handle.is_live());
        assert!(!handle.dispose(), "upstream asserts on the second call");
    }

    // -- Which one? ----------------------------------------------------------------------

    #[test]
    fn three_kinds_of_event_are_routed_without_a_hit_test_at_all() {
        for kind in [
            PointerEventKind::Hover,
            PointerEventKind::Added,
            PointerEventKind::Removed,
        ] {
            assert!(!kind.has_hit_test_entry(), "{kind:?}");
        }
        for kind in [
            PointerEventKind::Down,
            PointerEventKind::Move,
            PointerEventKind::Up,
            PointerEventKind::Cancel,
        ] {
            assert!(kind.has_hit_test_entry(), "{kind:?}");
        }
    }

    #[test]
    fn so_a_null_entry_names_a_category_rather_than_a_failure() {
        let from_tap = FlutterErrorDetailsForPointerEventDispatcher::for_event(
            "boom",
            PointerEventKind::Down,
            7,
        );
        assert_eq!(from_tap.hit_test_entry, Some(7));

        let from_hover = FlutterErrorDetailsForPointerEventDispatcher::for_event(
            "boom",
            PointerEventKind::Hover,
            7,
        );
        assert_eq!(from_hover.hit_test_entry, None);
        assert_eq!(
            from_hover.event,
            Some(PointerEventKind::Hover),
            "the event is still named, which is the half that always exists"
        );
    }

    #[test]
    fn the_details_say_which_library_threw() {
        let details = FlutterErrorDetailsForPointerEventDispatcher::new("boom");
        assert_eq!(details.library.as_deref(), Some("gesture library"));
        assert_eq!(details.exception, "boom");
    }
}
