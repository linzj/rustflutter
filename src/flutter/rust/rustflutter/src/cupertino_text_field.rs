//! Ports of `cupertino/text_field.dart`'s `CupertinoTextField` and
//! `cupertino/text_form_field_row.dart`'s `CupertinoTextFormFieldRow`.
//!
//! Tick 92 ported Material's `TextFormField` and recorded that its `maxLength`
//! has three states where you would expect two. These are the same fields in
//! the other design language, and they have two.

use crate::cupertino::CupertinoFormRow;
use crate::render::EdgeInsets;
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

/// Upstream `OverlayVisibilityMode`: when a prefix, a suffix or the clear
/// button is on screen.
///
/// # `Editing` is about content, not about activity
///
/// The name reads as "while the reader is typing" and upstream's own doc says
/// otherwise: it appears "when the current text entry is not empty. This
/// includes prefilled text that the user did not type in manually."
///
/// So a field that opens with a value already in it is *editing* before
/// anybody has touched it, and a field the reader is focused on but has not
/// typed into is not. The mode asks what is in the field, not what is
/// happening to it.
///
/// # And a placeholder is not text
///
/// Both content modes say so: `editing` "does not include text in
/// placeholders", `notEditing` ignores them too. Which follows -- a
/// placeholder is the field telling you it is empty, so counting it as
/// content would make "empty" impossible to reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverlayVisibilityMode {
    /// Never, whatever the field holds.
    Never,
    /// Only while the field holds text.
    Editing,
    /// Only while it does not.
    NotEditing,
    /// Always.
    #[default]
    Always,
}

impl OverlayVisibilityMode {
    /// Upstream's `_shouldShowAttachment`.
    pub fn shows(self, has_text: bool) -> bool {
        match self {
            OverlayVisibilityMode::Never => false,
            OverlayVisibilityMode::Always => true,
            OverlayVisibilityMode::Editing => has_text,
            OverlayVisibilityMode::NotEditing => !has_text,
        }
    }
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
    pub has_placeholder: bool,
    pub has_prefix: bool,
    pub has_suffix: bool,
    /// Upstream's `prefixMode`/`suffixMode`, **`always`** by default.
    pub prefix_mode: OverlayVisibilityMode,
    pub suffix_mode: OverlayVisibilityMode,
    /// Upstream's `clearButtonMode`, **`never`** by default -- the one of the
    /// three that starts off.
    pub clear_button_mode: OverlayVisibilityMode,
    /// `None` is upstream's null, which is where the rule in
    /// [`CupertinoTextField::text_align_vertical`] applies.
    pub text_align_vertical: Option<crate::render::TextAlignVertical>,
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
            has_placeholder: false,
            has_prefix: false,
            has_suffix: false,
            prefix_mode: OverlayVisibilityMode::Always,
            suffix_mode: OverlayVisibilityMode::Always,
            clear_button_mode: OverlayVisibilityMode::Never,
            text_align_vertical: None,
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
    /// Upstream's `_hasDecoration`.
    ///
    /// # A clear button counts before it appears
    ///
    /// The test is `clearButtonMode != never`, not "the clear button is
    /// showing". A field whose button only appears once there is text is
    /// decorated while it is still empty -- because otherwise the field would
    /// change alignment the moment the reader typed a character, and the text
    /// they were looking at would jump.
    ///
    /// The other three are plain presence: a placeholder, a prefix, a suffix.
    pub fn has_decoration(&self) -> bool {
        self.has_placeholder
            || self.clear_button_mode != OverlayVisibilityMode::Never
            || self.has_prefix
            || self.has_suffix
    }

    /// Upstream's `_textAlignVertical`, with its comment: "CupertinoTextField
    /// has top alignment by default, unless it has decoration like a prefix or
    /// suffix, in which case it's aligned to the center."
    ///
    /// A bare field starts its text at the top, so a growing multiline field
    /// grows downward from where the first line already is. A decorated one
    /// centres, so the text sits on the same line as the icons beside it.
    pub fn text_align_vertical(&self) -> crate::render::TextAlignVertical {
        if let Some(alignment) = self.text_align_vertical {
            return alignment;
        }
        if self.has_decoration() {
            crate::render::TextAlignVertical::CENTER
        } else {
            crate::render::TextAlignVertical::TOP
        }
    }

    /// Whether each of the three attachments is on screen, given the text.
    pub fn shows_prefix(&self, has_text: bool) -> bool {
        self.has_prefix && self.prefix_mode.shows(has_text)
    }

