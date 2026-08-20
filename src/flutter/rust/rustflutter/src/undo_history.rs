//! Undo and redo for anything with a value -- a port of upstream's
//! `widgets/undo_history.dart`.
//!
//! The hard part of an undo stack for a text field is not the stack. It is
//! **when to push**. A reader typing "hello" produces five value changes, and
//! five undo steps that each remove one letter is not what anybody means by
//! undo. So pushes are throttled: at most one every 500ms, and the one that
//! lands carries the **latest** value rather than the one that started the
//! window.
//!
//! That throttle is what makes the rest of the file subtle, because it opens a
//! window in which the value on screen is not yet in the stack. Two of the
//! decisions here exist only to cover it:
//!
//! * An undo while a push is pending **cancels the push and restores the last
//!   committed value** instead of stepping back through the stack. The reader
//!   is undoing what they just typed, and what they just typed was never
//!   recorded.
//! * An undo that arrives before the *first* value has been pushed does
//!   nothing at all, rather than cancelling that first push. Losing it would
//!   leave the field with no history to return to.
//!
//! ## What is not here
//!
//! The widget's `Actions` wiring, the platform `UndoManager` registration, and
//! the real timer are absent -- the throttle is driven by an explicit clock so
//! the window is testable. What is ported is the stack, the throttle's shape,
//! the push and undo rules, and the controller.

use crate::foundation::ValueNotifier;
use crate::services::system::UndoDirection;

/// Upstream `UndoHistoryValue`: whether the stack can go either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UndoHistoryValue {
    pub can_undo: bool,
    pub can_redo: bool,
}

impl UndoHistoryValue {
    /// Upstream's `UndoHistoryValue.empty`.
    pub const EMPTY: UndoHistoryValue = UndoHistoryValue {
        can_undo: false,
        can_redo: false,
    };

    pub fn new(can_undo: bool, can_redo: bool) -> UndoHistoryValue {
        UndoHistoryValue { can_undo, can_redo }
    }
}

impl std::fmt::Display for UndoHistoryValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "UndoHistoryValue(canUndo: {}, canRedo: {})",
            self.can_undo, self.can_redo
        )
    }
}

/// Upstream's private `_UndoStack`.
///
/// It is a list plus an index rather than two stacks, which is what makes the
/// redo tail a thing that can be truncated: pushing after an undo throws away
/// everything the reader had undone past, because they have now taken a
/// different branch and the old one is unreachable.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UndoStack<T> {
    list: Vec<T>,
    /// Where the reader is. `-1` when the list is empty.
    index: isize,
}

