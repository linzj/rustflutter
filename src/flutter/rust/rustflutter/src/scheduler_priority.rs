//! Ports of `scheduler/priority.dart`, the task-queue and performance-mode
//! halves of `scheduler/binding.dart`, and `gestures/binding.dart`'s
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

/// One queued task: what to run and how badly it wants to.
struct TaskEntry {
    priority: i64,
    task: Box<dyn FnOnce()>,
}

/// The task-queue and performance-mode halves of upstream `SchedulerBinding`.
///
/// [`Priority`] was ported here with nothing that consumed it. This is what it
/// was for: `scheduleTask` puts work behind the frame rather than in front of
/// it, and the priority decides which side of an animation a task lands on.
#[derive(Default)]
pub struct SchedulerBinding {
    performance_mode: Option<DartPerformanceMode>,
    request_count: usize,
    /// Upstream's `_taskQueue`, a heap. A sorted `Vec` here, which is the same
    /// order and the wrong complexity for a queue that never holds more than a
    /// handful.
    tasks: Vec<TaskEntry>,
    /// Upstream's `BindingBase.locked`: true while the binding is in the
    /// middle of something that must not be re-entered.
    locked: bool,
    /// Upstream's `transientCallbackCount`, which here is set rather than
    /// counted -- this port has no frame-callback register on this type, and
    /// the scheduling strategy only ever asks whether it is above zero.
    transient_callback_count: usize,
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

    // -- The task queue ------------------------------------------------------

    /// Upstream `SchedulerBinding.scheduleTask`.
    ///
    /// ```dart
    /// final bool isFirstTask = _taskQueue.isEmpty;
    /// _taskQueue.add(entry);
    /// if (isFirstTask && !locked) {
    ///   _ensureEventLoopCallback();
    /// }
    /// ```
    ///
    /// The event loop is kicked **only for the first task**, and not while
    /// locked. Both halves are about not asking twice: a queue that already
    /// has work in it already has a callback coming, and a locked binding gets
    /// its kick from [`SchedulerBinding::unlocked`] instead. Kicking on every
    /// `scheduleTask` would queue one event-loop callback per task and run
    /// them all in one turn, which is the thing the queue exists to avoid.
    ///
    /// Returns whether this call is the one that has to kick the loop, which
    /// is what `_ensureEventLoopCallback` would have done.
    pub fn schedule_task(&mut self, priority: Priority, task: impl FnOnce() + 'static) -> bool {
        let is_first_task = self.tasks.is_empty();
        self.tasks.push(TaskEntry {
            priority: priority.value(),
            task: Box::new(task),
        });
        // Upstream's `_taskSorter` is `-e1.priority.compareTo(e2.priority)`:
        // **descending**, so the highest priority is at the head. A stable
        // sort keeps equal priorities in the order they were scheduled, which
        // upstream's heap does not promise -- a difference worth knowing about
        // rather than one to rely on.
        self.tasks.sort_by(|a, b| b.priority.cmp(&a.priority));
        is_first_task && !self.locked
    }

    pub fn pending_tasks(&self) -> usize {
        self.tasks.len()
    }

