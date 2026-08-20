//! Ports of `material/input_decorator.dart` and the remaining class of
//! `material/input_border.dart`.
//!
//! The box around a text field: the border, the label that floats out of the
//! way, and the line underneath that is either an explanation or a complaint.

/// Upstream `FloatingLabelBehavior`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatingLabelBehavior {
    /// Always inside the content, or hidden.
    Never,
    /// Floats when the field is focused **or has content**, and that second
    /// half is the one that matters: a filled field must float its label, or
    /// the label would sit on top of what the reader typed.
    #[default]
    Auto,
    Always,
}

impl FloatingLabelBehavior {
    pub fn floats(self, focused: bool, has_content: bool) -> bool {
        match self {
            FloatingLabelBehavior::Never => false,
            FloatingLabelBehavior::Always => true,
            FloatingLabelBehavior::Auto => focused || has_content,
        }
    }
}

/// Upstream `ShapedInputBorder`.
///
/// The class whose documentation states the useful thing: **a border with no
/// side still defines a shape.** With `BorderSide.none` no line is drawn, and
/// upstream notes you can still see the shape *"if `InputDecoration.filled` is
/// true"* -- the fill is clipped to it.
///
/// A border is two things, a line and an outline, and turning the line off
/// leaves the outline behind. Which is also why upstream warns that the
/// floating label should be set to `never` in that case: the label's notch is
/// cut from the **shape**, so it goes on being cut *"as if the border were
/// still being drawn"*.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedInputBorder {
    pub has_side: bool,
    /// Horizontal padding either side of the gap the floating label cuts.
    pub gap_padding: f32,
}

impl ShapedInputBorder {
    pub const DEFAULT_GAP_PADDING: f32 = 4.0;

    pub fn new() -> ShapedInputBorder {
        ShapedInputBorder {
            has_side: true,
            gap_padding: ShapedInputBorder::DEFAULT_GAP_PADDING,
        }
    }

    pub fn without_side() -> ShapedInputBorder {
        ShapedInputBorder {
            has_side: false,
            ..ShapedInputBorder::new()
        }
    }

    pub fn is_valid(&self) -> bool {
        self.gap_padding >= 0.0
    }

    /// Whether a line is stroked.
    pub fn paints_line(&self) -> bool {
        self.has_side
    }

    /// Whether there is an outline to clip a fill against and to cut a label
    /// notch from. **Always** -- that is the point of the class.
    pub fn defines_shape(&self) -> bool {
        true
    }

    /// The trap upstream names: a label left on `auto` cuts its notch even with
    /// no line drawn.
    pub fn label_cuts_a_gap(
        &self,
        behavior: FloatingLabelBehavior,
        focused: bool,
        has_content: bool,
    ) -> bool {
        behavior.floats(focused, has_content)
    }
}

impl Default for ShapedInputBorder {
    fn default() -> Self {
        ShapedInputBorder::new()
    }
}

/// What is shown on the line beneath the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleSlot {
    Helper,
    Error,
    Nothing,
}

/// Upstream `InputDecoration`.
#[derive(Clone, Debug, PartialEq)]
pub struct InputDecoration {
    pub label_text: Option<String>,
    pub helper_text: Option<String>,
    pub error_text: Option<String>,
    pub counter_text: Option<String>,
    pub floating_label_behavior: FloatingLabelBehavior,
    pub filled: bool,
    pub is_dense: bool,
    /// Upstream's `isCollapsed`, which the `collapsed` constructor sets
    /// **together with** zero content padding. "Collapsed" is a bundle of two
    /// settings, not one flag -- a field with the flag and its ordinary padding
    /// would still take the room it was trying not to.
    pub is_collapsed: bool,
    pub content_padding_is_zero: bool,
    /// Whether a widget was given for the slot as well as a string.
    pub has_helper_widget: bool,
    pub has_prefix_widget: bool,
    pub has_prefix_text: bool,
    pub has_suffix_widget: bool,
    pub has_suffix_text: bool,
}

impl InputDecoration {
    pub fn new() -> InputDecoration {
        InputDecoration {
            label_text: None,
            helper_text: None,
            error_text: None,
            counter_text: None,
            floating_label_behavior: FloatingLabelBehavior::Auto,
            filled: false,
            is_dense: false,
            is_collapsed: false,
            content_padding_is_zero: false,
            has_helper_widget: false,
            has_prefix_widget: false,
            has_prefix_text: false,
            has_suffix_widget: false,
            has_suffix_text: false,
        }
    }

