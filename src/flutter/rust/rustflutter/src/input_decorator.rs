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
    /// Upstream's `labelShouldWithdraw`, which is
    /// `!isEmpty || (isFocused && decoration.enabled)` and then
    /// `|| behavior == always`.
    ///
    /// **The `enabled` term is upstream's and was missing here.** A disabled
    /// field that holds focus keeps its label inline: floating it would say
    /// the field is being edited when it cannot be.
    pub fn withdraws(self, focused: bool, has_content: bool, enabled: bool) -> bool {
        match self {
            FloatingLabelBehavior::Always => true,
            _ => has_content || (focused && enabled),
        }
    }

    /// Upstream's `_floatingLabelEnabled`: anything but `never`.
    pub fn allows_floating(self) -> bool {
        self != FloatingLabelBehavior::Never
    }

    /// Whether the label is drawn above the content.
    ///
    /// Withdrawing is not the same question as floating: a label withdraws
    /// because there is something in the field, and it floats only if the
    /// behaviour lets it. Under `never` it withdraws and goes **nowhere** --
    /// see [`LabelPlacement::Hidden`].
    pub fn floats(self, focused: bool, has_content: bool) -> bool {
        self.allows_floating() && self.withdraws(focused, has_content, true)
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
    /// Upstream's `hintText`, the placeholder shown in an empty field.
    pub hint_text: Option<String>,
    pub helper_text: Option<String>,
    pub error_text: Option<String>,
    pub counter_text: Option<String>,
    pub floating_label_behavior: FloatingLabelBehavior,
    /// Upstream's `enabled`, true by default. A disabled field is still
    /// decorated -- and still shows its error border if it has one, which is
    /// [`crate::component_themes::ResolvedInputBorder`]'s business.
    pub enabled: bool,
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
    /// The widget forms of the three that had only their string form here.
    ///
    /// Upstream carries both for each, forbids giving both, and asks
    /// `x != null || xText != null` wherever it wants to know whether there is
    /// one. Modelling only the string made the widget form invisible: a field
    /// given an `error` widget and no `errorText` reported no error at all, so
    /// its helper line stayed put and the border stayed the enabled colour.
    pub has_label_widget: bool,
    pub has_hint_widget: bool,
    pub has_error_widget: bool,
    pub has_prefix_widget: bool,
    pub has_prefix_text: bool,
    pub has_suffix_widget: bool,
    pub has_suffix_text: bool,
}

impl InputDecoration {
    pub fn new() -> InputDecoration {
        InputDecoration {
            hint_text: None,
            has_label_widget: false,
            has_hint_widget: false,
            has_error_widget: false,
            label_text: None,
            helper_text: None,
            error_text: None,
            counter_text: None,
            floating_label_behavior: FloatingLabelBehavior::Auto,
            enabled: true,
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

    /// Upstream's **six** "only one of" asserts. In each pair the widget form
    /// and the string form are alternatives: giving both says nothing, because
    /// there is one slot and no rule for which fills it.
    ///
    /// This said "three" and had three -- helper, prefix, suffix. Upstream
    /// also forbids both forms of the label, the hint and the error, and those
    /// three were missing here along with the widget forms they are about.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.has_label_widget && self.label_text.is_some() {
            return Err("only one of label and labelText can be specified");
        }
        if self.has_hint_widget && self.hint_text.is_some() {
            return Err("only one of hint and hintText can be specified");
        }
        if self.has_error_widget && self.error_text.is_some() {
            return Err("only one of error and errorText can be specified");
        }
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
        if self.has_error() {
            SubtitleSlot::Error
        } else if self.helper_text.is_some() || self.has_helper_widget {
            SubtitleSlot::Helper
        } else {
            SubtitleSlot::Nothing
        }
    }

    /// Upstream's `_hasError`: `errorText != null || error != null`.
    ///
    /// Both forms, because a field given an error *widget* is as wrong as one
    /// given an error string, and everything downstream -- the subtitle slot,
    /// the border colour, the label colour -- turns on this one answer.
    pub fn has_error(&self) -> bool {
        self.error_text.is_some() || self.has_error_widget
    }

