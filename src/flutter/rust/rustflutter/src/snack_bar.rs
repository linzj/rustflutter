//! The bar across the bottom, and the one word on it a reader can press --
//! a port of upstream's `material/snack_bar.dart`.
//!
//! Whose queue this joins, and what closes it, is in
//! [`crate::scaffold_messenger`]. What is here is the bar itself: how long it
//! stays, when it stops going away on its own, and how it lays its action out
//! when the label is too long to sit beside the message.

use crate::animation::Curve;
use crate::component_themes::SnackBarBehavior;
use crate::engine::Color;
use crate::scaffold_messenger::SnackBarClosedReason;
use std::rc::Rc;

/// Upstream's `_snackBarDisplayDuration`: four seconds.
///
/// Long enough to read a sentence and short enough not to sit over the thing
/// the reader was looking at.
pub const SNACK_BAR_DISPLAY_DURATION_MICROS: i64 = 4_000_000;

/// Upstream's `_snackBarTransitionDuration`.
pub const SNACK_BAR_TRANSITION_DURATION_MICROS: i64 = 250_000;

/// Upstream's `_singleLineVerticalPadding`.
pub const SINGLE_LINE_VERTICAL_PADDING: f32 = 14.0;

/// Upstream's `_snackBarHeightCurve` and its Material 3 replacement.
pub fn snack_bar_height_curve(use_material3: bool) -> Curve {
    if use_material3 {
        Curve::EASE_IN_OUT_QUART
    } else {
        Curve::FAST_OUT_SLOW_IN
    }
}

/// Upstream `SnackBarAction`: the one thing on a snack bar a reader can press.
///
/// There is at most one, and upstream's state gives the reason: a bar is up
/// for four seconds, and a choice between two things is not a decision anyone
/// can make in four seconds.
pub struct SnackBarAction {
    pub label: String,
    pub text_color: Option<Color>,
    pub disabled_text_color: Option<Color>,
    pub background_color: Option<Color>,
    pub disabled_background_color: Option<Color>,
    on_pressed: Rc<dyn Fn()>,
    /// Upstream's `_haveTriggeredAction`.
    triggered: bool,
}

impl SnackBarAction {
    pub fn new(label: impl Into<String>, on_pressed: impl Fn() + 'static) -> SnackBarAction {
        SnackBarAction {
            label: label.into(),
            text_color: None,
            disabled_text_color: None,
            background_color: None,
            disabled_background_color: None,
            on_pressed: Rc::new(on_pressed),
            triggered: false,
        }
    }

    pub fn with_text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    pub fn with_disabled_text_color(mut self, color: Color) -> Self {
        self.disabled_text_color = Some(color);
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_disabled_background_color(mut self, color: Color) -> Self {
        self.disabled_background_color = Some(color);
        self
    }

    /// Upstream's `_haveTriggeredAction`, which is also what disables the
    /// button.
    pub fn has_triggered(&self) -> bool {
        self.triggered
    }

    /// Upstream's constructor assertion: a `WidgetStateColor` background
    /// already answers for every state, so a separate disabled colour beside
    /// it would be a second answer to the same question.
    ///
    /// Returns the message upstream would have asserted with, or `None`.
    pub fn validate(&self, background_is_state_color: bool) -> Option<&'static str> {
        if background_is_state_color && self.disabled_background_color.is_some() {
            return Some(
                "disabledBackgroundColor must not be provided when background color is a \
                 WidgetStateColor",
            );
        }
        None
    }

    /// Upstream's `_handlePressed`.
    ///
    /// **It fires once and then never again.** A reader who taps "Undo" twice
    /// while the bar is animating out would otherwise undo twice, and the
    /// second undo is one they never asked for. The guard comes before the
    /// callback, so even a callback that rebuilds cannot let a second press
    /// through.
    ///
    /// Returns the reason to close the bar with, or `None` if the press was
    /// the second one and did nothing.
    pub fn press(&mut self) -> Option<SnackBarClosedReason> {
        if self.triggered {
            return None;
        }
        self.triggered = true;
        (self.on_pressed)();
        Some(SnackBarClosedReason::Action)
    }
}

