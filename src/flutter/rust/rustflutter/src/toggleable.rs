//! A port of `widgets/toggleable.dart`: `ToggleableStateMixin` and
//! `ToggleablePainter`.
//!
//! The machinery a checkbox, a switch and a radio all share. What they have in
//! common is not the shape but the *questions*: what a tap should do next, how
//! the control gets from one visual state to the next, and how the ink reaction
//! answers to three separate things at once -- the value, the pointer hovering,
//! and the keyboard focus.
//!
//! [`crate::radio_group::RawRadio`] is one of the mixin's users; upstream's
//! `_RawRadioState` mixes this in and answers `value` by asking the group.

use crate::borders::color_lerp;
use crate::engine::Color;

/// What [`ToggleableStateMixin::animate_to_value`] asked the position
/// controller to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionCommand {
    /// Run towards 1.0 -- the control filling in.
    Forward,
    /// Run back towards 0.0.
    Reverse,
    /// Snap to zero and then run forward again. Only a tristate control asks
    /// for this, and only for the indeterminate value.
    RestartForward,
}

/// Upstream `ToggleableStateMixin`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToggleableStateMixin {
    /// `None` is the indeterminate state, which only means something when
    /// [`tristate`](ToggleableStateMixin::tristate) is set.
    pub value: Option<bool>,
    pub tristate: bool,
    /// Upstream's `isInteractive`, which subclasses answer from whether they
    /// have an `onChanged`.
    pub interactive: bool,
    /// The position animation's current value: 0 is the control empty, 1 is
    /// filled.
    position: f32,
    reaction: f32,
    reaction_hover_fade: f32,
    reaction_focus_fade: f32,
    /// Where a pointer last went down, which is where the ink grows from.
    /// `None` when no pointer is down -- or when the control is not
    /// interactive, since there is nothing to react to.
    down_position: Option<(f32, f32)>,
    focused: bool,
    hovered: bool,
}

impl ToggleableStateMixin {
    pub fn new(value: Option<bool>) -> ToggleableStateMixin {
        ToggleableStateMixin {
            value,
            tristate: false,
            interactive: true,
            position: if value == Some(true) { 1.0 } else { 0.0 },
            reaction: 0.0,
            reaction_hover_fade: 0.0,
            reaction_focus_fade: 0.0,
            down_position: None,
            focused: false,
            hovered: false,
        }
    }

    pub fn tristate(mut self) -> Self {
        self.tristate = true;
        self
    }

    pub fn not_interactive(mut self) -> Self {
        self.interactive = false;
        self
    }

    pub fn position(&self) -> f32 {
        self.position
    }

    pub fn reaction(&self) -> f32 {
        self.reaction
    }

    pub fn reaction_hover_fade(&self) -> f32 {
        self.reaction_hover_fade
    }

    pub fn reaction_focus_fade(&self) -> f32 {
        self.reaction_focus_fade
    }