    /// Upstream's `locked` setter by way of `unlocked()`.
    ///
    /// ```dart
    /// void unlocked() {
    ///   super.unlocked();
    ///   if (_taskQueue.isNotEmpty) {
    ///     _ensureEventLoopCallback();
    ///   }
    /// }
    /// ```
    ///
    /// Returns whether unlocking has to kick the loop -- work that arrived
    /// while locked got no kick of its own, so this is where it comes from.
    pub fn set_locked(&mut self, locked: bool) -> bool {
        let was_locked = self.locked;
        self.locked = locked;
        was_locked && !locked && !self.tasks.is_empty()
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Upstream's `transientCallbackCount`, which the default scheduling
    /// strategy reads.
    pub fn set_transient_callback_count(&mut self, count: usize) {
        self.transient_callback_count = count;
    }

    pub fn transient_callback_count(&self) -> usize {
        self.transient_callback_count
    }

    /// Upstream `defaultSchedulingStrategy`.
    ///
    /// ```dart
    /// if (scheduler.transientCallbackCount > 0) {
    ///   return priority >= Priority.animation.value;
    /// }
    /// return true;
    /// ```
    ///
    /// While a frame callback is registered -- which is to say while something
    /// is animating -- only [`Priority::ANIMATION`] and above run. That is the
    /// whole reason the three named priorities are spaced the way they are:
    /// [`Priority::IDLE`] work is exactly the work that must not compete with
    /// a running animation for the frame budget, and it waits.
    pub fn default_scheduling_strategy(&self, priority: i64) -> bool {
        if self.transient_callback_count > 0 {
            return priority >= Priority::ANIMATION.value();
        }
        true
    }

    /// Upstream `SchedulerBinding.handleEventLoopCallback`: run the
    /// highest-priority task **if it is of a high enough priority**.
    ///
    /// The return value is the subtle part, and upstream's doc spells it out:
    ///
    /// > Returns false if the scheduler is locked, or if there are no tasks
    /// > remaining.
    /// >
    /// > Returns true otherwise, **including when no task is executed due to
    /// > priority being too low**.
    ///
    /// The caller re-arms itself on true. So "there is work, but not yet" and
    /// "I ran something" have to give the same answer: if a starved task
    /// returned false the loop would stop, and that task would still be
    /// waiting when the animation ended with nothing left to wake it. False is
    /// reserved for the two cases where the loop genuinely has nothing to come
    /// back to -- and even then `unlocked` and `schedule_task` will start it
    /// again.
    ///
    /// Only the head is consulted. It is the highest priority there is, so a
    /// head the strategy refuses means every task is refused.
    pub fn handle_event_loop_callback(&mut self) -> bool {
        if self.tasks.is_empty() || self.locked {
            return false;
        }
        if self.default_scheduling_strategy(self.tasks[0].priority) {
            let entry = self.tasks.remove(0);
            (entry.task)();
        }
        true
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

    // -- The task queue, which is what `Priority` was ported for -------------

    use std::cell::RefCell;
    use std::rc::Rc;

    /// A scheduler and the log its tasks write to.
    fn recording() -> (SchedulerBinding, Rc<RefCell<Vec<&'static str>>>) {
        (SchedulerBinding::new(), Rc::new(RefCell::new(Vec::new())))
    }

    fn push(
        binding: &mut SchedulerBinding,
        log: &Rc<RefCell<Vec<&'static str>>>,
        priority: Priority,
        name: &'static str,
    ) -> bool {
        let log = Rc::clone(log);
        binding.schedule_task(priority, move || log.borrow_mut().push(name))
    }

    #[test]
    fn the_highest_priority_runs_first_however_it_was_queued() {
        let (mut binding, log) = recording();
        push(&mut binding, &log, Priority::IDLE, "idle");
        push(&mut binding, &log, Priority::TOUCH, "touch");
        push(&mut binding, &log, Priority::ANIMATION, "animation");

        while binding.pending_tasks() > 0 {
            binding.handle_event_loop_callback();
        }
        assert_eq!(*log.borrow(), ["touch", "animation", "idle"]);
    }

    #[test]
    fn only_the_first_task_kicks_the_event_loop() {
        // Upstream: `if (isFirstTask && !locked) _ensureEventLoopCallback()`.
        // A queue with work in it already has a callback coming, and kicking
        // per task would run the whole queue in one turn -- which is the thing
        // the queue exists to avoid.
        let (mut binding, log) = recording();
        assert!(push(&mut binding, &log, Priority::IDLE, "first"));
        assert!(!push(&mut binding, &log, Priority::TOUCH, "second"));
        assert!(!push(&mut binding, &log, Priority::IDLE, "third"));

        // And once the queue drains, the next one kicks again.
        while binding.pending_tasks() > 0 {
            binding.handle_event_loop_callback();
        }
        assert!(push(&mut binding, &log, Priority::IDLE, "later"));
    }

    #[test]
    fn work_that_arrives_while_locked_is_kicked_by_the_unlocking() {
        // The other half of the same rule. A locked binding gets no kick from
        // `scheduleTask`, so without `unlocked()` doing it the task would sit
        // there with nothing coming to run it.
        let (mut binding, log) = recording();
        binding.set_locked(true);
        assert!(
            !push(&mut binding, &log, Priority::TOUCH, "queued"),
            "no kick while locked, even for the first task"
        );
        assert!(
            !binding.handle_event_loop_callback(),
            "and the loop refuses to run one"
        );
        assert!(binding.set_locked(false), "the unlocking is the kick");
        assert!(binding.handle_event_loop_callback());
        assert_eq!(*log.borrow(), ["queued"]);
    }

    #[test]
    fn unlocking_an_empty_queue_kicks_nothing() {
        let mut binding = SchedulerBinding::new();
        binding.set_locked(true);
        assert!(!binding.set_locked(false));
    }

    // -- The scheduling strategy ---------------------------------------------

    #[test]
    fn an_animation_starves_idle_work_and_lets_animation_work_through() {
        // `defaultSchedulingStrategy`: while a frame callback is registered,
        // only `Priority.animation` and above run. That is what the spacing of
        // the three named priorities is for -- idle work is exactly the work
        // that must not compete with a running animation for the frame budget.
        let (mut binding, log) = recording();
        binding.set_transient_callback_count(1);
        push(&mut binding, &log, Priority::IDLE, "idle");
        push(&mut binding, &log, Priority::ANIMATION, "animation");

        binding.handle_event_loop_callback();
        assert_eq!(*log.borrow(), ["animation"], "the higher one goes");

        binding.handle_event_loop_callback();
        assert_eq!(*log.borrow(), ["animation"], "and the idle one waits");
        assert_eq!(binding.pending_tasks(), 1);

        binding.set_transient_callback_count(0);
        binding.handle_event_loop_callback();
        assert_eq!(
            *log.borrow(),
            ["animation", "idle"],
            "until nothing animates"
        );
    }

    #[test]
    fn a_starved_queue_still_answers_true_so_the_loop_comes_back() {
        // The subtle half of `handleEventLoopCallback`, and upstream's doc
        // says it in as many words: "Returns true otherwise, including when no
        // task is executed due to priority being too low."
        //
        // The caller re-arms on true. A starved task answering false would
        // stop the loop, and that task would still be waiting when the
        // animation ended -- with nothing left to wake it.
        let (mut binding, log) = recording();
        binding.set_transient_callback_count(1);
        push(&mut binding, &log, Priority::IDLE, "idle");

        assert!(
            binding.handle_event_loop_callback(),
            "there is work, just not yet"
        );
        assert!(log.borrow().is_empty());
        assert_eq!(binding.pending_tasks(), 1, "and it is still queued");
    }

    #[test]
    fn and_answers_false_only_where_the_loop_has_nothing_to_come_back_to() {
        // The two cases upstream names: empty, and locked.
        let mut binding = SchedulerBinding::new();
        assert!(!binding.handle_event_loop_callback(), "empty");

        let (mut binding, log) = recording();
        push(&mut binding, &log, Priority::TOUCH, "queued");
        binding.set_locked(true);
        assert!(!binding.handle_event_loop_callback(), "locked");
        assert_eq!(binding.pending_tasks(), 1, "and nothing was thrown away");
    }

    #[test]
    fn a_refused_head_holds_the_whole_queue_and_loses_none_of_it() {
        // Upstream consults only the head, and that is a **consequence of the
        // sort rather than a rule of its own**: the head is the highest
        // priority there is, so a refused head means every task is refused.
        // Rewriting `handleEventLoopCallback` to scan for the first acceptable
        // task changes nothing and no test can tell the difference -- a
        // mutation doing exactly that stayed green, which is the honest reason
        // this test is named for what it establishes rather than for the
        // implementation detail it cannot see.
        //
        // What it does establish: a refused head starves everything, the loop
        // keeps answering true, and nothing is dropped on the way.
        //
        // The three priorities are distinct on purpose. Equal ones come back
        // in scheduling order here because the sort is stable, and upstream's
        // heap does not promise that -- a test that leaned on it would be
        // asserting more than upstream says.
        let (mut binding, log) = recording();
        binding.set_transient_callback_count(1);
        push(&mut binding, &log, Priority::IDLE, "lowest");
        push(&mut binding, &log, Priority::TOUCH, "touch");
        push(&mut binding, &log, Priority::IDLE.raise(1), "low");

        assert!(binding.handle_event_loop_callback());
        assert_eq!(*log.borrow(), ["touch"], "the one above the line goes");

        for _ in 0..3 {
            assert!(binding.handle_event_loop_callback());
        }
        assert_eq!(*log.borrow(), ["touch"], "and the two below it wait");
        assert_eq!(binding.pending_tasks(), 2, "both still there");

        binding.set_transient_callback_count(0);
        while binding.pending_tasks() > 0 {
            binding.handle_event_loop_callback();
        }
        assert_eq!(*log.borrow(), ["touch", "low", "lowest"]);
    }

    #[test]
    fn a_relative_priority_can_cross_the_line_the_strategy_draws() {
        // `Priority` and the strategy are one design: `raise` moves a task by
        // at most `MAX_OFFSET`, and the named priorities sit ten times that
        // apart, so a single offset cannot lift idle work over the animation
        // line. What it *can* do is move a task that is already above it.
        let (mut binding, log) = recording();
        binding.set_transient_callback_count(1);
        push(
            &mut binding,
            &log,
            Priority::ANIMATION.lower(Priority::MAX_OFFSET),
            "just under animation",
        );
        binding.handle_event_loop_callback();
        assert!(
            log.borrow().is_empty(),
            "one step under the line is under it"
        );

        let (mut binding, log) = recording();
        binding.set_transient_callback_count(1);
        push(
            &mut binding,
            &log,
            Priority::IDLE.raise(999_999),
            "idle, raised",
        );
        binding.handle_event_loop_callback();
        assert!(
            log.borrow().is_empty(),
            "and the offset is clamped, so one hop cannot get idle work over it"
        );
    }
}