impl<T: Clone + PartialEq> UndoStack<T> {
    pub fn new() -> UndoStack<T> {
        UndoStack {
            list: Vec::new(),
            index: -1,
        }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn index(&self) -> isize {
        self.index
    }

    pub fn current_value(&self) -> Option<&T> {
        if self.list.is_empty() {
            return None;
        }
        self.list.get(self.index as usize)
    }

    pub fn can_undo(&self) -> bool {
        !self.list.is_empty() && self.index > 0
    }

    pub fn can_redo(&self) -> bool {
        !self.list.is_empty() && (self.index as usize) < self.list.len() - 1
    }

    /// Upstream's `push`. Pushing the value that is already current does
    /// nothing, so a value arriving twice does not become two undo steps.
    pub fn push(&mut self, value: T) {
        if self.list.is_empty() {
            self.index = 0;
            self.list.push(value);
            return;
        }
        if Some(&value) == self.current_value() {
            return;
        }
        // The reader undid some things and has now typed something else. The
        // branch they abandoned is unreachable from here, so it goes.
        if (self.index as usize) != self.list.len() - 1 {
            self.list.truncate(self.index as usize + 1);
        }
        self.list.push(value);
        self.index = self.list.len() as isize - 1;
    }

    /// Upstream's `undo`, which **returns the current value either way**: at
    /// the bottom of the stack it stays put and hands back what is already
    /// there rather than `None`. Undo does not fall off the end -- it stops,
    /// and the reader gets the oldest state they can reach.
    pub fn undo(&mut self) -> Option<T> {
        if self.list.is_empty() {
            return None;
        }
        if self.index != 0 {
            self.index -= 1;
        }
        self.current_value().cloned()
    }

    /// Upstream's `redo`, the same shape at the other end.
    pub fn redo(&mut self) -> Option<T> {
        if self.list.is_empty() {
            return None;
        }
        if (self.index as usize) < self.list.len() - 1 {
            self.index += 1;
        }
        self.current_value().cloned()
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.index = -1;
    }
}

/// Upstream's `_throttle`, given an explicit clock.
///
/// The shape is worth naming, because "throttle" is used for several different
/// things: this one **schedules on the leading edge and fires with the
/// trailing value**. The first call starts the window; every call inside the
/// window replaces the argument and returns the *same* timer; when the window
/// closes the function runs once, with the newest argument.
///
/// For typing that is exactly right. The window opens on the first keystroke,
/// so an undo step exists promptly, and it closes carrying the whole burst
/// rather than its first letter.
#[derive(Clone, Debug, PartialEq)]
pub struct Throttled<T> {
    duration_micros: i64,
    /// When the pending window closes, if one is open.
    deadline_micros: Option<i64>,
    pending: Option<T>,
}

impl<T: Clone> Throttled<T> {
    pub fn new(duration_micros: i64) -> Throttled<T> {
        Throttled {
            duration_micros,
            deadline_micros: None,
            pending: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.deadline_micros.is_some()
    }

    /// The value that would fire when the window closes.
    pub fn pending(&self) -> Option<&T> {
        self.pending.as_ref()
    }

    /// Upstream's returned closure: record the argument, and start the window
    /// only if one is not already running.
    pub fn call(&mut self, now_micros: i64, arg: T) {
        self.pending = Some(arg);
        if self.is_active() {
            return;
        }
        self.deadline_micros = Some(now_micros + self.duration_micros);
    }

    /// Advances the clock; returns the value to run with, if the window closed.
    pub fn advance(&mut self, now_micros: i64) -> Option<T> {
        let deadline = self.deadline_micros?;
        if now_micros < deadline {
            return None;
        }
        self.deadline_micros = None;
        self.pending.take()
    }

    /// Upstream's `_throttleTimer?.cancel()`.
    pub fn cancel(&mut self) {
        self.deadline_micros = None;
        self.pending = None;
    }
}

/// Upstream `UndoHistoryController`.
///
/// It is a `ValueNotifier` for the *state* of the stack plus two bare
/// `ChangeNotifier`s for the two verbs, and the split is deliberate: a button
/// listens to the value to know whether to be enabled, while the history
/// listens to the verbs to know when to act. One notifier could not tell those
/// two apart.
pub struct UndoHistoryController {
    pub value: ValueNotifier<UndoHistoryValue>,
    undo_requests: std::cell::Cell<usize>,
    redo_requests: std::cell::Cell<usize>,
}

impl Default for UndoHistoryController {
    fn default() -> UndoHistoryController {
        UndoHistoryController::new()
    }
}

impl UndoHistoryController {
    pub fn new() -> UndoHistoryController {
        UndoHistoryController {
            value: ValueNotifier::new(UndoHistoryValue::EMPTY),
            undo_requests: std::cell::Cell::new(0),
            redo_requests: std::cell::Cell::new(0),
        }
    }

    pub fn with_value(value: UndoHistoryValue) -> UndoHistoryController {
        UndoHistoryController {
            value: ValueNotifier::new(value),
            ..UndoHistoryController::new()
        }
    }

    pub fn undo_requests(&self) -> usize {
        self.undo_requests.get()
    }

    pub fn redo_requests(&self) -> usize {
        self.redo_requests.get()
    }

    /// Upstream's `undo`, which **checks `canUndo` before announcing**. A
    /// request nobody could satisfy is not passed on, so a listener never has
    /// to ask whether the request was real.
    pub fn undo(&self) {
        if !self.value.value().can_undo {
            return;
        }
        self.undo_requests.set(self.undo_requests.get() + 1);
    }

