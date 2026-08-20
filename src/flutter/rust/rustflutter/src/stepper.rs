//! Ports of `material/stepper.dart` and `material/toggle_buttons.dart`.
//!
//! Two widgets built out of a list of children and a parallel list of state,
//! where the interesting parts are what each refuses to accept.

use crate::render::Axis;

/// Upstream `_kStepSize`, the default diameter of a step's circle.
pub const STEP_SIZE: f32 = 24.0;

/// Upstream `_kMaxStepSize`.
pub const MAX_STEP_SIZE: f32 = 80.0;

/// Upstream `StepState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StepState {
    /// Shows its index in the circle.
    #[default]
    Indexed,
    /// Shows a pencil.
    Editing,
    /// Shows a tick.
    Complete,
    /// Upstream: *"A step that is disabled and does not to react to taps."*
    Disabled,
    /// Shows a triangle.
    Error,
}

impl StepState {
    /// Only one of the five refuses taps.
    pub fn reacts_to_taps(self) -> bool {
        !matches!(self, StepState::Disabled)
    }
}

/// Upstream `StepperType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StepperType {
    /// Content between the titles.
    #[default]
    Vertical,
    /// Content below the titles.
    Horizontal,
}

/// Upstream `ControlsDetails`: everything a `controlsBuilder` needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlsDetails {
    /// Upstream: *"This may be different from `stepIndex` if the user has just
    /// changed steps and we are currently animating toward that step."*
    pub current_step: usize,
    /// Upstream: *"This is not necessarily the active index, if the user has
    /// just changed steps and this step is animating away."*
    pub step_index: usize,
    pub has_on_step_continue: bool,
    pub has_on_step_cancel: bool,
}

impl ControlsDetails {
    pub fn new(current_step: usize, step_index: usize) -> ControlsDetails {
        ControlsDetails {
            current_step,
            step_index,
            has_on_step_continue: true,
            has_on_step_cancel: true,
        }
    }

    /// Upstream `isActive`.
    ///
    /// The reason two indices exist at all: **while the stepper animates,
    /// both steps are on screen**, and the builder runs for each of them. One
    /// index says where the stepper is going, the other says which of the two
    /// this call is building. Same shape as the tab controller's two ways of
    /// moving -- during a transition there are two of something, and the code
    /// has to be able to name both.
    ///
    /// Note this is *not* the same `isActive` as [`Step::is_active`], which is
    /// a styling flag the caller sets. This one is derived.
    pub fn is_active(&self) -> bool {
        self.current_step == self.step_index
    }
}

/// Upstream `Step`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Step {
    pub state: StepState,
    /// Upstream: *"Whether or not the step is active. **The flag only
    /// influences styling.**"* A different thing from
    /// [`ControlsDetails::is_active`], which is computed.
    pub is_active: bool,
    pub has_subtitle: bool,
    pub has_label: bool,
    pub has_step_style: bool,
}

/// Upstream `StepStyle`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StepStyle {
    pub color: Option<u32>,
    pub error_color: Option<u32>,
    /// Upstream: *"This property only applies when `Stepper.type` is
    /// `StepperType.horizontal`."*
    pub connector_color: Option<u32>,
    pub connector_thickness: Option<f32>,
}

impl StepStyle {
    pub fn new() -> StepStyle {
        StepStyle::default()
    }

    /// Whether any of this style's connector settings will be looked at, given
    /// the stepper's axis.
    pub fn connector_applies(&self, stepper_type: StepperType) -> bool {
        matches!(stepper_type, StepperType::Horizontal)
    }
}

/// Why a stepper's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepperError {
    CurrentStepOutOfRange,
    IconHeightOutOfRange,
    IconWidthOutOfRange,
    IconNotSquare,
}

/// Upstream `Stepper`.
#[derive(Clone, Debug, PartialEq)]
pub struct Stepper {
    pub steps: Vec<Step>,
    pub current_step: usize,
    pub stepper_type: StepperType,
    pub step_icon_height: Option<f32>,
    pub step_icon_width: Option<f32>,
}