    /// Upstream `InputDecoration.collapsed`.
    pub fn collapsed() -> InputDecoration {
        InputDecoration {
            is_collapsed: true,
            content_padding_is_zero: true,
            ..InputDecoration::new()
        }
    }

    /// Upstream's three "only one of" asserts. In each pair the widget form and
    /// the string form are **alternatives**: giving both says nothing, because
    /// there is one slot and no rule for which fills it.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.has_helper_widget && self.helper_text.is_some() {
            return Err("only one of helper and helperText can be specified");
        }
        if self.has_prefix_widget && self.has_prefix_text {
            return Err("only one of prefix and prefixText can be specified");
        }
        if self.has_suffix_widget && self.has_suffix_text {
            return Err("only one of suffix and suffixText can be specified");
        }
        Ok(())
    }

    /// Upstream: the helper is *"displayed in the same location as errorText.
    /// If a non-null errorText value is specified then the helper text is not
    /// shown."*
    ///
    /// One slot, and the error wins. Which is right: both are the line under
    /// the field, and a field cannot be explaining itself and complaining at
    /// the same time. The complaint is the more urgent of the two.
    pub fn subtitle(&self) -> SubtitleSlot {
        if self.error_text.is_some() {
            SubtitleSlot::Error
        } else if self.helper_text.is_some() || self.has_helper_widget {
            SubtitleSlot::Helper
        } else {
            SubtitleSlot::Nothing
        }
    }
}

impl Default for InputDecoration {
    fn default() -> Self {
        InputDecoration::new()
    }
}

/// Where the label sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelPlacement {
    /// Floating above the content, out of the way of the text.
    Floating,
    /// Sitting in the content, where it reads as a placeholder.
    Inline,
    /// Not shown at all -- there was no label.
    Absent,
}

/// Upstream `InputDecorator`, as the decisions it makes.
#[derive(Clone, Debug, PartialEq)]
pub struct InputDecorator {
    pub decoration: InputDecoration,
    pub is_focused: bool,
    pub is_empty: bool,
    pub expands: bool,
}

impl InputDecorator {
    pub fn new(decoration: InputDecoration) -> InputDecorator {
        InputDecorator {
            decoration,
            is_focused: false,
            is_empty: true,
            expands: false,
        }
    }

    pub fn focused(mut self) -> Self {
        self.is_focused = true;
        self
    }

    pub fn with_content(mut self) -> Self {
        self.is_empty = false;
        self
    }

    pub fn label_placement(&self) -> LabelPlacement {
        if self.decoration.label_text.is_none() {
            return LabelPlacement::Absent;
        }
        if self
            .decoration
            .floating_label_behavior
            .floats(self.is_focused, !self.is_empty)
        {
            LabelPlacement::Floating
        } else {
            LabelPlacement::Inline
        }
    }