/// Upstream `SnackBar`: the bar itself.
pub struct SnackBar {
    /// How long it stays up before closing itself.
    pub duration_micros: i64,
    pub behavior: Option<SnackBarBehavior>,
    pub elevation: Option<f32>,
    /// Upstream's `margin`, which cannot be combined with `width`.
    pub margin: Option<f32>,
    pub width: Option<f32>,
    pub show_close_icon: Option<bool>,
    /// Upstream's `actionOverflowThreshold`: the fraction of the bar's width
    /// the action may take before it moves to its own line.
    pub action_overflow_threshold: Option<f32>,
    pub action: Option<SnackBarAction>,
    /// Upstream's `persist`.
    persist: bool,
}

impl Default for SnackBar {
    fn default() -> SnackBar {
        SnackBar::new()
    }
}

impl SnackBar {
    pub fn new() -> SnackBar {
        SnackBar {
            duration_micros: SNACK_BAR_DISPLAY_DURATION_MICROS,
            behavior: None,
            elevation: None,
            margin: None,
            width: None,
            show_close_icon: None,
            action_overflow_threshold: None,
            action: None,
            persist: false,
        }
    }

    /// Upstream's `action`, which also sets `persist`.
    ///
    /// **A bar with something to press does not go away on its own.**
    /// Upstream's default is `persist ?? action != null`: four seconds is
    /// enough to read a message and not enough to decide whether to undo
    /// something, so a bar that asks for a decision waits for one.
    pub fn with_action(mut self, action: SnackBarAction) -> Self {
        self.action = Some(action);
        self.persist = true;
        self
    }

    /// Upstream's explicit `persist`, which overrides the default either way.
    pub fn with_persist(mut self, persist: bool) -> Self {
        self.persist = persist;
        self
    }

    pub fn persists(&self) -> bool {
        self.persist
    }

    pub fn with_duration(mut self, micros: i64) -> Self {
        self.duration_micros = micros;
        self
    }

    pub fn with_behavior(mut self, behavior: SnackBarBehavior) -> Self {
        self.behavior = Some(behavior);
        self
    }

    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    pub fn with_margin(mut self, margin: f32) -> Self {
        self.margin = Some(margin);
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn with_action_overflow_threshold(mut self, threshold: f32) -> Self {
        self.action_overflow_threshold = Some(threshold);
        self
    }

    pub fn with_close_icon(mut self, show: bool) -> Self {
        self.show_close_icon = Some(show);
        self
    }

    /// Upstream's three constructor assertions, gathered.
    ///
    /// `width` and `margin` conflict because they are two ways of saying the
    /// same thing -- how far in from the edges the bar sits -- and a bar given
    /// both would have to ignore one.
    pub fn validate(&self) -> Option<&'static str> {
        if self.elevation.is_some_and(|elevation| elevation < 0.0) {
            return Some("elevation must not be negative");
        }
        if self.width.is_some() && self.margin.is_some() {
            return Some("Width and margin can not be used together");
        }
        if self
            .action_overflow_threshold
            .is_some_and(|threshold| !(0.0..=1.0).contains(&threshold))
        {
            return Some("Action overflow threshold must be between 0 and 1 inclusive");
        }
        None
    }

    /// Upstream's `willOverflowAction`: whether the action goes on its own
    /// line.
    ///
    /// The test is on the **fraction of the bar** the action and the close
    /// icon take, not on an absolute width -- a long label is only a problem
    /// relative to the space the message needs, and the same label is fine on
    /// a tablet and crowded on a phone.
    pub fn will_overflow_action(
        &self,
        action_and_icon_width: f32,
        snack_bar_width: f32,
        theme_threshold: Option<f32>,
        default_threshold: f32,
    ) -> bool {
        let threshold = self
            .action_overflow_threshold
            .or(theme_threshold)
            .unwrap_or(default_threshold);
        action_and_icon_width / snack_bar_width > threshold
    }