    pub fn redo(&self) {
        if !self.value.value().can_redo {
            return;
        }
        self.redo_requests.set(self.redo_requests.get() + 1);
    }
}

/// Upstream `UndoHistory`: the widget's configuration.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct UndoHistory {
    /// Upstream's `shouldChangeUndoStack`: a veto consulted with the previous
    /// and the new value, so a caller can say "a selection change is not an
    /// edit".
    pub has_should_change_filter: bool,
    /// Upstream's `undoStackModifier`, which rewrites what gets pushed. A text
    /// field uses it to drop the selection from the recorded value, so undoing
    /// restores the text and not the cursor.
    pub has_stack_modifier: bool,
}

/// Upstream `UndoHistoryState`, without the widget.
///
/// `T` is whatever is being tracked -- upstream's text field tracks a
/// `TextEditingValue`.
pub struct UndoHistoryState<T: Clone + PartialEq> {
    stack: UndoStack<T>,
    throttle: Throttled<T>,
    /// Upstream's `_lastValue`. It stops the same value being pushed twice in
    /// a row, which happens for real: `_push` runs both on init and again when
    /// the field takes focus.
    last_value: Option<T>,
    /// Upstream's `_duringTrigger`: true while the widget is being told to
    /// adopt an undone value.
    during_trigger: bool,
    pub controller: UndoHistoryController,
    /// What the widget was told to become, in order.
    triggered: Vec<T>,
    has_focus: bool,
}

impl<T: Clone + PartialEq> Default for UndoHistoryState<T> {
    fn default() -> UndoHistoryState<T> {
        UndoHistoryState::new()
    }
}

impl<T: Clone + PartialEq> UndoHistoryState<T> {
    /// Upstream's `_kThrottleDuration`, and its comment: "chosen as a best fit
    /// for the behavior of Mac, Linux, and Windows undo/redo state save
    /// durations, but it is not perfect for any of them". Three platforms
    /// disagree and one number has to serve all three.
    pub const THROTTLE_MICROS: i64 = 500_000;

    pub fn new() -> UndoHistoryState<T> {
        UndoHistoryState {
            stack: UndoStack::new(),
            throttle: Throttled::new(Self::THROTTLE_MICROS),
            last_value: None,
            during_trigger: false,
            controller: UndoHistoryController::new(),
            triggered: Vec::new(),
            has_focus: false,
        }
    }

    pub fn stack(&self) -> &UndoStack<T> {
        &self.stack
    }

    pub fn triggered(&self) -> &[T] {
        &self.triggered
    }

    pub fn can_undo(&self) -> bool {
        self.stack.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.stack.can_redo()
    }

    /// Upstream's `_push`, with its four early returns.
    ///
    /// `should_change` and `modifier` stand in for upstream's
    /// `shouldChangeUndoStack` and `undoStackModifier` callbacks. The modifier
    /// runs **before** the duplicate check against `_lastValue`, which is what
    /// makes a text field's use of it work: two values differing only in
    /// selection both modify down to the same text, and the second is then
    /// correctly recognised as nothing new.
    pub fn push(
        &mut self,
        now_micros: i64,
        value: T,
        should_change: Option<&dyn Fn(Option<&T>, &T) -> bool>,
        modifier: Option<&dyn Fn(&T) -> T>,
    ) {
        if self.last_value.as_ref() == Some(&value) {
            return;
        }
        // An undo is not an edit. See [`Self::echo_during_trigger`] for when
        // this fires and why the check above is not enough on its own.
        if self.during_trigger {
            return;
        }
        if let Some(should_change) = should_change {
            if !should_change(self.last_value.as_ref(), &value) {
                return;
            }
        }
        let next = match modifier {
            Some(modifier) => modifier(&value),
            None => value,
        };
        if self.last_value.as_ref() == Some(&next) {
            return;
        }
        self.last_value = Some(next.clone());
        self.throttle.call(now_micros, next);
    }