    pub fn down_position(&self) -> Option<(f32, f32)> {
        self.down_position
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    /// Upstream `animateToValue`.
    ///
    /// The tristate branch is worth reading twice, because it has a statement
    /// where an `else` would be expected:
    ///
    /// ```dart
    /// if (value == null) { _positionController.value = 0.0; }
    /// if (value ?? true) { _positionController.forward(); } else { ... }
    /// ```
    ///
    /// For `null`, **both** run: the position is snapped to zero and then
    /// animated forward again. So a tristate control going indeterminate does
    /// not settle somewhere between empty and full -- **it empties and refills**,
    /// which is how an indeterminate checkbox comes to look like a deliberate
    /// state rather than a half-finished one.
    ///
    /// A control that is not tristate reads `null` as false and empties.
    pub fn animate_to_value(&self) -> PositionCommand {
        if self.tristate {
            match self.value {
                None => PositionCommand::RestartForward,
                Some(true) => PositionCommand::Forward,
                Some(false) => PositionCommand::Reverse,
            }
        } else if self.value.unwrap_or(false) {
            PositionCommand::Forward
        } else {
            PositionCommand::Reverse
        }
    }

    /// Applies a command, as the animation controller would once it finished.
    pub fn settle(&mut self) {
        self.position = match self.animate_to_value() {
            PositionCommand::Forward | PositionCommand::RestartForward => 1.0,
            PositionCommand::Reverse => 0.0,
        };
    }

    /// Upstream `_handleTap`, and the cycle is the design.
    ///
    /// `false → true → (tristate ? null : false) → false`
    ///
    /// So a tristate control goes **off, on, indeterminate** rather than off,
    /// indeterminate, on. The indeterminate state is one the reader arrives
    /// at, not one they pass through on the way to switching something on.
    ///
    /// Returns the value the control asks its owner for, or `None` when it is
    /// not interactive -- a toggleable never changes its own value.
    pub fn handle_tap(&self) -> Option<Option<bool>> {
        if !self.interactive {
            return None;
        }
        Some(match self.value {
            Some(false) => Some(true),
            Some(true) => {
                if self.tristate {
                    None
                } else {
                    Some(false)
                }
            }
            None => Some(false),
        })
    }

    /// Upstream `_handleTapDown`, which records where the ink should grow from
    /// and starts it. A control that is not interactive records nothing.
    pub fn handle_tap_down(&mut self, local_position: (f32, f32)) {
        if !self.interactive {
            return;
        }
        self.down_position = Some(local_position);
        self.reaction = 1.0;
    }

    /// Upstream `_handleTapEnd`. The ink reverses whether or not there was a
    /// down position -- a cancelled press still has to fade out.
    pub fn handle_tap_end(&mut self) {
        self.down_position = None;
        self.reaction = 0.0;
    }

    /// Upstream `_handleFocusHighlightChanged`, which does nothing when the
    /// value did not change: a `setState` for the same answer is a rebuild for
    /// nothing.
    pub fn handle_focus_highlight_changed(&mut self, focused: bool) -> bool {
        if focused == self.focused {
            return false;
        }
        self.focused = focused;
        self.reaction_focus_fade = if focused { 1.0 } else { 0.0 };
        true
    }

    /// Upstream `_handleHoverChanged`, the same shape.
    pub fn handle_hover_changed(&mut self, hovered: bool) -> bool {
        if hovered == self.hovered {
            return false;
        }
        self.hovered = hovered;
        self.reaction_hover_fade = if hovered { 1.0 } else { 0.0 };
        true
    }
}

/// The colours a [`ToggleablePainter`] blends between.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToggleableColors {
    pub inactive_reaction_color: Color,
    pub reaction_color: Color,
    pub hover_color: Color,
    pub focus_color: Color,
}

/// Upstream `ToggleablePainter`.
///
/// A `ChangeNotifier` that paints one of these controls. Its animations are
/// assigned rather than owned -- the state mixin holds them and the painter
/// listens, which is what lets the same painter be handed a new set when the
/// widget is rebuilt without restarting anything.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToggleablePainter {
    pub colors: ToggleableColors,
    pub splash_radius: f32,
}

impl ToggleablePainter {
    pub fn new(colors: ToggleableColors, splash_radius: f32) -> ToggleablePainter {
        ToggleablePainter {
            colors,
            splash_radius,
        }
    }

    /// Upstream's triple-nested `Color.lerp` in `paintRadialReaction`, and the
    /// nesting is a precedence: **focus has the last word over hover, and hover
    /// over the control's own value.** The outermost blend is the one that
    /// wins, so a focused control looks focused whatever else is true of it.
    pub fn reaction_color(&self, state: &ToggleableStateMixin) -> Color {
        let by_value = color_lerp(
            self.colors.inactive_reaction_color,
            self.colors.reaction_color,
            state.position(),
        );
        let by_hover = color_lerp(
            by_value,
            self.colors.hover_color,
            state.reaction_hover_fade(),
        );
        color_lerp(
            by_hover,
            self.colors.focus_color,
            state.reaction_focus_fade(),
        )
    }

    /// The reaction's radius.
    ///
    /// A tap's ink **grows** from where the finger landed; a hover's or a
    /// focus ring's is simply **there**, at full size. The difference is that a
    /// tap has a point to grow from and the other two do not -- there is no
    /// place on the control that "being focused" happened at.
    pub fn reaction_radius(&self, state: &ToggleableStateMixin) -> f32 {
        if state.is_focused() || state.is_hovered() {
            self.splash_radius
        } else {
            self.splash_radius * state.reaction()
        }
    }