    /// Upstream substitutes its own `borderSide` based on the theme and whether
    /// the field is focused -- **unless** the caller asked for
    /// `BorderSide.none`, which is taken as a decision rather than an absence.
    pub fn overrides_border_side(border: &ShapedInputBorder) -> bool {
        border.has_side
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labelled() -> InputDecoration {
        InputDecoration {
            label_text: Some("Name".to_string()),
            ..InputDecoration::new()
        }
    }

    // -- The floating label ----------------------------------------------------

    #[test]
    fn a_filled_field_must_float_its_label_even_unfocused() {
        // Otherwise the label would sit on top of what the reader typed.
        let auto = FloatingLabelBehavior::Auto;
        assert!(!auto.floats(false, false), "empty and unfocused: inline");
        assert!(auto.floats(true, false), "focused: floating");
        assert!(auto.floats(false, true), "and filled, even unfocused");
    }

    #[test]
    fn never_and_always_do_not_consult_anything() {
        for focused in [false, true] {
            for content in [false, true] {
                assert!(!FloatingLabelBehavior::Never.floats(focused, content));
                assert!(FloatingLabelBehavior::Always.floats(focused, content));
            }
        }
    }

    #[test]
    fn a_field_with_no_label_has_nowhere_to_put_one() {
        let decorator = InputDecorator::new(InputDecoration::new()).focused();
        assert_eq!(decorator.label_placement(), LabelPlacement::Absent);
    }

    #[test]
    fn an_inline_label_reads_as_a_placeholder_and_a_floating_one_as_a_name() {
        let empty = InputDecorator::new(labelled());
        assert_eq!(empty.label_placement(), LabelPlacement::Inline);

        assert_eq!(
            InputDecorator::new(labelled()).focused().label_placement(),
            LabelPlacement::Floating
        );
        assert_eq!(
            InputDecorator::new(labelled())
                .with_content()
                .label_placement(),
            LabelPlacement::Floating
        );
    }

    // -- A border with no side ---------------------------------------------------

    #[test]
    fn a_border_with_no_side_still_defines_a_shape() {
        // Which you can see when the field is filled: the fill is clipped to
        // it. A border is a line and an outline, and turning off the line
        // leaves the outline.
        let bare = ShapedInputBorder::without_side();
        assert!(!bare.paints_line());
        assert!(bare.defines_shape());

        let ordinary = ShapedInputBorder::new();
        assert!(ordinary.paints_line());
        assert!(ordinary.defines_shape());
    }

    #[test]
    fn the_label_notch_is_cut_from_the_shape_and_not_from_the_line() {
        // Which is the trap upstream names: with no border drawn, a label left
        // on auto still cuts its gap as if the border were there.
        let bare = ShapedInputBorder::without_side();
        assert!(
            bare.label_cuts_a_gap(FloatingLabelBehavior::Auto, true, false),
            "still notched, with no line to notch"
        );
        assert!(!bare.label_cuts_a_gap(FloatingLabelBehavior::Never, true, false));
    }

    #[test]
    fn a_side_the_caller_removed_is_a_decision_and_not_an_absence() {
        // So the decorator does not substitute its own from the theme.
        assert!(InputDecorator::overrides_border_side(
            &ShapedInputBorder::new()
        ));
        assert!(!InputDecorator::overrides_border_side(
            &ShapedInputBorder::without_side()
        ));
    }

    #[test]
    fn a_negative_gap_padding_is_refused() {
        assert!(ShapedInputBorder::new().is_valid());
        let mut bad = ShapedInputBorder::new();
        bad.gap_padding = -1.0;
        assert!(!bad.is_valid());
    }

    // -- One line, two things that might go on it -----------------------------------

    #[test]
    fn a_field_cannot_explain_itself_and_complain_at_the_same_time() {
        // Both are the line under the field, and the complaint is the more
        // urgent.
        let mut decoration = InputDecoration::new();
        assert_eq!(decoration.subtitle(), SubtitleSlot::Nothing);

        decoration.helper_text = Some("Your full name".to_string());
        assert_eq!(decoration.subtitle(), SubtitleSlot::Helper);

        decoration.error_text = Some("Required".to_string());
        assert_eq!(
            decoration.subtitle(),
            SubtitleSlot::Error,
            "and the helper is not shown"
        );
    }

    #[test]
    fn the_widget_form_and_the_string_form_are_alternatives() {
        // Giving both says nothing: one slot, and no rule for which fills it.
        let mut decoration = InputDecoration::new();
        assert_eq!(decoration.validate(), Ok(()));

        decoration.has_helper_widget = true;
        assert_eq!(decoration.validate(), Ok(()), "a widget alone is fine");

        decoration.helper_text = Some("Your full name".to_string());
        assert!(decoration.validate().is_err());
    }

    #[test]
    fn the_same_rule_holds_for_the_prefix_and_the_suffix() {
        let mut prefix = InputDecoration::new();
        prefix.has_prefix_widget = true;
        prefix.has_prefix_text = true;
        assert!(prefix.validate().is_err());

        let mut suffix = InputDecoration::new();
        suffix.has_suffix_widget = true;
        suffix.has_suffix_text = true;
        assert!(suffix.validate().is_err());
    }

    #[test]
    fn collapsed_is_a_bundle_of_two_settings_and_not_one_flag() {
        // A field with the flag and its ordinary padding would still take the
        // room it was trying not to.
        let collapsed = InputDecoration::collapsed();
        assert!(collapsed.is_collapsed);
        assert!(collapsed.content_padding_is_zero);

        let ordinary = InputDecoration::new();
        assert!(!ordinary.is_collapsed);
        assert!(!ordinary.content_padding_is_zero);
    }
}