    /// Upstream's label test: `labelText != null || label != null`.
    pub fn has_label(&self) -> bool {
        self.label_text.is_some() || self.has_label_widget
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
    /// There is a label, and it is drawn nowhere.
    ///
    /// Upstream's `_shouldShowLabel` -- `_hasInlineLabel || _floatingLabelEnabled`
    /// -- is false in exactly one case: the behaviour is `never` **and** the
    /// label has withdrawn. It has nowhere to go: `never` forbids the floating
    /// position, and the inline position is where the reader's own text now
    /// is.
    ///
    /// This port had no such state, and answered `Inline` -- which draws the
    /// label **on top of what was typed**. That is the thing
    /// [`ShapedInputBorder`]'s own doc warns about from the other direction.
    Hidden,
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

    /// Upstream's `_shouldShowLabel` and `_hasInlineLabel`, together.
    pub fn label_placement(&self) -> LabelPlacement {
        if !self.decoration.has_label() {
            return LabelPlacement::Absent;
        }
        let behavior = self.decoration.floating_label_behavior;
        let withdrawn = behavior.withdraws(
            self.is_focused,
            !self.is_empty,
            self.decoration.enabled,
        );
        match (withdrawn, behavior.allows_floating()) {
            (true, true) => LabelPlacement::Floating,
            (true, false) => LabelPlacement::Hidden,
            (false, _) => LabelPlacement::Inline,
        }
    }

    /// Which named border applies, and where its side comes from -- see
    /// [`crate::component_themes::ResolvedInputBorder`].
    pub fn resolved_border(
        &self,
        context: &mut crate::framework::BuildContext,
        border: &ShapedInputBorder,
        border_is_state_property: bool,
    ) -> crate::component_themes::ResolvedInputBorder {
        crate::component_themes::ResolvedInputBorder::of(
            context,
            self.decoration.enabled,
            self.is_focused,
            self.decoration.has_error(),
            border_is_state_property,
            !border.has_side,
        )
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

    #[test]
    fn a_label_told_never_to_float_is_hidden_rather_than_drawn_over_the_text() {
        // Upstream's `_shouldShowLabel` is `_hasInlineLabel || _floatingLabelEnabled`,
        // and it is false in exactly one case: `never`, with the label
        // withdrawn. It has nowhere to go -- `never` forbids the floating
        // position and the reader's own text is now in the inline one.
        //
        // This port had no such state and answered `Inline`, which draws the
        // label **on top of what was typed**. `ShapedInputBorder`'s own doc
        // warns about `never` from the other direction; this is the near side
        // of it.
        let never = || {
            let mut decoration = labelled();
            decoration.floating_label_behavior = FloatingLabelBehavior::Never;
            decoration
        };
        assert_eq!(
            InputDecorator::new(never()).label_placement(),
            LabelPlacement::Inline,
            "an empty field still reads it as a placeholder"
        );
        assert_eq!(
            InputDecorator::new(never()).with_content().label_placement(),
            LabelPlacement::Hidden,
            "and once there is text, it goes nowhere"
        );
        assert_eq!(
            InputDecorator::new(never()).focused().label_placement(),
            LabelPlacement::Hidden,
            "the same while it is being typed into"
        );
    }

    #[test]
    fn a_field_you_cannot_edit_does_not_float_its_label_for_being_focused() {
        // Upstream's `_labelShouldWithdraw` is
        // `!isEmpty || (isFocused && decoration.enabled)`, and the `enabled`
        // term was missing here. Floating the label of a disabled field says
        // it is being edited when it cannot be.
        let disabled = || {
            let mut decoration = labelled();
            decoration.enabled = false;
            decoration
        };
        assert_eq!(
            InputDecorator::new(disabled()).focused().label_placement(),
            LabelPlacement::Inline,
            "focus alone does not lift it"
        );
        // But content does, because the label would otherwise sit on the text
        // -- and that is true whether or not the field can be edited.
        assert_eq!(
            InputDecorator::new(disabled())
                .with_content()
                .label_placement(),
            LabelPlacement::Floating
        );
        // And an enabled field is the case that shows the term is read.
        assert_eq!(
            InputDecorator::new(labelled()).focused().label_placement(),
            LabelPlacement::Floating
        );
    }

    #[test]
    fn always_floats_a_label_with_nothing_under_it_at_all() {
        // The third behaviour, and the one that makes `withdraws` more than
        // "is there something here": `always` withdraws an empty, unfocused,
        // disabled field's label.
        let mut decoration = labelled();
        decoration.floating_label_behavior = FloatingLabelBehavior::Always;
        decoration.enabled = false;
        assert_eq!(
            InputDecorator::new(decoration).label_placement(),
            LabelPlacement::Floating
        );
    }

    #[test]
    fn a_field_given_an_error_widget_is_as_wrong_as_one_given_an_error_string() {
        // Upstream's `_hasError` is `errorText != null || error != null`, and
        // everything under the field turns on that one answer: which slot the
        // subtitle line holds, what colour the border is, what colour the
        // label is. Only the string form was modelled here, so a field given
        // an error *widget* reported no error at all -- its helper line stayed
        // put and its border stayed the enabled colour.
        let mut widget_form = InputDecoration::new();
        widget_form.has_error_widget = true;
        assert!(widget_form.has_error());
        assert_eq!(widget_form.subtitle(), SubtitleSlot::Error);

        let mut string_form = InputDecoration::new();
        string_form.error_text = Some(String::from("too short"));
        assert!(string_form.has_error());
        assert_eq!(string_form.subtitle(), SubtitleSlot::Error);

        // And the error still outranks a helper given either way.
        let mut both = InputDecoration::new();
        both.has_error_widget = true;
        both.has_helper_widget = true;
        assert_eq!(both.subtitle(), SubtitleSlot::Error);
    }

    #[test]
    fn a_label_widget_is_a_label_and_gets_a_placement_like_one() {
        // The same asymmetry, on the other end of the field. A label given as
        // a widget was `Absent` -- so it was never floated, never withdrawn,
        // and the field looked like one with no name.
        let mut decoration = InputDecoration::new();
        decoration.has_label_widget = true;
        assert!(decoration.has_label());
        assert_eq!(
            InputDecorator::new(decoration.clone()).label_placement(),
            LabelPlacement::Inline
        );
        assert_eq!(
            InputDecorator::new(decoration).focused().label_placement(),
            LabelPlacement::Floating
        );
    }

    #[test]
    fn each_of_the_six_pairs_may_be_given_one_way_or_the_other_and_not_both() {
        // Upstream asserts six times, once per pair. This said "three" and had
        // three; the label, the hint and the error were missing along with the
        // widget forms they are about.
        let pairs: Vec<(&str, fn(&mut InputDecoration), fn(&mut InputDecoration))> = vec![
            (
                "label",
                |d| d.has_label_widget = true,
                |d| d.label_text = Some(String::from("x")),
            ),
            (
                "hint",
                |d| d.has_hint_widget = true,
                |d| d.hint_text = Some(String::from("x")),
            ),
            (
                "error",
                |d| d.has_error_widget = true,
                |d| d.error_text = Some(String::from("x")),
            ),
            (
                "helper",
                |d| d.has_helper_widget = true,
                |d| d.helper_text = Some(String::from("x")),
            ),
            ("prefix", |d| d.has_prefix_widget = true, |d| d.has_prefix_text = true),
            ("suffix", |d| d.has_suffix_widget = true, |d| d.has_suffix_text = true),
        ];
        for (name, as_widget, as_string) in pairs {
            let mut only_widget = InputDecoration::new();
            as_widget(&mut only_widget);
            assert!(only_widget.validate().is_ok(), "{name} as a widget alone");

            let mut only_string = InputDecoration::new();
            as_string(&mut only_string);
            assert!(only_string.validate().is_ok(), "{name} as a string alone");

            let mut both = InputDecoration::new();
            as_widget(&mut both);
            as_string(&mut both);
            let refused = both.validate();
            assert!(refused.is_err(), "{name} both ways");
            assert!(
                refused.unwrap_err().contains(name),
                "and the message names the pair"
            );
        }
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

#[cfg(test)]
mod input_border_tests {
    use super::*;
    use crate::component_themes::{
        InputBorderSide, InputBorderSlot, InputDecorationTheme, InputDecorationThemeData,
        ResolvedInputBorder,
    };
    use crate::framework::{
        AnyWidget, BuildContext, Component, ElementTree, component, leaf, provide,
    };
    use crate::theme::ThemeData;
    use crate::widget_state::{WidgetState, WidgetStates};
    use crate::widgets::SizedBox;

    struct Reader {
        decorator: InputDecorator,
        border: ShapedInputBorder,
        state_property: bool,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedInputBorder>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.decorator.resolved_border(
                context,
                &self.border,
                self.state_property,
            ));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn resolve(
        theme: ThemeData,
        data: InputDecorationThemeData,
        decorator: InputDecorator,
        border: ShapedInputBorder,
        state_property: bool,
    ) -> ResolvedInputBorder {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            theme,
            InputDecorationTheme::new(
                data,
                component(Reader {
                    decorator,
                    border,
                    state_property,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn field(enabled: bool, focused: bool, error: bool) -> InputDecorator {
        let mut decoration = InputDecoration::new();
        decoration.enabled = enabled;
        if error {
            decoration.error_text = Some("no".to_string());
        }
        let mut decorator = InputDecorator::new(decoration);
        decorator.is_focused = focused;
        decorator
    }

    fn slot(enabled: bool, focused: bool, error: bool) -> InputBorderSlot {
        resolve(
            ThemeData::fallback(),
            InputDecorationThemeData::new(),
            field(enabled, focused, error),
            ShapedInputBorder::new(),
            false,
        )
        .slot
    }

    // -- Which of the five ------------------------------------------------------

    #[test]
    fn a_field_you_cannot_edit_still_tells_you_it_is_wrong() {
        // `errorBorder` covers two of the six cells, and the disabled one is
        // among them. Being unable to fix something is not a reason to stop
        // being told about it.
        assert_eq!(slot(false, false, true), InputBorderSlot::Error);
        assert_eq!(slot(false, false, false), InputBorderSlot::Disabled);
        assert_ne!(slot(false, false, true), slot(false, false, false));
    }

    #[test]
    fn and_being_disabled_outranks_being_focused() {
        // A disabled field cannot hold focus, but the pick is written with
        // `!enabled` first, so if it somehow did the disabled answer wins.
        assert_eq!(slot(false, true, false), InputBorderSlot::Disabled);
        assert_eq!(slot(false, true, true), InputBorderSlot::Error);
    }

    #[test]
    fn focus_and_error_together_have_a_border_of_their_own() {
        // Five names for six cells: the only pair that shares one is the two
        // error cells, and this is the cell that got its own name instead.
        assert_eq!(slot(true, true, true), InputBorderSlot::FocusedError);
        assert_eq!(slot(true, true, false), InputBorderSlot::Focused);
        assert_eq!(slot(true, false, true), InputBorderSlot::Error);
        assert_eq!(slot(true, false, false), InputBorderSlot::Enabled);
    }

    // -- Where the side comes from ---------------------------------------------

    #[test]
    fn a_border_that_asked_for_no_side_keeps_none() {
        // Replacing it would put a line back on a border that said it wanted
        // none -- and a border with no side still has a shape, so that was a
        // decision rather than an absence.
        let resolved = resolve(
            ThemeData::fallback(),
            InputDecorationThemeData::new(),
            field(true, false, false),
            ShapedInputBorder::without_side(),
            false,
        );
        assert_eq!(resolved.side, InputBorderSide::AsGiven);
    }

    #[test]
    fn a_state_dependent_border_is_left_to_answer_for_itself() {
        // The caller is already resolving per state; doing it again below
        // would be second-guessing them.
        let resolved = resolve(
            ThemeData::fallback(),
            InputDecorationThemeData::new(),
            field(true, false, false),
            ShapedInputBorder::new(),
            true,
        );
        assert_eq!(resolved.side, InputBorderSide::AsGiven);
    }

    #[test]
    fn filled_and_unfilled_read_different_fields() {
        let mut filled = InputDecorationThemeData::new();
        filled.filled = true;
        assert_eq!(
            resolve(
                ThemeData::fallback(),
                filled,
                field(true, false, false),
                ShapedInputBorder::new(),
                false
            )
            .side,
            InputBorderSide::ActiveIndicator
        );
        assert_eq!(
            resolve(
                ThemeData::fallback(),
                InputDecorationThemeData::new(),
                field(true, false, false),
                ShapedInputBorder::new(),
                false
            )
            .side,
            InputBorderSide::Outline
        );
    }

    #[test]
    fn material_two_computes_a_width_instead_of_reading_a_side() {
        let two = ThemeData {
            use_material3: false,
            ..ThemeData::fallback()
        };
        for filled in [false, true] {
            let mut data = InputDecorationThemeData::new();
            data.filled = filled;
            assert_eq!(
                resolve(
                    two.clone(),
                    data,
                    field(true, false, false),
                    ShapedInputBorder::new(),
                    false
                )
                .side,
                InputBorderSide::MaterialTwo,
                "filled: {filled}"
            );
        }
    }

    #[test]
    fn the_early_returns_beat_the_material_version_too() {
        // They are returns, not branches: neither Material 3 nor Material 2
        // gets as far as choosing a source.
        for material3 in [false, true] {
            let theme = ThemeData {
                use_material3: material3,
                ..ThemeData::fallback()
            };
            assert_eq!(
                resolve(
                    theme,
                    InputDecorationThemeData::new(),
                    field(true, false, false),
                    ShapedInputBorder::without_side(),
                    false
                )
                .side,
                InputBorderSide::AsGiven
            );
        }
    }

    // -- The widths and the two ladders ----------------------------------------

    #[test]
    fn material_twos_zero_folds_three_different_reasons_together() {
        // A collapsed field, a border set to none and a disabled field all
        // draw nothing, and upstream writes them as one case because the
        // result is the same and only the reason differs.
        assert_eq!(
            ResolvedInputBorder::material_two_width(true, false, true, false),
            0.0
        );
        assert_eq!(
            ResolvedInputBorder::material_two_width(false, true, true, false),
            0.0
        );
        assert_eq!(
            ResolvedInputBorder::material_two_width(false, false, false, false),
            0.0
        );
        // And the zero wins even while focused, which would otherwise be 2.
        assert_eq!(
            ResolvedInputBorder::material_two_width(false, false, false, true),
            0.0
        );
        assert_eq!(
            ResolvedInputBorder::material_two_width(false, false, true, true),
            2.0
        );
        assert_eq!(
            ResolvedInputBorder::material_two_width(false, false, true, false),
            1.0
        );
    }

    #[test]
    fn the_two_ladders_agree_everywhere_but_disabled_and_resting() {
        // The claim the type's docs make, checked arm by arm.
        let scheme = ThemeData::fallback().color_scheme;
        let shared = [
            WidgetStates::of(&[WidgetState::Error, WidgetState::Focused]),
            WidgetStates::of(&[WidgetState::Error, WidgetState::Hovered]),
            WidgetStates::of(&[WidgetState::Error]),
            WidgetStates::of(&[WidgetState::Focused]),
            WidgetStates::of(&[WidgetState::Hovered]),
        ];
        for states in shared {
            assert_eq!(
                ResolvedInputBorder::side_color(InputBorderSide::ActiveIndicator, states, &scheme),
                ResolvedInputBorder::side_color(InputBorderSide::Outline, states, &scheme),
                "{states:?}"
            );
        }

        for states in [
            WidgetStates::of(&[WidgetState::Disabled]),
            WidgetStates::NONE,
        ] {
            assert_ne!(
                ResolvedInputBorder::side_color(InputBorderSide::ActiveIndicator, states, &scheme),
                ResolvedInputBorder::side_color(InputBorderSide::Outline, states, &scheme),
                "{states:?}"
            );
        }
    }

    #[test]
    fn the_outline_is_the_fainter_of_the_two_when_the_field_is_dead() {
        // Three times fainter, for the shape that encloses more: a box drawn
        // all the way round a dead field at the indicator's strength would
        // read as a live one.
        let scheme = ThemeData::fallback().color_scheme;
        let disabled = WidgetStates::of(&[WidgetState::Disabled]);
        assert_eq!(
            ResolvedInputBorder::side_color(InputBorderSide::Outline, disabled, &scheme),
            Some(crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                0.12
            ))
        );
        assert_eq!(
            ResolvedInputBorder::side_color(InputBorderSide::ActiveIndicator, disabled, &scheme),
            Some(crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                0.38
            ))
        );
    }

    #[test]
    fn the_colour_says_what_is_wrong_and_the_width_says_where_you_are() {
        // Error outranks focus for the colour, and focus still sets the width.
        let scheme = ThemeData::fallback().color_scheme;
        let error_focused = WidgetStates::of(&[WidgetState::Error, WidgetState::Focused]);
        assert_eq!(
            ResolvedInputBorder::side_color(InputBorderSide::Outline, error_focused, &scheme),
            Some(scheme.error),
            "not the primary, which plain focus would give"
        );
        assert_ne!(
            ResolvedInputBorder::side_color(InputBorderSide::Outline, error_focused, &scheme),
            ResolvedInputBorder::side_color(
                InputBorderSide::Outline,
                WidgetStates::of(&[WidgetState::Focused]),
                &scheme
            )
        );
        assert_eq!(ResolvedInputBorder::side_width(error_focused), 2.0);
        assert_eq!(ResolvedInputBorder::side_width(WidgetStates::NONE), 1.0);
    }

    #[test]
    fn a_side_that_was_kept_as_given_has_no_colour_to_report() {
        // `AsGiven` and `MaterialTwo` are not ladder positions -- one keeps the
        // caller's side and the other computes a width, so asking either for a
        // ladder colour is asking the wrong question.
        let scheme = ThemeData::fallback().color_scheme;
        assert_eq!(
            ResolvedInputBorder::side_color(InputBorderSide::AsGiven, WidgetStates::NONE, &scheme),
            None
        );
        assert_eq!(
            ResolvedInputBorder::side_color(
                InputBorderSide::MaterialTwo,
                WidgetStates::NONE,
                &scheme
            ),
            None
        );
    }
}