impl Stepper {
    pub fn new(steps: Vec<Step>, current_step: usize) -> Stepper {
        Stepper {
            steps,
            current_step,
            stepper_type: StepperType::Vertical,
            step_icon_height: None,
            step_icon_width: None,
        }
    }

    /// Upstream's constructor asserts.
    ///
    /// The square one carries a message that does not match its own condition:
    ///
    /// ```dart
    /// assert(
    ///   stepIconHeight == null || stepIconWidth == null || stepIconHeight == stepIconWidth,
    ///   'If either stepIconHeight or stepIconWidth is specified, both must be specified and '
    ///   'the values must be equal.',
    /// );
    /// ```
    ///
    /// **The message says both must be specified. The condition says no such
    /// thing** -- give a height and leave the width null and the second term
    /// short-circuits the check away. So the half-specified case the message
    /// calls an error is in fact allowed, and it falls back to `_kStepSize` for
    /// the other axis.
    ///
    /// The range messages say *"must be greater than 24.0"* where the code is
    /// `>= _kStepSize`, so 24.0 exactly is permitted too.
    ///
    /// Ported as the conditions behave. This is the second doc-versus-assert
    /// disagreement in as many files: `TabController`'s constructor has the same
    /// shape.
    pub fn validate(&self) -> Result<(), StepperError> {
        if self.current_step >= self.steps.len() {
            return Err(StepperError::CurrentStepOutOfRange);
        }
        if let Some(height) = self.step_icon_height {
            if !(STEP_SIZE..=MAX_STEP_SIZE).contains(&height) {
                return Err(StepperError::IconHeightOutOfRange);
            }
        }
        if let Some(width) = self.step_icon_width {
            if !(STEP_SIZE..=MAX_STEP_SIZE).contains(&width) {
                return Err(StepperError::IconWidthOutOfRange);
            }
        }
        if let (Some(height), Some(width)) = (self.step_icon_height, self.step_icon_width) {
            if height != width {
                return Err(StepperError::IconNotSquare);
            }
        }
        Ok(())
    }

    /// The size actually painted, with either axis falling back to `_kStepSize`.
    pub fn icon_size(&self) -> (f32, f32) {
        (
            self.step_icon_width.unwrap_or(STEP_SIZE),
            self.step_icon_height.unwrap_or(STEP_SIZE),
        )
    }

    /// Upstream's `didUpdateWidget` opens with
    /// `assert(widget.steps.length == oldWidget.steps.length)`, and the doc for
    /// `steps` says plainly: *"The length of `steps` must not change."*
    ///
    /// **The list's length is part of the widget's identity**, which is unusual
    /// -- most widgets take a rebuilt list of any length. The reason is the line
    /// underneath: the state walks the old steps by index and stores each one's
    /// previous state, so it can animate a circle from `indexed` to `complete`.
    /// A list that changed length would pair those up wrong, and every step past
    /// the edit would animate from somebody else's past.
    pub fn accepts_update(&self, updated: &Stepper) -> bool {
        self.steps.len() == updated.steps.len()
    }

    /// Upstream's `didUpdateWidget` body: remember what each step used to be.
    pub fn old_states(&self) -> Vec<StepState> {
        self.steps.iter().map(|step| step.state).collect()
    }

    /// Upstream's `build` throws a `FlutterError` if it finds a `Stepper`
    /// ancestor: *"Steppers must not be nested. The material specification
    /// advises that one should avoid embedding steppers within steppers."*
    ///
    /// Worth noticing what kind of rule that is. Nothing here would break --
    /// this is not a layout constraint or an invariant the code depends on. It
    /// is a **design guideline, enforced by the framework as an error**, with a
    /// link to the spec in the message.
    pub fn may_nest_inside_a_stepper() -> bool {
        false
    }
}

