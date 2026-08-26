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
    /// Upstream's `closeIconColor`, the first step of
    /// `widget.closeIconColor ?? snackBarTheme.closeIconColor ?? defaults`.
    /// The field was absent here, so neither the bar's own choice nor the
    /// theme's could be expressed.
    pub close_icon_color: Option<Color>,
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
            close_icon_color: None,
            action_overflow_threshold: None,
            action: None,
            persist: false,
        }
    }

    /// This bar's appearance, with the theme and the defaults folded in.
    ///
    /// Upstream does this inline in `_SnackBarState.build`; here it is a method
    /// so that the answer can be checked without building a frame, and so that
    /// [`SnackBar::check`] has something to check.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedSnackBar {
        crate::component_themes::ResolvedSnackBar::of(context, self)
    }

    /// Upstream's floating-only assert, run against the resolved behaviour.
    ///
    /// It has to be the *resolved* one and not `self.behavior`: a bar that set
    /// a width and left the behaviour alone is still wrong when the theme says
    /// fixed, and that is the case a developer has the hardest time seeing --
    /// which is why the message names where the behaviour came from.
    pub fn check(&self, context: &mut crate::framework::BuildContext) -> Result<(), String> {
        self.resolved(context).check(self.margin)
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

    /// Upstream's `closeIconColor`.
    pub fn with_close_icon_color(mut self, color: Color) -> Self {
        self.close_icon_color = Some(color);
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

#[cfg(test)]
mod snack_bar_theme_tests {
    use super::*;
    use crate::component_themes::{
        ResolvedSnackBar, SnackBarBehaviorSource, SnackBarTheme, SnackBarThemeData,
    };
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, provide};

    struct Reader {
        bar: std::cell::RefCell<Option<SnackBar>>,
        seen: std::rc::Rc<std::cell::RefCell<Option<(ResolvedSnackBar, Result<(), String>)>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let bar = self.bar.borrow_mut().take().expect("built once");
            let resolved = bar.resolved(context);
            let checked = bar.check(context);
            *self.seen.borrow_mut() = Some((resolved, checked));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(bar: SnackBar, data: SnackBarThemeData) -> (ResolvedSnackBar, Result<(), String>) {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            SnackBarTheme::new(
                data,
                component(Reader {
                    bar: std::cell::RefCell::new(Some(bar)),
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn a_bar_is_fixed_unless_something_says_otherwise() {
        let (resolved, _) = resolve(SnackBar::new(), SnackBarThemeData::new());
        assert_eq!(resolved.behavior, SnackBarBehavior::Fixed);
        assert_eq!(resolved.behavior_source, SnackBarBehaviorSource::Default);
    }

    #[test]
    fn the_behaviour_comes_from_the_widget_then_the_theme_then_the_default() {
        let mut floating = SnackBarThemeData::new();
        floating.behavior = Some(SnackBarBehavior::Floating);

        let (from_theme, _) = resolve(SnackBar::new(), floating.clone());
        assert_eq!(from_theme.behavior, SnackBarBehavior::Floating);
        assert_eq!(from_theme.behavior_source, SnackBarBehaviorSource::Theme);

        let mut fixed = SnackBar::new();
        fixed.behavior = Some(SnackBarBehavior::Fixed);
        let (from_widget, _) = resolve(fixed, floating);
        assert_eq!(from_widget.behavior, SnackBarBehavior::Fixed);
        assert_eq!(from_widget.behavior_source, SnackBarBehaviorSource::Widget);
    }

    #[test]
    fn width_and_margin_are_floating_only() {
        let mut wide = SnackBar::new();
        wide.width = Some(300.0);
        let (_, checked) = resolve(wide, SnackBarThemeData::new());
        assert!(checked.is_err(), "fixed by default, and a width was set");

        let mut floating = SnackBarThemeData::new();
        floating.behavior = Some(SnackBarBehavior::Floating);
        let mut wide = SnackBar::new();
        wide.width = Some(300.0);
        assert!(resolve(wide, floating).1.is_ok());
    }

    #[test]
    fn the_complaint_names_which_of_the_three_steps_chose_fixed() {
        // Told only that a width needs floating behaviour, a developer who
        // never wrote `behavior:` has nowhere to look.
        let mut wide = SnackBar::new();
        wide.width = Some(300.0);
        let message = resolve(wide, SnackBarThemeData::new()).1.unwrap_err();
        assert!(message.contains("by default"), "{message}");

        let mut fixed_theme = SnackBarThemeData::new();
        fixed_theme.behavior = Some(SnackBarBehavior::Fixed);
        let mut wide = SnackBar::new();
        wide.width = Some(300.0);
        let message = resolve(wide, fixed_theme).1.unwrap_err();
        assert!(message.contains("inherited SnackBarThemeData"), "{message}");

        let mut wide = SnackBar::new();
        wide.width = Some(300.0);
        wide.behavior = Some(SnackBarBehavior::Fixed);
        let message = resolve(wide, SnackBarThemeData::new()).1.unwrap_err();
        assert!(message.contains("in the SnackBar constructor"), "{message}");
    }

    #[test]
    fn a_margin_is_complained_about_by_name_and_so_is_a_width() {
        let mut with_margin = SnackBar::new();
        with_margin.margin = Some(8.0);
        assert!(
            resolve(with_margin, SnackBarThemeData::new())
                .1
                .unwrap_err()
                .starts_with("Margin")
        );

        let mut with_width = SnackBar::new();
        with_width.width = Some(8.0);
        assert!(
            resolve(with_width, SnackBarThemeData::new())
                .1
                .unwrap_err()
                .starts_with("Width")
        );
    }

    #[test]
    fn a_floating_bar_needs_less_padding_of_its_own() {
        // Its inset padding already holds it off the edges.
        assert_eq!(
            ResolvedSnackBar::horizontal_padding(SnackBarBehavior::Floating),
            16.0
        );
        assert_eq!(
            ResolvedSnackBar::horizontal_padding(SnackBarBehavior::Fixed),
            24.0
        );
    }

    #[test]
    fn the_action_moves_to_its_own_line_by_fraction_and_not_by_width() {
        // The bar's width is the screen's, and the same action is comfortable
        // on a tablet and crowded on a phone.
        let (resolved, _) = resolve(SnackBar::new(), SnackBarThemeData::new());
        assert_eq!(resolved.action_overflow_threshold, 0.25);

        assert!(
            !resolved.will_overflow_action(100.0, 800.0),
            "an eighth of a tablet"
        );
        assert!(
            resolved.will_overflow_action(100.0, 320.0),
            "the same action on a phone"
        );
    }

    #[test]
    fn a_threshold_of_exactly_the_share_does_not_overflow() {
        // Upstream's test is `>`, not `>=`.
        let (resolved, _) = resolve(SnackBar::new(), SnackBarThemeData::new());
        assert!(!resolved.will_overflow_action(100.0, 400.0), "exactly 0.25");
        assert!(resolved.will_overflow_action(101.0, 400.0));
    }

    #[test]
    fn a_bar_of_no_width_does_not_divide_by_it() {
        let (resolved, _) = resolve(SnackBar::new(), SnackBarThemeData::new());
        assert!(!resolved.will_overflow_action(100.0, 0.0));
    }

    #[test]
    fn a_width_set_on_both_sides_takes_the_bars() {
        // `bar.width.or(data.width)` -- with only one side set the direction is
        // invisible, which is how it went untested.
        let mut data = SnackBarThemeData::new();
        data.behavior = Some(SnackBarBehavior::Floating);
        data.width = Some(100.0);

        assert_eq!(resolve(SnackBar::new(), data.clone()).0.width, Some(100.0));

        let mut bar = SnackBar::new();
        bar.width = Some(400.0);
        assert_eq!(resolve(bar, data).0.width, Some(400.0));
    }

    #[test]
    fn the_widget_beats_the_theme_beats_the_default_for_the_rest_too() {
        let mut data = SnackBarThemeData::new();
        data.elevation = Some(3.0);
        data.show_close_icon = Some(true);
        data.action_overflow_threshold = Some(0.5);

        let (from_theme, _) = resolve(SnackBar::new(), data.clone());
        assert_eq!(from_theme.elevation, 3.0);
        assert!(from_theme.show_close_icon);
        assert_eq!(from_theme.action_overflow_threshold, 0.5);

        let mut bar = SnackBar::new();
        bar.elevation = Some(9.0);
        bar.show_close_icon = Some(false);
        bar.action_overflow_threshold = Some(0.75);
        let (from_widget, _) = resolve(bar, data);
        assert_eq!(from_widget.elevation, 9.0);
        assert!(!from_widget.show_close_icon);
        assert_eq!(from_widget.action_overflow_threshold, 0.75);
    }

    #[test]
    fn the_defaults_are_upstreams() {
        let (resolved, _) = resolve(SnackBar::new(), SnackBarThemeData::new());
        assert_eq!(resolved.elevation, 6.0);
        assert!(!resolved.show_close_icon);
        assert_eq!(
            resolved.inset_padding,
            crate::render::EdgeInsets {
                left: 15.0,
                top: 5.0,
                right: 15.0,
                bottom: 10.0,
            }
        );
        assert_eq!(
            resolved.background_color,
            crate::theme::ThemeData::fallback()
                .color_scheme
                .inverse_surface(),
            "a snack bar is the inverse surface: it is a message over the app, \
             not part of it"
        );
    }

    // -- Five colour chains, tick 230 ---------------------------------------
    //
    // `tools/unread_theme_fields.py` found four `SnackBarThemeData` colours
    // named nowhere outside their own paperwork, and reading upstream turned
    // up a fifth problem: `action_text_color` was carried, but only from the
    // theme -- neither the action's own colour nor upstream's default reached
    // anything -- and `SnackBar` had no `close_icon_color` field at all, so
    // the first step of that chain could not be expressed.
    //
    // Every level below uses a number no other level or chain uses, so a line
    // reading its neighbour's source, or its neighbour's field, answers with
    // a value that is not its own.

    fn ink(blue: u8) -> Color {
        Color::argb(255, 0, 0, blue)
    }

    fn action_with(
        text: Option<Color>,
        disabled_text: Option<Color>,
        background: Option<Color>,
        disabled_background: Option<Color>,
    ) -> SnackBarAction {
        let mut action = SnackBarAction::new("undo", || {});
        if let Some(color) = text {
            action = action.with_text_color(color);
        }
        if let Some(color) = disabled_text {
            action = action.with_disabled_text_color(color);
        }
        if let Some(color) = background {
            action = action.with_background_color(color);
        }
        if let Some(color) = disabled_background {
            action = action.with_disabled_background_color(color);
        }
        action
    }

    #[test]
    fn the_action_colours_prefer_the_action_then_the_theme_then_the_default() {
        let themed = SnackBarThemeData {
            action_text_color: Some(ink(10)),
            disabled_action_text_color: Some(ink(20)),
            action_background_color: Some(ink(30)),
            disabled_action_background_color: Some(ink(40)),
            ..SnackBarThemeData::new()
        };

        // The action's own colours win over the theme's.
        let (resolved, _) = resolve(
            SnackBar::new().with_action(action_with(
                Some(ink(50)),
                Some(ink(60)),
                Some(ink(70)),
                Some(ink(80)),
            )),
            themed.clone(),
        );
        assert_eq!(resolved.action_text_color, ink(50));
        assert_eq!(resolved.disabled_action_text_color, ink(60));
        assert_eq!(resolved.action_background_color, ink(70));
        assert_eq!(resolved.disabled_action_background_color, ink(80));

        // With none of its own, the theme's -- which is the half that reached
        // nothing for three of the four.
        let (resolved, _) = resolve(
            SnackBar::new().with_action(action_with(None, None, None, None)),
            themed,
        );
        assert_eq!(resolved.action_text_color, ink(10));
        assert_eq!(resolved.disabled_action_text_color, ink(20));
        assert_eq!(resolved.action_background_color, ink(30));
        assert_eq!(resolved.disabled_action_background_color, ink(40));
    }

    #[test]
    fn and_fall_back_to_upstreams_own_defaults() {
        // `_SnackbarDefaultsM3`: both label colours are `inversePrimary`, and
        // both backgrounds are transparent -- the action is a text button
        // until something says otherwise.
        let (resolved, _) = resolve(
            SnackBar::new().with_action(action_with(None, None, None, None)),
            SnackBarThemeData::new(),
        );
        let scheme = crate::theme::ThemeData::light().color_scheme;
        assert_eq!(resolved.action_text_color, scheme.inverse_primary());
        assert_eq!(
            resolved.disabled_action_text_color,
            scheme.inverse_primary()
        );
        assert_eq!(resolved.action_background_color, Color::TRANSPARENT);
        assert_eq!(
            resolved.disabled_action_background_color,
            Color::TRANSPARENT
        );
    }

    #[test]
    fn the_close_icon_colour_has_the_same_three_steps() {
        let themed = SnackBarThemeData {
            close_icon_color: Some(ink(90)),
            ..SnackBarThemeData::new()
        };
        let (resolved, _) = resolve(
            SnackBar::new().with_close_icon_color(ink(100)),
            themed.clone(),
        );
        assert_eq!(resolved.close_icon_color, ink(100), "the bar's own");

        let (resolved, _) = resolve(SnackBar::new(), themed);
        assert_eq!(resolved.close_icon_color, ink(90), "then the theme's");

        let (resolved, _) = resolve(SnackBar::new(), SnackBarThemeData::new());
        assert_eq!(
            resolved.close_icon_color,
            crate::theme::ThemeData::light()
                .color_scheme
                .on_inverse_surface(),
            "then upstream's default, the ink the content is written in"
        );
    }

    #[test]
    fn a_bar_with_no_action_still_resolves_its_action_colours() {
        // The action is optional, and the colours are not: upstream resolves
        // them whether or not there is anything to paint with them, and a
        // resolver that reached for a missing action would panic rather than
        // answer.
        let (resolved, _) = resolve(
            SnackBar::new(),
            SnackBarThemeData {
                action_text_color: Some(ink(110)),
                ..SnackBarThemeData::new()
            },
        );
        assert_eq!(resolved.action_text_color, ink(110));
    }
}