    /// Upstream's guard: nothing is painted while all three animations are
    /// dismissed, and nothing is drawn at a radius of zero either.
    pub fn paints_reaction(&self, state: &ToggleableStateMixin) -> bool {
        let anything_running = state.reaction() != 0.0
            || state.reaction_focus_fade() != 0.0
            || state.reaction_hover_fade() != 0.0;
        anything_running && self.reaction_radius(state) > 0.0
    }

    /// Where the ink is centred: the point the pointer went down at, or the
    /// control's own centre when the reaction came from focus or hover.
    pub fn reaction_origin(&self, state: &ToggleableStateMixin, centre: (f32, f32)) -> (f32, f32) {
        state.down_position().unwrap_or(centre)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFF: Color = Color(0x1000_0000);
    const ON: Color = Color(0x2000_0000);
    const HOVER: Color = Color(0x3000_0000);
    const FOCUS: Color = Color(0x4000_0000);

    fn painter() -> ToggleablePainter {
        ToggleablePainter::new(
            ToggleableColors {
                inactive_reaction_color: OFF,
                reaction_color: ON,
                hover_color: HOVER,
                focus_color: FOCUS,
            },
            20.0,
        )
    }

    // -- The tap cycle ---------------------------------------------------------

    #[test]
    fn a_plain_toggle_goes_back_and_forth() {
        let off = ToggleableStateMixin::new(Some(false));
        assert_eq!(off.handle_tap(), Some(Some(true)));

        let on = ToggleableStateMixin::new(Some(true));
        assert_eq!(on.handle_tap(), Some(Some(false)));
    }

    #[test]
    fn a_tristate_control_goes_off_then_on_then_indeterminate() {
        // Not off, indeterminate, on. The indeterminate state is one the reader
        // arrives at, not one they pass through on the way to switching
        // something on.
        let mut value = Some(false);
        let mut seen = vec![value];
        for _ in 0..3 {
            let state = ToggleableStateMixin {
                value,
                ..ToggleableStateMixin::new(value).tristate()
            };
            value = state.handle_tap().unwrap();
            seen.push(value);
        }
        assert_eq!(seen, [Some(false), Some(true), None, Some(false)]);
    }

    #[test]
    fn a_toggleable_never_changes_its_own_value() {
        // It asks; the owner decides. A non-interactive one does not even ask.
        let state = ToggleableStateMixin::new(Some(false)).not_interactive();
        assert_eq!(state.handle_tap(), None);
    }

    // -- Getting there ----------------------------------------------------------

    #[test]
    fn a_tristate_control_going_indeterminate_empties_and_refills() {
        // Upstream has a statement where an else would be expected, so for null
        // both run: the position is snapped to zero and animated forward again.
        // That is how an indeterminate checkbox looks like a deliberate state
        // rather than a half-finished one.
        let indeterminate = ToggleableStateMixin::new(None).tristate();
        assert_eq!(
            indeterminate.animate_to_value(),
            PositionCommand::RestartForward
        );

        let on = ToggleableStateMixin::new(Some(true)).tristate();
        assert_eq!(on.animate_to_value(), PositionCommand::Forward);

        let off = ToggleableStateMixin::new(Some(false)).tristate();
        assert_eq!(off.animate_to_value(), PositionCommand::Reverse);
    }

    #[test]
    fn a_control_that_is_not_tristate_reads_null_as_off() {
        let state = ToggleableStateMixin::new(None);
        assert_eq!(state.animate_to_value(), PositionCommand::Reverse);
        assert_ne!(
            state.animate_to_value(),
            ToggleableStateMixin::new(None)
                .tristate()
                .animate_to_value(),
            "which is the opposite of what a tristate control does with it"
        );
    }

    #[test]
    fn both_ways_of_arriving_at_full_end_at_full() {
        let mut indeterminate = ToggleableStateMixin::new(None).tristate();
        indeterminate.settle();
        assert_eq!(indeterminate.position(), 1.0);

        let mut on = ToggleableStateMixin::new(Some(true)).tristate();
        on.settle();
        assert_eq!(on.position(), 1.0);
    }

    // -- The ink ------------------------------------------------------------------

    #[test]
    fn the_ink_grows_from_where_the_finger_landed() {
        let mut state = ToggleableStateMixin::new(Some(false));
        assert_eq!(state.down_position(), None);

        state.handle_tap_down((4.0, 9.0));
        assert_eq!(state.down_position(), Some((4.0, 9.0)));
        assert_eq!(painter().reaction_origin(&state, (10.0, 10.0)), (4.0, 9.0));

        state.handle_tap_end();
        assert_eq!(state.down_position(), None);
        assert_eq!(
            painter().reaction_origin(&state, (10.0, 10.0)),
            (10.0, 10.0),
            "and with nothing down it comes from the middle"
        );
    }

    #[test]
    fn a_control_that_cannot_be_used_records_nothing_to_react_to() {
        let mut state = ToggleableStateMixin::new(Some(false)).not_interactive();
        state.handle_tap_down((4.0, 9.0));
        assert_eq!(state.down_position(), None);
        assert_eq!(state.reaction(), 0.0);
    }

    #[test]
    fn a_cancelled_press_still_fades_out() {
        // handle_tap_end reverses the ink whether or not anything was down.
        let mut state = ToggleableStateMixin::new(Some(false));
        state.handle_tap_down((4.0, 9.0));
        assert_eq!(state.reaction(), 1.0);
        state.handle_tap_end();
        assert_eq!(state.reaction(), 0.0);
    }

    #[test]
    fn a_taps_ink_grows_and_a_hovers_is_simply_there() {
        // A tap has a point to grow from; there is no place on a control that
        // "being focused" happened at.
        let painter = painter();
        let mut tapped = ToggleableStateMixin::new(Some(false));
        tapped.handle_tap_down((4.0, 9.0));
        tapped.reaction = 0.5;
        assert_eq!(painter.reaction_radius(&tapped), 10.0);

        let mut hovered = ToggleableStateMixin::new(Some(false));
        hovered.handle_hover_changed(true);
        assert_eq!(painter.reaction_radius(&hovered), 20.0, "full size at once");

        let mut focused = ToggleableStateMixin::new(Some(false));
        focused.handle_focus_highlight_changed(true);
        assert_eq!(painter.reaction_radius(&focused), 20.0);
    }

    #[test]
    fn nothing_is_painted_while_nothing_is_happening() {
        let painter = painter();
        let idle = ToggleableStateMixin::new(Some(false));
        assert!(!painter.paints_reaction(&idle));

        let mut touched = ToggleableStateMixin::new(Some(false));
        touched.handle_tap_down((4.0, 9.0));
        assert!(painter.paints_reaction(&touched));
    }

    #[test]
    fn focus_has_the_last_word_over_hover_and_hover_over_the_value() {
        // The nesting of upstream's three lerps is a precedence: the outermost
        // blend wins, so a focused control looks focused whatever else is true.
        let painter = painter();

        let mut on = ToggleableStateMixin::new(Some(true));
        on.settle();
        assert_eq!(painter.reaction_color(&on), ON);

        on.handle_hover_changed(true);
        assert_eq!(painter.reaction_color(&on), HOVER, "hover covers the value");

        on.handle_focus_highlight_changed(true);
        assert_eq!(
            painter.reaction_color(&on),
            FOCUS,
            "and focus covers the hover"
        );
    }

    #[test]
    fn an_untouched_control_shows_its_inactive_colour() {
        let painter = painter();
        let off = ToggleableStateMixin::new(Some(false));
        assert_eq!(painter.reaction_color(&off), OFF);
    }

    #[test]
    fn a_highlight_that_did_not_change_rebuilds_nothing() {
        // A setState for the same answer is a rebuild for nothing.
        let mut state = ToggleableStateMixin::new(Some(false));
        assert!(state.handle_focus_highlight_changed(true));
        assert!(!state.handle_focus_highlight_changed(true));
        assert!(state.handle_focus_highlight_changed(false));

        assert!(state.handle_hover_changed(true));
        assert!(!state.handle_hover_changed(true));
    }
}