/// Upstream `ToggleButtons`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToggleButtons {
    pub child_count: usize,
    /// Upstream: *"They are both correlated by their index in the list."*
    pub is_selected: Vec<bool>,
    pub focus_node_count: Option<usize>,
    pub direction: Axis,
    /// Upstream's `tapTargetSize` padding: the buttons are padded out so the
    /// touchable area reaches the minimum, which is what makes
    /// [`ToggleButtons::hit_test_point`] necessary.
    pub tap_target_extent: f32,
    pub button_extent: f32,
}

impl ToggleButtons {
    pub fn new(is_selected: Vec<bool>) -> ToggleButtons {
        ToggleButtons {
            child_count: is_selected.len(),
            is_selected,
            focus_node_count: None,
            direction: Axis::Horizontal,
            tap_target_extent: 48.0,
            button_extent: 32.0,
        }
    }

    /// Upstream `assert(children.length == isSelected.length)`.
    pub fn lengths_match(&self) -> bool {
        self.child_count == self.is_selected.len()
    }

    /// Upstream's build-time assert, whose message is
    /// *"focusNodes.length must match children.length."*
    ///
    /// The condition returns `true` outright when `focusNodes` is null, while
    /// the message interpolates `focusNodes!.length` unguarded. Dart only builds
    /// an assert's message when the assert fails, and the condition guarantees
    /// non-null in exactly that case -- so it is correct, by one hair.
    pub fn focus_nodes_match(&self) -> bool {
        self.focus_node_count
            .is_none_or(|count| count == self.child_count)
    }

    /// Upstream's `hitTest` override, returning the point the hit is forwarded
    /// to.
    ///
    /// It deliberately does not call `super.hitTest()`, and pins the **cross
    /// axis** coordinate to the child's centre while leaving the main axis
    /// alone. Upstream's comment: *"Only adjust one axis to ensure the correct
    /// button is tapped."*
    ///
    /// This is the third widget this week whose touchable area is larger than
    /// what it draws, and the only one that expands on **one axis only**. It has
    /// to: the buttons are padded vertically to reach the tap target size, so a
    /// tap above a button must still count as that button -- but the buttons sit
    /// shoulder to shoulder along the row, and collapsing the main axis too
    /// would make every tap land on whichever neighbour got asked first.
    ///
    /// **Expanding both axes is what you want for a lone control and exactly
    /// wrong for a row of them.** The scrollbar and the icon button could take
    /// the easy version; this one could not.
    pub fn hit_test_point(&self, position: (f32, f32), child_size: (f32, f32)) -> (f32, f32) {
        match self.direction {
            Axis::Horizontal => (position.0, child_size.1 / 2.0),
            Axis::Vertical => (child_size.0 / 2.0, position.1),
        }
    }

    /// Whether a hit at this point is inside the padded region at all. Upstream
    /// checks `size.contains(position)` first and returns false otherwise,
    /// because skipping `super.hitTest` skipped that check too.
    pub fn contains(&self, position: (f32, f32), size: (f32, f32)) -> bool {
        position.0 >= 0.0 && position.0 < size.0 && position.1 >= 0.0 && position.1 < size.1
    }