    /// The widget's own value listener firing **inside** `widget.onTriggered`
    /// -- which upstream's does, synchronously, every time an undo makes the
    /// widget adopt a value.
    ///
    /// Upstream's `_lastValue` check catches the ordinary case on its own:
    /// `_update` sets `_lastValue` to the value it is about to trigger, and a
    /// widget that adopts it exactly echoes back the same thing.
    ///
    /// `_duringTrigger` is the second line of defence, for a widget that
    /// adopts the value **approximately** -- a text field normalising the
    /// selection into the restored text, say. That echo does not equal
    /// `_lastValue`, so the first guard lets it through, and without this flag
    /// it would be pushed as a fresh undo step. Undo would then appear to do
    /// nothing, because the state it restored was immediately recorded as the
    /// newest one.
    pub fn echo_during_trigger(
        &mut self,
        now_micros: i64,
        value: T,
        should_change: Option<&dyn Fn(Option<&T>, &T) -> bool>,
        modifier: Option<&dyn Fn(&T) -> T>,
    ) {
        self.during_trigger = true;
        self.push(now_micros, value, should_change, modifier);
        self.during_trigger = false;
    }

    /// Runs the throttled push if its window has closed.
    pub fn advance(&mut self, now_micros: i64) {
        if let Some(value) = self.throttle.advance(now_micros) {
            self.stack.push(value);
            self.update_state();
        }
    }

    /// Upstream's `undo`.
    ///
    /// Two guards, and both are about the throttle window.
    ///
    /// The first: nothing in the stack yet means the **first** push is still
    /// pending, and cancelling it would leave the field with no history at
    /// all. So an undo that early does nothing.
    ///
    /// The second: a pending push means what is on screen was never recorded,
    /// so the undo **cancels the push and restores the last committed value**
    /// rather than stepping back. The reader is undoing what they just typed.
    pub fn undo(&mut self) {
        if self.stack.current_value().is_none() {
            return;
        }
        if self.throttle.is_active() {
            self.throttle.cancel();
            let current = self.stack.current_value().cloned();
            self.update(current);
        } else {
            let previous = self.stack.undo();
            self.update(previous);
        }
        self.update_state();
    }

    /// Upstream's `redo`.
    pub fn redo(&mut self) {
        let next = self.stack.redo();
        self.update(next);
        self.update_state();
    }

    /// Upstream's `handlePlatformUndo`, for the platform's own undo gesture --
    /// a three-finger swipe on iOS, or a shake.
    pub fn handle_platform_undo(&mut self, direction: UndoDirection) {
        match direction {
            UndoDirection::Undo => self.undo(),
            UndoDirection::Redo => self.redo(),
        }
    }

    /// Upstream's `_update`, which tells the widget to become `next`.
    fn update(&mut self, next: Option<T>) {
        let Some(next) = next else {
            return;
        };
        if self.last_value.as_ref() == Some(&next) {
            return;
        }
        self.last_value = Some(next.clone());
        self.during_trigger = true;
        self.triggered.push(next);
        self.during_trigger = false;
    }

    fn update_state(&self) {
        self.controller
            .value
            .set_value(UndoHistoryValue::new(self.can_undo(), self.can_redo()));
    }

    /// Upstream's `_handleFocus`: the field registers as the platform's undo
    /// client on focus and unregisters on blur, so the system undo gesture
    /// reaches whichever field the reader is actually in.
    pub fn handle_focus(&mut self, has_focus: bool) {
        self.has_focus = has_focus;
        if has_focus {
            self.update_state();
        }
    }

    pub fn is_platform_undo_client(&self) -> bool {
        self.has_focus
    }

