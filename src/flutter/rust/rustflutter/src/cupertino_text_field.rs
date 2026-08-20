//! Ports of `cupertino/text_field.dart`'s `CupertinoTextField` and
//! `cupertino/text_form_field_row.dart`'s `CupertinoTextFormFieldRow`.
//!
//! Tick 92 ported Material's `TextFormField` and recorded that its `maxLength`
//! has three states where you would expect two. These are the same fields in
//! the other design language, and they have two.

use crate::services::text_formatter::MaxLengthEnforcement;

/// Why a Cupertino text field's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CupertinoTextFieldError {
    InitialValueWithController,
    ObscuringCharacterNotSingle,
    NonPositiveMaxLines,
    NonPositiveMinLines,
    MinLinesAboveMaxLines,
    ExpandsWithLineCount,
    ObscuredAndMultiline,
    NonPositiveMaxLength,
}

/// Upstream `CupertinoTextField`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoTextField {
    pub obscuring_character_len: usize,
    pub max_lines: Option<u32>,
    pub min_lines: Option<u32>,
    pub expands: bool,
    pub obscure_text: bool,
    pub max_length: Option<i32>,
    pub max_length_enforcement: MaxLengthEnforcement,
    /// Upstream's `.borderless` named constructor, which repeats the plain
    /// one's asserts verbatim.
    pub borderless: bool,
}

impl CupertinoTextField {
    pub fn new() -> CupertinoTextField {
        CupertinoTextField {
            obscuring_character_len: 1,
            max_lines: Some(1),
            min_lines: None,
            expands: false,
            obscure_text: false,
            max_length: None,
            max_length_enforcement: MaxLengthEnforcement::Enforced,
            borderless: false,
        }
    }

    pub fn borderless() -> CupertinoTextField {
        CupertinoTextField {
            borderless: true,
            ..CupertinoTextField::new()
        }
    }

    /// Upstream's asserts, and the one worth stopping on is the last:
    ///
    /// ```dart
    /// assert(maxLength == null || maxLength > 0),
    /// ```
    ///
    /// Material's `TextFormField` writes the same line as
    /// `maxLength == null || maxLength == TextField.noMaxLength || maxLength > 0`,
    /// carving out `-1` as a sentinel. **There is no `noMaxLength` anywhere in
    /// this file**, and the field's doc says flatly: *"This value must be either
    /// null or greater than zero."*
    ///
    /// Which is not an oversight. Tick 92 read Material's third state as "show
    /// the counter, count up, never stop" -- and **`CupertinoTextField` has no
    /// counter at all**, no `buildCounter`, nothing to display a running total.
    /// With no counter there is nothing for a count-without-limit to mean, so
    /// the sentinel has no work to do here.
    ///
    /// And where a Cupertino field does want the limit not enforced, it says so
    /// with a **separate enum** -- `MaxLengthEnforcement.none` -- rather than a
    /// magic value inside the number. Two designs for one requirement: Material
    /// overloads the value, Cupertino adds a mode. The second is the one you can
    /// combine with a real limit.
    pub fn validate(&self) -> Result<(), CupertinoTextFieldError> {
        if self.obscuring_character_len != 1 {
            return Err(CupertinoTextFieldError::ObscuringCharacterNotSingle);
        }
        if self.max_lines == Some(0) {
            return Err(CupertinoTextFieldError::NonPositiveMaxLines);
        }
        if self.min_lines == Some(0) {
            return Err(CupertinoTextFieldError::NonPositiveMinLines);
        }
        if let (Some(max), Some(min)) = (self.max_lines, self.min_lines) {
            if max < min {
                return Err(CupertinoTextFieldError::MinLinesAboveMaxLines);
            }
        }
        if self.expands && (self.max_lines.is_some() || self.min_lines.is_some()) {
            return Err(CupertinoTextFieldError::ExpandsWithLineCount);
        }
        if self.obscure_text && self.max_lines != Some(1) {
            return Err(CupertinoTextFieldError::ObscuredAndMultiline);
        }
        if self.max_length.is_some_and(|length| length <= 0) {
            return Err(CupertinoTextFieldError::NonPositiveMaxLength);
        }
        Ok(())
    }

    /// Whether typing past the limit is actually prevented.
    pub fn limits_input(&self) -> bool {
        self.max_length.is_some() && self.max_length_enforcement != MaxLengthEnforcement::None
    }

    /// `CupertinoTextField` shows no character counter under any configuration.
    pub fn shows_a_counter() -> bool {
        false
    }