    /// How much taller the touchable strip is than the button drawn in it.
    pub fn padding_each_side(&self) -> f32 {
        ((self.tap_target_extent - self.button_extent) / 2.0).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(count: usize) -> Vec<Step> {
        vec![Step::default(); count]
    }

    // -- The assert with the hole in it --------------------------------------------

    #[test]
    fn half_specifying_the_icon_size_is_allowed_despite_the_message_saying_otherwise() {
        // 'If either stepIconHeight or stepIconWidth is specified, both must be
        // specified and the values must be equal.' The condition
        // `h == null || w == null || h == w` never checks that.
        let mut stepper = Stepper::new(steps(3), 0);
        stepper.step_icon_height = Some(40.0);
        assert_eq!(stepper.validate(), Ok(()));
    }

    #[test]
    fn and_the_case_it_lets_through_produces_the_very_shape_it_is_guarding_against() {
        // The other axis falls back to _kStepSize, so the icon the assert exists
        // to keep square comes out 24 by 40.
        let mut stepper = Stepper::new(steps(3), 0);
        stepper.step_icon_height = Some(40.0);
        let (width, height) = stepper.icon_size();
        assert_eq!((width, height), (24.0, 40.0));
        assert_ne!(width, height, "which is exactly what IconNotSquare forbids");

        // Say the same thing out loud and it is refused.
        stepper.step_icon_width = Some(24.0);
        assert_eq!(stepper.validate(), Err(StepperError::IconNotSquare));
    }

    #[test]
    fn two_equal_sizes_are_fine_and_two_different_ones_are_not() {
        let mut stepper = Stepper::new(steps(3), 0);
        stepper.step_icon_height = Some(40.0);
        stepper.step_icon_width = Some(40.0);
        assert_eq!(stepper.validate(), Ok(()));

        stepper.step_icon_width = Some(41.0);
        assert_eq!(stepper.validate(), Err(StepperError::IconNotSquare));
    }

    #[test]
    fn the_range_message_says_greater_than_where_the_code_says_at_least() {
        let mut stepper = Stepper::new(steps(3), 0);
        stepper.step_icon_height = Some(STEP_SIZE);
        assert_eq!(stepper.validate(), Ok(()), "24.0 exactly is permitted");

        stepper.step_icon_height = Some(STEP_SIZE - 0.5);
        assert_eq!(
            stepper.validate(),
            Err(StepperError::IconHeightOutOfRange),
            "and anything under it is not"
        );

        stepper.step_icon_height = Some(MAX_STEP_SIZE);
        assert_eq!(stepper.validate(), Ok(()));
        stepper.step_icon_height = Some(MAX_STEP_SIZE + 0.5);
        assert_eq!(stepper.validate(), Err(StepperError::IconHeightOutOfRange));
    }

    #[test]
    fn a_stepper_must_be_somewhere_in_its_own_list() {
        assert_eq!(Stepper::new(steps(3), 2).validate(), Ok(()));
        assert_eq!(
            Stepper::new(steps(3), 3).validate(),
            Err(StepperError::CurrentStepOutOfRange)
        );
        assert_eq!(
            Stepper::new(steps(0), 0).validate(),
            Err(StepperError::CurrentStepOutOfRange),
            "unlike a tab controller, an empty stepper has no valid index at all"
        );
    }

    // -- A length that is part of the identity -------------------------------------

    #[test]
    fn the_list_may_be_rebuilt_but_not_resized() {
        let before = Stepper::new(steps(4), 0);
        assert!(before.accepts_update(&Stepper::new(steps(4), 1)));
        assert!(!before.accepts_update(&Stepper::new(steps(5), 1)));
        assert!(!before.accepts_update(&Stepper::new(steps(3), 1)));
    }

    #[test]
    fn the_remembered_states_line_up_with_the_steps_by_index() {
        // Which is the reason for the length rule: an inserted step would pair
        // every later circle with somebody else's past and animate from it.
        let mut list = steps(3);
        list[0].state = StepState::Complete;
        list[1].state = StepState::Editing;
        let stepper = Stepper::new(list, 1);

        assert_eq!(
            stepper.old_states(),
            vec![StepState::Complete, StepState::Editing, StepState::Indexed]
        );
    }

    #[test]
    fn only_the_disabled_state_refuses_a_tap() {
        for state in [
            StepState::Indexed,
            StepState::Editing,
            StepState::Complete,
            StepState::Error,
        ] {
            assert!(state.reacts_to_taps(), "{state:?}");
        }
        assert!(!StepState::Disabled.reacts_to_taps());
    }

    #[test]
    fn a_guideline_is_enforced_as_an_error() {
        // Nothing here would break; the material spec says not to, and the
        // framework throws.
        assert!(!Stepper::may_nest_inside_a_stepper());
    }

    #[test]
    fn the_connector_colour_is_only_read_on_one_axis() {
        let style = StepStyle::new();
        assert!(style.connector_applies(StepperType::Horizontal));
        assert!(!style.connector_applies(StepperType::Vertical));
    }

    // -- Two indices, because there are two steps on screen ----------------------

    #[test]
    fn mid_transition_exactly_one_of_the_two_builders_is_building_the_active_step() {
        // The stepper is heading for step 2 while step 1 animates away, and both
        // get a controlsBuilder call.
        let arriving = ControlsDetails::new(2, 2);
        let leaving = ControlsDetails::new(2, 1);
        assert!(arriving.is_active());
        assert!(!leaving.is_active());
        assert_eq!(
            arriving.current_step, leaving.current_step,
            "the same stepper, told to two different builders"
        );
    }

    #[test]
    fn the_two_is_actives_in_this_file_are_not_the_same_thing() {
        // Step.isActive is a styling flag the caller sets; it says nothing about
        // where the stepper is.
        let mut step = Step::default();
        step.is_active = true;
        assert!(step.is_active);
        assert!(!ControlsDetails::new(0, 3).is_active());
    }

    // -- One axis, not two -----------------------------------------------------------

    #[test]
    fn a_tap_in_the_padding_above_a_button_still_lands_on_it() {
        let buttons = ToggleButtons::new(vec![false, true, false]);
        assert_eq!(buttons.padding_each_side(), 8.0);

        // Two pixels down from the top of the padded strip -- above the drawn
        // button entirely.
        let (_, y) = buttons.hit_test_point((120.0, 2.0), (60.0, 32.0));
        assert_eq!(y, 16.0, "pinned to the button's middle");
    }

    #[test]
    fn but_the_main_axis_is_left_alone_so_the_right_button_is_tapped() {
        // Collapsing this axis too would hand every tap to whichever neighbour
        // was asked first.
        let buttons = ToggleButtons::new(vec![false, false, false]);
        let first = buttons.hit_test_point((10.0, 2.0), (60.0, 32.0));
        let third = buttons.hit_test_point((140.0, 2.0), (60.0, 32.0));

        assert_eq!(first.0, 10.0);
        assert_eq!(third.0, 140.0);
        assert_ne!(first.0, third.0, "the row is still distinguishable");
        assert_eq!(first.1, third.1, "while the cross axis is not");
    }

    #[test]
    fn a_vertical_row_pins_the_other_axis() {
        let mut buttons = ToggleButtons::new(vec![false, false]);
        buttons.direction = Axis::Vertical;
        let point = buttons.hit_test_point((2.0, 90.0), (48.0, 32.0));
        assert_eq!(point, (24.0, 90.0));
    }

    #[test]
    fn a_hit_outside_the_padded_box_is_refused_before_any_of_that() {
        // Skipping super.hitTest() skipped its bounds check, so the override
        // does it by hand.
        let buttons = ToggleButtons::new(vec![false]);
        assert!(buttons.contains((10.0, 10.0), (60.0, 48.0)));
        assert!(!buttons.contains((10.0, 60.0), (60.0, 48.0)));
        assert!(!buttons.contains((-1.0, 10.0), (60.0, 48.0)));
    }

    #[test]
    fn a_button_with_no_padding_to_spare_asks_for_none() {
        let mut buttons = ToggleButtons::new(vec![false]);
        buttons.button_extent = 48.0;
        assert_eq!(buttons.padding_each_side(), 0.0);
        buttons.button_extent = 60.0;
        assert_eq!(buttons.padding_each_side(), 0.0, "and never negative");
    }

    // -- The parallel lists ---------------------------------------------------------

    #[test]
    fn the_children_and_their_states_are_correlated_by_index() {
        let mut buttons = ToggleButtons::new(vec![true, false, true]);
        assert!(buttons.lengths_match());
        buttons.child_count = 2;
        assert!(!buttons.lengths_match());
    }

    #[test]
    fn focus_nodes_are_optional_but_not_optional_in_length() {
        let mut buttons = ToggleButtons::new(vec![true, false, true]);
        assert!(buttons.focus_nodes_match(), "none given is fine");
        buttons.focus_node_count = Some(3);
        assert!(buttons.focus_nodes_match());
        buttons.focus_node_count = Some(2);
        assert!(!buttons.focus_nodes_match());
    }
}