    /// Upstream's `didUpdateWidget` when the tracked value changes identity:
    /// **the stack is cleared**. A different document has a different history,
    /// and keeping the old one would let an undo replace one document's text
    /// with another's.
    pub fn did_change_tracked_value(&mut self) {
        self.stack.clear();
        self.throttle.cancel();
        self.last_value = None;
        self.update_state();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: i64 = 1_000;
    const WINDOW: i64 = UndoHistoryState::<String>::THROTTLE_MICROS;

    fn state() -> UndoHistoryState<String> {
        UndoHistoryState::new()
    }

    fn push(state: &mut UndoHistoryState<String>, at: i64, value: &str) {
        state.push(at, value.to_string(), None, None);
    }

    /// Push a value and let its window close.
    fn commit(state: &mut UndoHistoryState<String>, at: i64, value: &str) -> i64 {
        push(state, at, value);
        let after = at + WINDOW;
        state.advance(after);
        after
    }

    // -- The stack ---------------------------------------------------------

    #[test]
    fn the_bottom_of_the_stack_is_where_undo_stops_rather_than_where_it_breaks() {
        // Upstream's undo returns the current value either way. It does not
        // fall off the end -- it stays, and hands back the oldest state the
        // reader can reach.
        let mut stack: UndoStack<&str> = UndoStack::new();
        stack.push("a");
        stack.push("b");

        assert_eq!(stack.undo(), Some("a"));
        assert!(!stack.can_undo());
        assert_eq!(stack.undo(), Some("a"), "still there");
        assert_eq!(stack.index(), 0);
    }

    #[test]
    fn an_empty_stack_has_nothing_to_hand_back() {
        let mut stack: UndoStack<&str> = UndoStack::new();
        assert_eq!(stack.undo(), None);
        assert_eq!(stack.redo(), None);
        assert_eq!(stack.current_value(), None);
        assert!(!stack.can_undo() && !stack.can_redo());
        assert_eq!(stack.index(), -1);
    }

    #[test]
    fn typing_after_an_undo_throws_away_the_branch_that_was_abandoned() {
        // The reader took a different path, and the old one is unreachable
        // from here.
        let mut stack: UndoStack<&str> = UndoStack::new();
        stack.push("a");
        stack.push("b");
        stack.push("c");
        stack.undo();
        stack.undo();
        assert_eq!(stack.current_value(), Some(&"a"));
        assert!(stack.can_redo());

        stack.push("d");
        assert!(!stack.can_redo(), "b and c are gone");
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.current_value(), Some(&"d"));
    }

    #[test]
    fn pushing_what_is_already_current_is_not_a_second_step() {
        let mut stack: UndoStack<&str> = UndoStack::new();
        stack.push("a");
        stack.push("a");
        assert_eq!(stack.len(), 1);
        assert!(!stack.can_undo());
    }

    #[test]
    fn redo_stops_at_the_top_the_same_way_undo_stops_at_the_bottom() {
        let mut stack: UndoStack<&str> = UndoStack::new();
        stack.push("a");
        stack.push("b");
        assert_eq!(stack.redo(), Some("b"), "already at the top");
        stack.undo();
        assert_eq!(stack.redo(), Some("b"));
        assert_eq!(stack.redo(), Some("b"));
    }

    // -- The throttle ------------------------------------------------------

    #[test]
    fn a_burst_of_typing_becomes_one_undo_step_carrying_the_last_letter() {
        // Five value changes for "hello" must not become five undo steps, and
        // the step that lands has to be the whole word rather than "h".
        let mut throttle: Throttled<&str> = Throttled::new(WINDOW);
        throttle.call(0, "h");
        throttle.call(10 * MS, "he");
        throttle.call(20 * MS, "hel");
        throttle.call(30 * MS, "hell");
        throttle.call(40 * MS, "hello");

        assert_eq!(throttle.advance(100 * MS), None, "window still open");
        assert_eq!(
            throttle.advance(WINDOW),
            Some("hello"),
            "the newest value, not the one that opened the window"
        );
        assert!(!throttle.is_active());
    }

    #[test]
    fn the_window_opens_on_the_first_call_and_not_on_the_last() {
        // Which is what makes an undo step exist promptly rather than 500ms
        // after the reader stops typing.
        let mut throttle: Throttled<&str> = Throttled::new(WINDOW);
        throttle.call(0, "a");
        throttle.call(400 * MS, "b");
        assert_eq!(
            throttle.advance(WINDOW),
            Some("b"),
            "the window closed 500ms after the first call, not the second"
        );
    }

    #[test]
    fn a_cancelled_window_takes_its_pending_value_with_it() {
        let mut throttle: Throttled<&str> = Throttled::new(WINDOW);
        throttle.call(0, "a");
        assert_eq!(throttle.pending(), Some(&"a"));
        throttle.cancel();
        assert!(!throttle.is_active());
        assert_eq!(throttle.advance(WINDOW), None);
    }

    // -- Undo across the throttle window -----------------------------------