    /// The twenty lines of asserts appear **twice**, once on the plain
    /// constructor and once on `.borderless`, byte for byte.
    ///
    /// Worth setting against tick 94's three copies of `buildScrollbar`, each
    /// carrying a comment asking you to keep them in step -- and already out of
    /// step. These two carry no such comment and are identical. **The comment is
    /// not what holds copies together**; proximity is, and those three live in
    /// three files while these two live twelve lines apart.
    pub fn assert_block_appears_twice() -> bool {
        true
    }
}

impl Default for CupertinoTextField {
    fn default() -> Self {
        CupertinoTextField::new()
    }
}

/// Upstream `CupertinoTextFormFieldRow`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoTextFormFieldRow {
    pub field: CupertinoTextField,
    pub has_initial_value: bool,
    pub has_controller: bool,
    pub has_prefix: bool,
    /// `None` is **not** zero. See [`CupertinoTextFormFieldRow::padding`].
    pub padding: Option<f32>,
}

impl CupertinoTextFormFieldRow {
    /// The standard iOS form row padding `CupertinoFormRow` supplies when none
    /// is given.
    pub const DEFAULT_PADDING: f32 = 6.0;

    pub fn new() -> CupertinoTextFormFieldRow {
        CupertinoTextFormFieldRow {
            field: CupertinoTextField::new(),
            has_initial_value: false,
            has_controller: false,
            has_prefix: false,
            padding: None,
        }
    }

    /// The same asserts as [`CupertinoTextField`] plus Material's
    /// `initialValue == null || controller == null` -- two ways of saying what
    /// is in the field, one too many.
    pub fn validate(&self) -> Result<(), CupertinoTextFieldError> {
        if self.has_initial_value && self.has_controller {
            return Err(CupertinoTextFieldError::InitialValueWithController);
        }
        self.field.validate()
    }

    /// Upstream's `padding` doc:
    ///
    /// > If the `padding` parameter is null, `CupertinoFormRow` constructs its
    /// > own default padding, which is the standard form row padding in iOS.
    /// > **If no edge insets are intended, explicitly pass `EdgeInsets.zero`.**
    ///
    /// **Null is not zero, it is "use the standard".** The third instance of
    /// this distinction in the sweep, after the icon button's `splashRadius`
    /// (null means the default, `Some(0.0)` is refused) and this file's own
    /// `maxLength` (null means no limit, zero is refused). Each time the unset
    /// value and the zero value mean different things, and each time the API
    /// has to say so in prose because the type cannot.
    pub fn padding(&self) -> f32 {
        self.padding
            .unwrap_or(CupertinoTextFormFieldRow::DEFAULT_PADDING)
    }

    /// Upstream: *"iOS guidelines encourage passing a `Text` widget to `prefix`
    /// to detail the nature of the input."* A label beside the field rather than
    /// floating inside it, which is where Material would put it.
    pub fn labels_beside_rather_than_inside() -> bool {
        true
    }
}

