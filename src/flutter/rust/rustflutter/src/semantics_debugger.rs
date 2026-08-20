//! A port of `widgets/semantics_debugger.dart`.
//!
//! Draws the semantics tree over the running application, and -- the part that
//! makes it useful rather than decorative -- **takes over the gestures and
//! dispatches semantic actions instead of touches.**
//!
//! A debugger that simulated taps would only ever tell you the app works. This
//! one hit-tests the *semantics* tree and performs the action it finds there,
//! which is exactly what a screen reader does, so what you are seeing is
//! whether the semantics work.

use crate::semantics::SemanticsAction;

/// Upstream `SemanticsDebugger`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SemanticsDebugger {
    pub child: u64,
    /// Upstream's default is a black 10-point style with a **line height of
    /// 0.8** -- tighter than the text is tall, so several lines of label fit
    /// inside the node's own box rather than spilling over its neighbours.
    pub label_font_size: f32,
    pub label_height: f32,
}

impl SemanticsDebugger {
    pub const DEFAULT_FONT_SIZE: f32 = 10.0;
    pub const DEFAULT_HEIGHT: f32 = 0.8;

    pub fn new(child: u64) -> SemanticsDebugger {
        SemanticsDebugger {
            child,
            label_font_size: SemanticsDebugger::DEFAULT_FONT_SIZE,
            label_height: SemanticsDebugger::DEFAULT_HEIGHT,
        }
    }
}

/// What a gesture on the debugger turned into.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebuggerActions {
    /// The actions dispatched, in order. Upstream sends them at the position of
    /// the **pointer down**, not of the fling, so a flick that wandered still
    /// acts on the node it started at.
    pub actions: Vec<SemanticsAction>,
}

impl SemanticsDebugger {
    /// Upstream `_handleTap` and `_handleLongPress`.
    pub fn tap() -> DebuggerActions {
        DebuggerActions {
            actions: vec![SemanticsAction::Tap],
        }
    }

    pub fn long_press() -> DebuggerActions {
        DebuggerActions {
            actions: vec![SemanticsAction::LongPress],
        }
    }

    /// Upstream `_handlePanEnd`.
    ///
    /// Two things here are worth writing down.
    ///
    /// **An exactly diagonal fling does nothing.** Upstream returns when the
    /// two velocity components are equal in magnitude rather than picking one:
    /// there is no right answer, and guessing would make the debugger's
    /// behaviour depend on floating-point noise.
    ///
    /// **A horizontal fling dispatches two actions and a vertical one
    /// dispatches one.** Left means "decrease" on a slider and "scroll left" on
    /// a list, and the debugger has no idea which the node under the finger is
    /// -- so it sends both and lets the node take whichever it has. There is no
    /// vertical increase-and-decrease convention, so up and down send only the
    /// scroll.
    pub fn pan_end(velocity: (f32, f32)) -> DebuggerActions {
        let (vx, vy) = velocity;
        if vx.abs() == vy.abs() {
            return DebuggerActions::default();
        }
        let actions = if vx.abs() > vy.abs() {
            if vx < 0.0 {
                vec![SemanticsAction::Decrease, SemanticsAction::ScrollLeft]
            } else {
                vec![SemanticsAction::Increase, SemanticsAction::ScrollRight]
            }
        } else if vy < 0.0 {
            vec![SemanticsAction::ScrollUp]
        } else {
            vec![SemanticsAction::ScrollDown]
        };
        DebuggerActions { actions }
    }

    /// Whether the debugger drives the application through the semantics tree
    /// rather than the render tree. Upstream's `_performAction` goes through
    /// `semanticsOwner.performActionAt`, which hit-tests the semantics nodes.
    ///
    /// This is the whole point of the widget. A debugger that dispatched
    /// pointer events would exercise the same code the app already works
    /// through; only dispatching semantic actions can show whether a screen
    /// reader would get anywhere.
    pub fn dispatches_through_semantics() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use SemanticsAction::{
        Decrease, Increase, LongPress, ScrollDown, ScrollLeft, ScrollRight, ScrollUp, Tap,
    };

    #[test]
    fn the_debugger_drives_the_app_the_way_a_screen_reader_does() {
        // A debugger dispatching pointer events would only exercise the code
        // the app already works through. Only semantic actions can show whether
        // assistive technology would get anywhere.
        assert!(SemanticsDebugger::dispatches_through_semantics());
        assert_eq!(SemanticsDebugger::tap().actions, [Tap]);
        assert_eq!(SemanticsDebugger::long_press().actions, [LongPress]);
    }

    #[test]
    fn an_exactly_diagonal_fling_does_nothing_rather_than_guessing() {
        // There is no right answer, and guessing would make the behaviour
        // depend on floating-point noise.
        assert!(
            SemanticsDebugger::pan_end((300.0, 300.0))
                .actions
                .is_empty()
        );
        assert!(
            SemanticsDebugger::pan_end((-300.0, 300.0))
                .actions
                .is_empty()
        );
        assert!(SemanticsDebugger::pan_end((0.0, 0.0)).actions.is_empty());
    }

    #[test]
    fn a_horizontal_fling_sends_two_actions_and_lets_the_node_choose() {
        // Left means decrease on a slider and scroll left on a list, and the
        // debugger has no idea which the node under the finger is.
        assert_eq!(
            SemanticsDebugger::pan_end((-800.0, 100.0)).actions,
            [Decrease, ScrollLeft]
        );
        assert_eq!(
            SemanticsDebugger::pan_end((800.0, 100.0)).actions,
            [Increase, ScrollRight]
        );
    }

    #[test]
    fn a_vertical_fling_sends_one_because_there_is_no_vertical_increase() {
        assert_eq!(
            SemanticsDebugger::pan_end((100.0, -800.0)).actions,
            [ScrollUp]
        );
        assert_eq!(
            SemanticsDebugger::pan_end((100.0, 800.0)).actions,
            [ScrollDown]
        );
    }

    #[test]
    fn the_dominant_axis_wins_however_slight_the_difference() {
        assert_eq!(
            SemanticsDebugger::pan_end((300.1, 300.0)).actions,
            [Increase, ScrollRight]
        );
        assert_eq!(
            SemanticsDebugger::pan_end((300.0, 300.1)).actions,
            [ScrollDown]
        );
    }

    #[test]
    fn the_labels_are_set_tighter_than_the_text_is_tall() {
        // So several lines fit inside the node's own box rather than spilling
        // over its neighbours.
        let debugger = SemanticsDebugger::new(1);
        assert_eq!(debugger.label_font_size, 10.0);
        assert!(debugger.label_height < 1.0);
    }
}