    #[test]
    fn an_undo_before_the_first_push_lands_does_nothing_at_all() {
        // Cancelling that first push would leave the field with no history to
        // return to.
        let mut state = state();
        push(&mut state, 0, "hello");
        assert!(state.stack().is_empty());

        state.undo();
        assert!(state.triggered().is_empty(), "nothing was undone");

        state.advance(WINDOW);
        assert_eq!(
            state.stack().current_value().map(String::as_str),
            Some("hello"),
            "and the first push survived to land"
        );
    }

    #[test]
    fn an_undo_while_a_push_is_pending_restores_the_last_committed_value() {
        // What is on screen was never recorded, so stepping back through the
        // stack would skip a state the reader can see.
        let mut state = state();
        let at = commit(&mut state, 0, "hello");
        push(&mut state, at + MS, "hello world");

        state.undo();
        assert_eq!(
            state.triggered(),
            &["hello".to_string()],
            "back to what was committed, not one further"
        );

        state.advance(at + MS + WINDOW);
        assert_eq!(
            state.stack().len(),
            1,
            "and the cancelled push never landed"
        );
    }

    #[test]
    fn once_the_push_has_landed_undo_steps_through_the_stack_normally() {
        let mut state = state();
        let at = commit(&mut state, 0, "hello");
        commit(&mut state, at + MS, "hello world");
        assert_eq!(state.stack().len(), 2);
        assert!(state.can_undo());

        state.undo();
        assert_eq!(state.triggered().last().map(String::as_str), Some("hello"));
        assert!(!state.can_undo());
        assert!(state.can_redo());

        state.redo();
        assert_eq!(
            state.triggered().last().map(String::as_str),
            Some("hello world")
        );
    }

    #[test]
    fn the_value_an_undo_produces_is_not_pushed_straight_back() {
        // The widget adopts the old value, its listener fires, and that echo
        // must not become a new undo step -- or undo would appear to do
        // nothing, because the state it restored was immediately recorded as
        // the newest one.
        let mut state = state();
        let at = commit(&mut state, 0, "a");
        let at = commit(&mut state, at + MS, "b");

        state.undo();
        state.echo_during_trigger(at + MS, "a".to_string(), None, None);
        state.advance(at + WINDOW * 3);

        assert_eq!(state.stack().len(), 2, "unchanged by the undo");
        assert!(state.can_redo(), "so redo is still available");
    }

    #[test]
    fn a_widget_that_adopts_the_value_only_approximately_is_what_the_flag_catches() {
        // A text field normalising the selection into the restored text echoes
        // back something that is not quite what it was told, so the _lastValue
        // check lets it through. Without the flag it becomes a third undo step
        // and the reader's undo appears to do nothing.
        let mut state = state();
        let at = commit(&mut state, 0, "a");
        let at = commit(&mut state, at + MS, "b");
        assert_eq!(state.stack().len(), 2);

        state.undo();
        assert_eq!(state.triggered().last().map(String::as_str), Some("a"));

        // The widget adopted "a" but reports "a " back.
        state.echo_during_trigger(at + MS, "a ".to_string(), None, None);
        state.advance(at + WINDOW * 3);

        assert_eq!(state.stack().len(), 2, "still two");
        assert!(state.can_redo(), "and redo still reaches b");
    }

    #[test]
    fn the_same_value_arriving_twice_is_not_two_steps() {
        // _push runs both on init and again when the field takes focus.
        let mut state = state();
        push(&mut state, 0, "hello");
        push(&mut state, MS, "hello");
        state.advance(WINDOW);
        assert_eq!(state.stack().len(), 1);
    }

    // -- The two callbacks -------------------------------------------------

    #[test]
    fn a_veto_keeps_a_change_out_of_the_history_entirely() {
        // A caller uses this to say "a selection change is not an edit".
        let mut state = state();
        let only_longer = |previous: Option<&String>, next: &String| match previous {
            Some(previous) => next.len() > previous.len(),
            None => true,
        };
        state.push(0, "hello".to_string(), Some(&only_longer), None);
        state.advance(WINDOW);
        assert_eq!(state.stack().len(), 1);

        state.push(WINDOW, "hi".to_string(), Some(&only_longer), None);
        state.advance(WINDOW * 3);
        assert_eq!(state.stack().len(), 1, "vetoed");
    }