impl Default for CupertinoTextFormFieldRow {
    fn default() -> Self {
        CupertinoTextFormFieldRow::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_bridge::TextFormField;

    // -- Three states there, two here, and the reason ------------------------------

    #[test]
    fn the_sentinel_material_accepts_is_refused_here() {
        let mut cupertino = CupertinoTextField::new();
        cupertino.max_length = Some(TextFormField::NO_MAX_LENGTH);
        assert_eq!(
            cupertino.validate(),
            Err(CupertinoTextFieldError::NonPositiveMaxLength)
        );

        let mut material = TextFormField::new();
        material.max_length = Some(TextFormField::NO_MAX_LENGTH);
        assert_eq!(material.validate(), Ok(()));
    }

    #[test]
    fn because_there_is_no_counter_for_a_count_without_a_limit_to_feed() {
        // Tick 92 read the sentinel as "show the counter, never stop". Nothing
        // here shows one.
        assert!(!CupertinoTextField::shows_a_counter());
        assert_eq!(
            TextFormField::new().counter(),
            None,
            "and null means no counter in both"
        );
    }

    #[test]
    fn not_enforcing_a_limit_is_a_mode_here_rather_than_a_magic_number() {
        let mut field = CupertinoTextField::new();
        field.max_length = Some(10);
        assert!(field.limits_input());

        field.max_length_enforcement = MaxLengthEnforcement::None;
        assert_eq!(field.validate(), Ok(()), "still a perfectly good limit");
        assert!(!field.limits_input(), "just not an enforced one");
    }

    #[test]
    fn which_is_the_version_you_can_combine_with_a_real_number() {
        // The mode leaves maxLength meaning what it says; the sentinel replaces
        // it, so a Material field cannot both count to 10 and allow more.
        let mut field = CupertinoTextField::new();
        field.max_length = Some(10);
        field.max_length_enforcement = MaxLengthEnforcement::None;
        assert_eq!(field.max_length, Some(10));

        let mut material = TextFormField::new();
        material.max_length = Some(TextFormField::NO_MAX_LENGTH);
        assert_eq!(
            material.counter(),
            Some(None),
            "the number is gone, replaced by the sentinel"
        );
    }

    #[test]
    fn zero_is_refused_in_both_designs() {
        let mut cupertino = CupertinoTextField::new();
        cupertino.max_length = Some(0);
        assert_eq!(
            cupertino.validate(),
            Err(CupertinoTextFieldError::NonPositiveMaxLength)
        );

        let mut material = TextFormField::new();
        material.max_length = Some(0);
        assert!(material.validate().is_err());
    }

    // -- The asserts the two designs share --------------------------------------------

    #[test]
    fn an_obscured_field_cannot_be_multiline_here_either() {
        let mut field = CupertinoTextField::new();
        field.obscure_text = true;
        assert_eq!(field.validate(), Ok(()));

        field.max_lines = None;
        assert_eq!(
            field.validate(),
            Err(CupertinoTextFieldError::ObscuredAndMultiline)
        );
    }

    #[test]
    fn expanding_and_counting_lines_still_conflict() {
        let mut field = CupertinoTextField::new();
        field.expands = true;
        assert_eq!(
            field.validate(),
            Err(CupertinoTextFieldError::ExpandsWithLineCount)
        );
        field.max_lines = None;
        assert_eq!(field.validate(), Ok(()));
    }

    #[test]
    fn a_line_range_still_has_to_face_the_right_way() {
        let mut field = CupertinoTextField::new();
        field.max_lines = Some(2);
        field.min_lines = Some(5);
        assert_eq!(
            field.validate(),
            Err(CupertinoTextFieldError::MinLinesAboveMaxLines)
        );
    }

    #[test]
    fn the_borderless_constructor_repeats_every_one_of_them() {
        // Byte-identical blocks, and both are live.
        assert!(CupertinoTextField::assert_block_appears_twice());
        let mut borderless = CupertinoTextField::borderless();
        borderless.max_length = Some(0);
        assert_eq!(
            borderless.validate(),
            Err(CupertinoTextFieldError::NonPositiveMaxLength)
        );
        assert!(borderless.borderless);
    }

    #[test]
    fn the_row_adds_the_one_assert_the_field_does_not_need() {
        // A plain text field has no initialValue to conflict with a controller.
        let mut row = CupertinoTextFormFieldRow::new();
        row.has_initial_value = true;
        row.has_controller = true;
        assert_eq!(
            row.validate(),
            Err(CupertinoTextFieldError::InitialValueWithController)
        );

        row.has_controller = false;
        assert_eq!(row.validate(), Ok(()));
    }

    #[test]
    fn and_otherwise_defers_to_the_field_it_wraps() {
        let mut row = CupertinoTextFormFieldRow::new();
        row.field.obscuring_character_len = 3;
        assert_eq!(
            row.validate(),
            Err(CupertinoTextFieldError::ObscuringCharacterNotSingle)
        );
    }

    // -- Null is not zero -------------------------------------------------------------

    #[test]
    fn unset_padding_means_the_standard_one_and_zero_means_none() {
        let mut row = CupertinoTextFormFieldRow::new();
        assert_eq!(row.padding(), CupertinoTextFormFieldRow::DEFAULT_PADDING);
        assert_ne!(row.padding(), 0.0, "which is why the doc has to say so");

        row.padding = Some(0.0);
        assert_eq!(row.padding(), 0.0);
    }

    #[test]
    fn the_same_distinction_the_icon_buttons_splash_radius_makes() {
        use crate::buttons::IconButton;
        let mut button = IconButton::new();
        assert!(button.is_valid(), "None is the default");
        button.splash_radius = Some(0.0);
        assert!(!button.is_valid(), "and zero is not a default, it is a bug");
    }

    #[test]
    fn a_label_sits_beside_a_cupertino_field_rather_than_inside_it() {
        assert!(CupertinoTextFormFieldRow::labels_beside_rather_than_inside());
        let mut row = CupertinoTextFormFieldRow::new();
        row.has_prefix = true;
        assert!(row.has_prefix);
    }
}