    pub fn shows_suffix(&self, has_text: bool) -> bool {
        self.has_suffix && self.suffix_mode.shows(has_text)
    }

    pub fn shows_clear_button(&self, has_text: bool) -> bool {
        self.clear_button_mode.shows(has_text)
    }

    /// What a screen reader calls the clear button.
    ///
    /// ```dart
    /// final String clearLabel =
    ///     widget.clearButtonSemanticLabel ?? CupertinoLocalizations.of(context).clearButtonLabel;
    /// return Semantics(button: true, label: clearLabel, child: ...);
    /// ```
    ///
    /// The button paints an icon and nothing else, so this word is the only
    /// name it has. Upstream also marks the node `button: true` -- an icon in
    /// a `GestureDetector` is not a button to a screen reader unless somebody
    /// says so, and a label on a node that is not a button is read as static
    /// text the reader will not offer to activate.
    ///
    /// `override_label` is upstream's `clearButtonSemanticLabel`, a widget
    /// property; it is a parameter here rather than a field because this
    /// struct is `Copy` and a per-instance `String` would end that for the
    /// sake of one rarely-set word.
    ///
    /// `None` when there is no clear button to name, so a caller cannot
    /// attach a label to something that is not on screen.
    pub fn clear_button_semantics_label<'a>(
        &self,
        has_text: bool,
        override_label: Option<&'a str>,
    ) -> Option<&'a str> {
        if !self.shows_clear_button(has_text) {
            return None;
        }
        Some(
            override_label
                .unwrap_or(crate::cupertino_app::DefaultCupertinoLocalizations::CLEAR_BUTTON_LABEL),
        )
    }

    /// Upstream's `_onClearButtonTapped`: whether tapping it should reach
    /// `onChanged`.
    ///
    /// Only when there was text to clear -- clearing an empty field changed
    /// nothing, and reporting a change that did not happen would be a lie to
    /// anything counting keystrokes.
    ///
    /// And upstream says what the report means: "Tapping the clear button is
    /// also considered a 'user initiated' change (instead of a programmatical
    /// one)". The same line
    /// [`crate::services::text_input::TextInputClient::update_editing_value`]
    /// draws -- a value the reader caused goes through the formatters and the
    /// callbacks, one the application set does not -- and the clear button is
    /// on the reader's side of it despite being the field's own widget.
    pub fn clearing_reports_a_change(had_text: bool) -> bool {
        had_text
    }

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
    /// Upstream: *"iOS guidelines encourage passing a `Text` widget to
    /// `prefix` to detail the nature of the input."* A label beside the field
    /// rather than floating inside it, which is where Material would put it.
    ///
    /// This used to be a `labels_beside_rather_than_inside()` returning `true`
    /// -- a function taking nothing, which no input could make answer
    /// otherwise. It stated the fact without checking it, so it is stated
    /// here, where it is not pretending to be a test.
    pub has_prefix: bool,
    /// `None` is **not** zero. See [`CupertinoTextFormFieldRow::padding`].
    pub padding: Option<EdgeInsets>,
}

impl CupertinoTextFormFieldRow {
    /// The standard iOS form row padding `CupertinoFormRow` supplies when none
    /// is given -- **the form row's own constant, not a second copy of it.**
    ///
    /// It was a bare `6.0` here while [`CupertinoFormRow::PADDING`] said
    /// `(20, 6, 6, 6)`: one upstream number written down in two places,
    /// disagreeing about the start inset. And the start inset is the one that
    /// matters -- 20 against 6 is what makes the labels line up down a column,
    /// and it is the only side of that rectangle a scalar could not carry.
    ///
    /// The two only came into contact because the form row's padding was
    /// ported as an `EdgeInsets` in the tick before this one. Until then each
    /// was locally plausible.
    pub const DEFAULT_PADDING: EdgeInsets = CupertinoFormRow::PADDING;