    #[test]
    fn the_modifier_runs_before_the_duplicate_check_and_that_is_what_makes_it_work() {
        // A text field uses it to drop the selection from the recorded value.
        // Two values differing only in selection modify down to the same text,
        // and the second is then correctly seen as nothing new -- which only
        // happens because the check is after the modifier, not before.
        let strip_after_hash =
            |value: &String| value.split('#').next().unwrap_or_default().to_string();
        let mut state = state();
        state.push(0, "hello#1".to_string(), None, Some(&strip_after_hash));
        state.advance(WINDOW);
        assert_eq!(
            state.stack().current_value().map(String::as_str),
            Some("hello"),
            "the selection was not recorded"
        );

        state.push(WINDOW, "hello#7".to_string(), None, Some(&strip_after_hash));
        state.advance(WINDOW * 3);
        assert_eq!(
            state.stack().len(),
            1,
            "moving the cursor is not an undo step"
        );
    }

    // -- The controller ----------------------------------------------------

    #[test]
    fn a_request_nobody_could_satisfy_is_not_passed_on() {
        // So a listener never has to ask whether the request was real.
        let controller = UndoHistoryController::new();
        controller.undo();
        controller.redo();
        assert_eq!(controller.undo_requests(), 0);
        assert_eq!(controller.redo_requests(), 0);

        controller
            .value
            .set_value(UndoHistoryValue::new(true, false));
        controller.undo();
        controller.redo();
        assert_eq!(controller.undo_requests(), 1);
        assert_eq!(controller.redo_requests(), 0, "still nothing to redo");
    }

    #[test]
    fn the_state_of_the_stack_and_the_two_verbs_are_separate_notifications() {
        // A button listens to the value to know whether to be enabled; the
        // history listens to the verbs to know when to act. One notifier could
        // not tell those apart.
        let mut state = state();
        assert_eq!(state.controller.value.value(), UndoHistoryValue::EMPTY);

        let at = commit(&mut state, 0, "a");
        assert_eq!(
            state.controller.value.value(),
            UndoHistoryValue::new(false, false),
            "one entry is not something to undo to"
        );

        commit(&mut state, at + MS, "b");
        assert_eq!(
            state.controller.value.value(),
            UndoHistoryValue::new(true, false)
        );

        state.undo();
        assert_eq!(
            state.controller.value.value(),
            UndoHistoryValue::new(false, true)
        );
    }

    #[test]
    fn an_empty_history_value_can_go_neither_way() {
        assert_eq!(UndoHistoryValue::EMPTY, UndoHistoryValue::default());
        assert!(!UndoHistoryValue::EMPTY.can_undo);
        assert_eq!(
            UndoHistoryValue::new(true, false).to_string(),
            "UndoHistoryValue(canUndo: true, canRedo: false)"
        );
    }

    // -- Focus and identity ------------------------------------------------

    #[test]
    fn the_platform_undo_gesture_follows_the_focused_field() {
        // A three-finger swipe should undo in whichever field the reader is
        // actually in.
        let mut state = state();
        assert!(!state.is_platform_undo_client());
        state.handle_focus(true);
        assert!(state.is_platform_undo_client());
        state.handle_focus(false);
        assert!(!state.is_platform_undo_client());
    }

    #[test]
    fn the_platform_asking_to_undo_goes_through_the_same_path() {
        let mut state = state();
        let at = commit(&mut state, 0, "a");
        commit(&mut state, at + MS, "b");

        state.handle_platform_undo(UndoDirection::Undo);
        assert_eq!(state.triggered().last().map(String::as_str), Some("a"));

        state.handle_platform_undo(UndoDirection::Redo);
        assert_eq!(state.triggered().last().map(String::as_str), Some("b"));
    }

    #[test]
    fn a_different_document_gets_a_different_history() {
        // Keeping the old one would let an undo replace one document's text
        // with another's.
        let mut state = state();
        let at = commit(&mut state, 0, "a");
        commit(&mut state, at + MS, "b");
        assert!(state.can_undo());

        state.did_change_tracked_value();
        assert!(state.stack().is_empty());
        assert!(!state.can_undo());
        assert_eq!(state.controller.value.value(), UndoHistoryValue::EMPTY);
    }
}