    /// Upstream's `withAnimation`.
    ///
    /// Upstream's `fallbackKey` matters more than it looks: two snack bars
    /// that happen to be built the same way would otherwise be matched as one
    /// widget, and the ink splash from a press on the *first* one's action
    /// would still be spreading on the second. A fresh key per bar keeps them
    /// separate.
    ///
    /// The animation is the messenger's, not the bar's -- one controller runs
    /// the whole queue, which is why a bar is handed one rather than making it.
    pub fn take_action(&mut self) -> Option<SnackBarAction> {
        self.action.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_bar_with_something_to_press_does_not_go_away_on_its_own() {
        // Four seconds is enough to read a message and not enough to decide
        // whether to undo something, so a bar that asks for a decision waits
        // for one.
        let plain = SnackBar::new();
        assert!(!plain.persists());

        let asking = SnackBar::new().with_action(SnackBarAction::new("Undo", || {}));
        assert!(asking.persists());
    }

    #[test]
    fn a_caller_may_say_otherwise_in_either_direction() {
        // Upstream's `persist ?? action != null` is a default, not a rule.
        let insistent = SnackBar::new().with_persist(true);
        assert!(insistent.persists(), "no action, but it waits anyway");

        let fleeting = SnackBar::new()
            .with_action(SnackBarAction::new("Undo", || {}))
            .with_persist(false);
        assert!(!fleeting.persists(), "an action that times out");
    }

    #[test]
    fn the_action_fires_once_and_then_never_again() {
        // A reader who taps Undo twice while the bar is animating out would
        // otherwise undo twice, and the second undo is one they never asked
        // for.
        let undone = Rc::new(Cell::new(0usize));
        let counter = undone.clone();
        let mut action = SnackBarAction::new("Undo", move || counter.set(counter.get() + 1));

        assert!(!action.has_triggered());
        assert_eq!(action.press(), Some(SnackBarClosedReason::Action));
        assert_eq!(undone.get(), 1);
        assert!(action.has_triggered());

        assert_eq!(action.press(), None, "the second press does nothing");
        assert_eq!(undone.get(), 1);
    }

    #[test]
    fn the_guard_is_set_before_the_callback_runs() {
        // So even a callback that rebuilds -- or that presses again -- cannot
        // let a second one through.
        let seen = Rc::new(Cell::new(false));
        let witness = seen.clone();
        let mut action = SnackBarAction::new("Undo", move || witness.set(true));
        action.press();
        assert!(seen.get(), "the callback did run");
        assert!(action.has_triggered());
    }

    #[test]
    fn pressing_the_action_is_what_closes_the_bar_and_says_why() {
        // Which is how an undo prompt tells "the reader undid it" from "the
        // reader let it stand".
        let mut action = SnackBarAction::new("Undo", || {});
        assert_eq!(action.press(), Some(SnackBarClosedReason::Action));
        assert_ne!(
            SnackBarClosedReason::Action,
            SnackBarClosedReason::Timeout,
            "and those are different answers"
        );
    }

    #[test]
    fn width_and_margin_are_two_ways_of_saying_the_same_thing() {
        // A bar given both would have to ignore one, so upstream refuses.
        assert_eq!(SnackBar::new().validate(), None);
        assert_eq!(SnackBar::new().with_width(400.0).validate(), None);
        assert_eq!(SnackBar::new().with_margin(16.0).validate(), None);
        assert_eq!(
            SnackBar::new()
                .with_width(400.0)
                .with_margin(16.0)
                .validate(),
            Some("Width and margin can not be used together")
        );
    }

    #[test]
    fn the_other_two_constructor_rules_hold_too() {
        assert_eq!(
            SnackBar::new().with_elevation(-1.0).validate(),
            Some("elevation must not be negative")
        );
        assert_eq!(SnackBar::new().with_elevation(0.0).validate(), None);
        assert_eq!(
            SnackBar::new()
                .with_action_overflow_threshold(1.5)
                .validate(),
            Some("Action overflow threshold must be between 0 and 1 inclusive")
        );
        assert_eq!(
            SnackBar::new()
                .with_action_overflow_threshold(-0.1)
                .validate(),
            Some("Action overflow threshold must be between 0 and 1 inclusive")
        );
        // The ends are inclusive.
        assert_eq!(
            SnackBar::new()
                .with_action_overflow_threshold(0.0)
                .validate(),
            None
        );
        assert_eq!(
            SnackBar::new()
                .with_action_overflow_threshold(1.0)
                .validate(),
            None
        );
    }

    #[test]
    fn a_state_coloured_background_already_answers_for_the_disabled_state() {
        // So a separate disabled colour beside it would be a second answer to
        // the same question.
        let plain = SnackBarAction::new("Undo", || {});
        assert_eq!(plain.validate(true), None);

        let both =
            SnackBarAction::new("Undo", || {}).with_disabled_background_color(Color(0xFF00_0000));
        assert!(both.validate(true).is_some());
        assert_eq!(
            both.validate(false),
            None,
            "and it is fine beside a plain colour"
        );
    }

    #[test]
    fn the_action_moves_to_its_own_line_by_fraction_and_not_by_width() {
        // The same label is fine on a tablet and crowded on a phone, so the
        // test is on the share of the bar the action takes.
        let bar = SnackBar::new();
        // A 120-pixel action on a 600-pixel bar is a fifth: comfortable.
        assert!(!bar.will_overflow_action(120.0, 600.0, None, 0.25));
        // The same action on a 360-pixel bar is a third: not.
        assert!(bar.will_overflow_action(120.0, 360.0, None, 0.25));
    }

    #[test]
    fn the_threshold_comes_from_the_bar_then_the_theme_then_the_default() {
        let default_only = SnackBar::new();
        assert!(!default_only.will_overflow_action(120.0, 400.0, None, 0.5));
        assert!(
            default_only.will_overflow_action(120.0, 400.0, Some(0.2), 0.5),
            "the theme is consulted before the default"
        );

        let insistent = SnackBar::new().with_action_overflow_threshold(0.9);
        assert!(
            !insistent.will_overflow_action(120.0, 400.0, Some(0.2), 0.5),
            "and the bar's own answer beats both"
        );
    }

    #[test]
    fn the_durations_and_curves_are_upstreams() {
        assert_eq!(SNACK_BAR_DISPLAY_DURATION_MICROS, 4_000_000);
        assert_eq!(SNACK_BAR_TRANSITION_DURATION_MICROS, 250_000);
        assert_eq!(SINGLE_LINE_VERTICAL_PADDING, 14.0);
        // Material 3 changed the height curve, and only that.
        assert_eq!(snack_bar_height_curve(false), Curve::FAST_OUT_SLOW_IN);
        assert_eq!(snack_bar_height_curve(true), Curve::EASE_IN_OUT_QUART);
        assert_ne!(snack_bar_height_curve(true), snack_bar_height_curve(false));
    }

    #[test]
    fn a_bar_carries_the_rest_of_what_it_was_given() {
        let bar = SnackBar::new()
            .with_duration(1_500_000)
            .with_behavior(SnackBarBehavior::Floating)
            .with_close_icon(true)
            .with_margin(8.0);
        assert_eq!(bar.duration_micros, 1_500_000);
        assert_eq!(bar.behavior, Some(SnackBarBehavior::Floating));
        assert_eq!(bar.show_close_icon, Some(true));
        assert_eq!(bar.margin, Some(8.0));
        assert_eq!(bar.validate(), None);

        // And the action can be taken out of it, which is what the messenger
        // does when it hands the bar on.
        let mut with_action = SnackBar::new().with_action(SnackBarAction::new("Retry", || {}));
        let taken = with_action.take_action().expect("there was one");
        assert_eq!(taken.label, "Retry");
        assert!(with_action.take_action().is_none());
        assert!(
            with_action.persists(),
            "and it still waits -- persist was decided when the action arrived"
        );
    }
}