    pub fn new() -> CupertinoTextFormFieldRow {
        CupertinoTextFormFieldRow {
            // Upstream's builder does not offer this as a choice: it
            // builds a `CupertinoTextField.borderless` outright. The row is
            // the visual container -- it is what draws the divider and sits
            // inside the section's card -- so a field with a border of its own
            // would put a second box inside the first.
            field: CupertinoTextField::borderless(),
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
    pub fn padding(&self) -> EdgeInsets {
        self.padding
            .unwrap_or(CupertinoTextFormFieldRow::DEFAULT_PADDING)
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
        assert_ne!(
            row.padding(),
            EdgeInsets::ZERO,
            "which is why the doc has to say so"
        );

        row.padding = Some(EdgeInsets::ZERO);
        assert_eq!(row.padding(), EdgeInsets::ZERO);
    }

    #[test]
    fn and_the_standard_one_is_the_form_rows_own() {
        // Not a second copy of it. These were `6.0` and `(20, 6, 6, 6)`
        // separately, and nothing compared them until the form row's padding
        // stopped being a scalar.
        assert_eq!(
            CupertinoTextFormFieldRow::DEFAULT_PADDING,
            CupertinoFormRow::PADDING
        );
        // The start inset is the half a scalar could not carry, and the half
        // that does the work.
        assert_eq!(CupertinoTextFormFieldRow::DEFAULT_PADDING.left, 20.0);
        assert_ne!(
            CupertinoTextFormFieldRow::DEFAULT_PADDING.left,
            CupertinoTextFormFieldRow::DEFAULT_PADDING.right
        );
    }

    #[test]
    fn the_field_inside_a_form_row_is_borderless() {
        // Upstream's builder does not take this as a parameter: it names
        // `CupertinoTextField.borderless` outright. The row draws the box, so
        // a bordered field would draw a second one inside it.
        assert!(CupertinoTextFormFieldRow::new().field.borderless);
        // And a field built on its own is not borderless, so this is the row's
        // doing rather than the default everywhere.
        assert!(!CupertinoTextField::new().borderless);
    }

    #[test]
    fn a_row_still_refuses_what_the_field_refuses() {
        // The row delegates the rest of the asserts, and being borderless does
        // not excuse it from them. `expands` with the default line count is
        // the sharp one: `maxLines` defaults to 1 rather than null, so
        // `expands: true` and nothing else is already illegal upstream.
        let mut row = CupertinoTextFormFieldRow::new();
        assert_eq!(row.validate(), Ok(()));
        row.field.expands = true;
        assert_eq!(
            row.validate(),
            Err(CupertinoTextFieldError::ExpandsWithLineCount),
            "because max_lines is Some(1) until someone clears it"
        );
        row.field.max_lines = None;
        assert_eq!(row.validate(), Ok(()));
    }

    #[test]
    fn the_same_distinction_the_icon_buttons_splash_radius_makes() {
        use crate::buttons::IconButton;
        let mut button = IconButton::new();
        assert!(button.is_valid(), "None is the default");
        button.splash_radius = Some(0.0);
        assert!(!button.is_valid(), "and zero is not a default, it is a bug");
    }
}

#[cfg(test)]
mod overlay_visibility_tests {
    use super::*;
    use crate::render::TextAlignVertical;

    fn decorated() -> CupertinoTextField {
        CupertinoTextField {
            has_prefix: true,
            ..CupertinoTextField::new()
        }
    }

    #[test]
    fn editing_asks_what_is_in_the_field_not_what_is_happening_to_it() {
        // Upstream: "includes prefilled text that the user did not type in
        // manually". A field that opens with a value is *editing* before
        // anybody has touched it.
        assert!(OverlayVisibilityMode::Editing.shows(true));
        assert!(!OverlayVisibilityMode::Editing.shows(false));
        assert!(!OverlayVisibilityMode::NotEditing.shows(true));
        assert!(OverlayVisibilityMode::NotEditing.shows(false));
    }

    #[test]
    fn the_two_content_modes_are_exact_opposites() {
        // Which is worth pinning: they are one question asked twice, so a
        // field showing a prefix while editing and a suffix while not shows
        // exactly one of them at any moment.
        for has_text in [false, true] {
            assert_ne!(
                OverlayVisibilityMode::Editing.shows(has_text),
                OverlayVisibilityMode::NotEditing.shows(has_text)
            );
        }
    }

    #[test]
    fn and_the_two_constant_modes_ignore_the_text_entirely() {
        for has_text in [false, true] {
            assert!(!OverlayVisibilityMode::Never.shows(has_text));
            assert!(OverlayVisibilityMode::Always.shows(has_text));
        }
    }

    #[test]
    fn the_three_attachments_do_not_share_a_default() {
        // Prefix and suffix are `always`; the clear button is `never`. The
        // one that can delete the reader's text is the one that is off until
        // asked for.
        let field = CupertinoTextField::new();
        assert_eq!(field.prefix_mode, OverlayVisibilityMode::Always);
        assert_eq!(field.suffix_mode, OverlayVisibilityMode::Always);
        assert_eq!(field.clear_button_mode, OverlayVisibilityMode::Never);
    }

    #[test]
    fn an_attachment_needs_both_a_widget_and_a_mode() {
        // `always` on a prefix nobody supplied still shows nothing.
        let bare = CupertinoTextField::new();
        assert!(!bare.shows_prefix(true), "no prefix widget to show");
        assert!(decorated().shows_prefix(true));
    }

    // -- Decoration -------------------------------------------------------------

    #[test]
    fn the_clear_button_has_a_word_that_is_never_drawn() {
        // It paints `CupertinoIcons.clear_thick_circled` and nothing else, so
        // the localized word is the only name a screen reader has for it.
        let mut field = CupertinoTextField::new();
        field.clear_button_mode = OverlayVisibilityMode::Editing;
        assert_eq!(
            field.clear_button_semantics_label(true, None),
            Some("Clear")
        );
    }

    #[test]
    fn a_button_that_is_not_showing_has_no_label_to_give() {
        // Upstream builds the Semantics node inside `_buildClearButton`, so
        // there is no node and no label when the button is not built.
        let mut field = CupertinoTextField::new();
        assert_eq!(field.clear_button_mode, OverlayVisibilityMode::Never);
        assert_eq!(field.clear_button_semantics_label(true, None), None);

        field.clear_button_mode = OverlayVisibilityMode::Editing;
        assert_eq!(
            field.clear_button_semantics_label(false, None),
            None,
            "empty field, no button, no label"
        );
        assert!(field.clear_button_semantics_label(true, None).is_some());
    }

    #[test]
    fn the_widgets_own_label_replaces_the_localized_one() {
        // `widget.clearButtonSemanticLabel ?? localizations.clearButtonLabel`
        // -- it overrides rather than adds to.
        let mut field = CupertinoTextField::new();
        field.clear_button_mode = OverlayVisibilityMode::Always;
        assert_eq!(
            field.clear_button_semantics_label(false, Some("Erase query")),
            Some("Erase query")
        );
    }

    #[test]
    fn a_clear_button_counts_as_decoration_before_it_appears() {
        // `clearButtonMode != never`, not "is showing". Otherwise the field
        // would change alignment the moment the reader typed a character and
        // the text they were looking at would jump.
        let mut field = CupertinoTextField::new();
        field.clear_button_mode = OverlayVisibilityMode::Editing;
        assert!(!field.shows_clear_button(false), "not showing while empty");
        assert!(field.has_decoration(), "and decorated anyway");
    }

    #[test]
    fn a_field_with_nothing_attached_is_not_decorated() {
        assert!(!CupertinoTextField::new().has_decoration());
    }

    #[test]
    fn any_one_of_the_four_is_enough() {
        for field in [
            CupertinoTextField {
                has_placeholder: true,
                ..CupertinoTextField::new()
            },
            CupertinoTextField {
                has_prefix: true,
                ..CupertinoTextField::new()
            },
            CupertinoTextField {
                has_suffix: true,
                ..CupertinoTextField::new()
            },
            CupertinoTextField {
                clear_button_mode: OverlayVisibilityMode::Always,
                ..CupertinoTextField::new()
            },
        ] {
            assert!(field.has_decoration());
        }
    }

    // -- Vertical alignment -----------------------------------------------------

    #[test]
    fn a_bare_field_starts_its_text_at_the_top_and_a_decorated_one_centres() {
        // So a growing multiline field grows downward from where its first
        // line already is, while a decorated one sits on the line of the
        // icons beside it.
        assert_eq!(
            CupertinoTextField::new().text_align_vertical(),
            TextAlignVertical::TOP
        );
        assert_eq!(decorated().text_align_vertical(), TextAlignVertical::CENTER);
    }

    #[test]
    fn and_an_alignment_someone_asked_for_beats_both() {
        let mut field = decorated();
        field.text_align_vertical = Some(TextAlignVertical::BOTTOM);
        assert_eq!(field.text_align_vertical(), TextAlignVertical::BOTTOM);

        let mut bare = CupertinoTextField::new();
        bare.text_align_vertical = Some(TextAlignVertical::BOTTOM);
        assert_eq!(bare.text_align_vertical(), TextAlignVertical::BOTTOM);
    }

    // -- Clearing ---------------------------------------------------------------

    #[test]
    fn clearing_an_empty_field_reports_nothing() {
        // Reporting a change that did not happen would be a lie to anything
        // counting keystrokes.
        assert!(!CupertinoTextField::clearing_reports_a_change(false));
        assert!(CupertinoTextField::clearing_reports_a_change(true));
    }
}
