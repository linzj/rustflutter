//! Ports of `material/theme_data.dart`'s two cross-design adapters,
//! `material/text_form_field.dart` and `material/user_accounts_drawer_header.dart`.
//!
//! The last four Material classes, and two of them are the seam between the two
//! design languages.

use crate::platform::Brightness;

/// A Cupertino theme with every default stripped out, upstream's
/// `NoDefaultCupertinoThemeData`.
///
/// Every field is optional, and that is the point: **"unset" has to stay
/// distinguishable from "set to the iOS default"**, because
/// [`MaterialBasedCupertinoThemeData`] falls back to the Material theme on
/// exactly the unset ones. A theme that had already resolved its defaults would
/// short-circuit every `??` on an iOS colour and the Material theme would never
/// be reached.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoDefaultCupertinoThemeData {
    pub brightness: Option<Brightness>,
    pub primary_color: Option<u32>,
    pub primary_contrasting_color: Option<u32>,
    pub scaffold_background_color: Option<u32>,
    pub bar_background_color: Option<u32>,
    pub selection_handle_color: Option<u32>,
}

/// The parts of a Material `ThemeData` these adapters read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialThemeColors {
    pub brightness: Brightness,
    pub color_scheme_primary: u32,
    pub color_scheme_on_primary: u32,
    pub scaffold_background_color: u32,
    pub selection_handle_color: u32,
}

impl MaterialThemeColors {
    pub fn new() -> MaterialThemeColors {
        MaterialThemeColors {
            brightness: Brightness::Light,
            color_scheme_primary: 0xFF6200EE,
            color_scheme_on_primary: 0xFFFFFFFF,
            scaffold_background_color: 0xFFFAFAFA,
            selection_handle_color: 0xFF6200EE,
        }
    }
}

impl Default for MaterialThemeColors {
    fn default() -> Self {
        MaterialThemeColors::new()
    }
}

/// Upstream `MaterialBasedCupertinoThemeData`.
///
/// It **extends `CupertinoThemeData`** and overrides each getter to consult the
/// override first and the Material theme second, at every access. A live view
/// onto two themes, not a conversion of one into the other.
///
/// Its constructor comment explains why it still hands everything to `super.raw`
/// rather than only overriding: *"Pass all values to the superclass so
/// Material-agnostic properties like `barBackgroundColor` can still behave like
/// a normal `CupertinoThemeData`."* The properties Material has no opinion about
/// are left to work the ordinary way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialBasedCupertinoThemeData {
    material_theme: MaterialThemeColors,
    cupertino_override_theme: NoDefaultCupertinoThemeData,
}

impl MaterialBasedCupertinoThemeData {
    /// Upstream's constructor, which calls `.noDefault()` on the override --
    /// `(materialTheme.cupertinoOverrideTheme ?? const CupertinoThemeData()).noDefault()`.
    pub fn new(
        material_theme: MaterialThemeColors,
        cupertino_override_theme: Option<NoDefaultCupertinoThemeData>,
    ) -> MaterialBasedCupertinoThemeData {
        MaterialBasedCupertinoThemeData {
            material_theme,
            cupertino_override_theme: cupertino_override_theme.unwrap_or_default(),
        }
    }

    pub fn brightness(&self) -> Brightness {
        self.cupertino_override_theme
            .brightness
            .unwrap_or(self.material_theme.brightness)
    }

    pub fn primary_color(&self) -> u32 {
        self.cupertino_override_theme
            .primary_color
            .unwrap_or(self.material_theme.color_scheme_primary)
    }

    pub fn primary_contrasting_color(&self) -> u32 {
        self.cupertino_override_theme
            .primary_contrasting_color
            .unwrap_or(self.material_theme.color_scheme_on_primary)
    }

    pub fn scaffold_background_color(&self) -> u32 {
        self.cupertino_override_theme
            .scaffold_background_color
            .unwrap_or(self.material_theme.scaffold_background_color)
    }

    /// Resolved once in the constructor upstream rather than in a getter, since
    /// `super.raw` takes it as a value.
    pub fn selection_handle_color(&self) -> u32 {
        self.cupertino_override_theme
            .selection_handle_color
            .unwrap_or(self.material_theme.selection_handle_color)
    }

    /// A property Material has no opinion about, so nothing falls back.
    pub fn bar_background_color(&self) -> Option<u32> {
        self.cupertino_override_theme.bar_background_color
    }

    /// Upstream's `copyWith`, whose doc is unusually blunt about its limits:
    ///
    /// > Only the specified override attributes [...] are in the returned
    /// > `CupertinoThemeData`. **No derived attributes from iOS defaults or from
    /// > cascaded Material theme attributes are copied.** [...] This `copyWith`
    /// > cannot change the base Material `ThemeData`.
    ///
    /// So copying this theme does not copy what it *answers*, only what it was
    /// *told*. The Material half rides along unchanged because it is the base,
    /// not a value.
    pub fn copy_with(&self, primary_color: Option<u32>) -> MaterialBasedCupertinoThemeData {
        MaterialBasedCupertinoThemeData {
            material_theme: self.material_theme,
            cupertino_override_theme: NoDefaultCupertinoThemeData {
                primary_color: primary_color.or(self.cupertino_override_theme.primary_color),
                ..self.cupertino_override_theme
            },
        }
    }
}

/// Upstream `CupertinoBasedMaterialThemeData`, and the striking thing is what it
/// is *not*.
///
/// Its counterpart above extends `CupertinoThemeData` and answers questions
/// live. **This one extends nothing.** It is a holder with a single field,
/// `materialTheme`, built once in the constructor:
///
/// ```dart
/// CupertinoBasedMaterialThemeData({required CupertinoThemeData themeData})
///   : materialTheme = ThemeData(
///       colorScheme: ColorScheme.fromSeed(
///         seedColor: themeData.primaryColor,
///         brightness: themeData.brightness ?? Brightness.light,
///         primary: themeData.primaryColor,
///         onPrimary: themeData.primaryContrastingColor,
///       ),
///     );
/// ```
///
/// **One direction is a view and the other is a snapshot**, and the asymmetry
/// follows from the data. A Material theme can answer any Cupertino question,
/// because Cupertino asks about a handful of colours it already has. Going the
/// other way, four Cupertino colours cannot answer an arbitrary Material
/// question -- so instead of deferring, it *seeds* a whole `ColorScheme` from
/// the primary colour once and hands you that.
///
/// Note also that the seed is used **and** `primary` is passed explicitly, so
/// the generated scheme is overruled on the one colour the caller actually
/// stated. The seeding fills in everything nobody said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoBasedMaterialThemeData {
    pub material_theme: MaterialThemeColors,
}

impl CupertinoBasedMaterialThemeData {
    pub fn new(
        primary_color: u32,
        primary_contrasting_color: u32,
        brightness: Option<Brightness>,
    ) -> CupertinoBasedMaterialThemeData {
        CupertinoBasedMaterialThemeData {
            material_theme: MaterialThemeColors {
                brightness: brightness.unwrap_or(Brightness::Light),
                color_scheme_primary: primary_color,
                color_scheme_on_primary: primary_contrasting_color,
                ..MaterialThemeColors::new()
            },
        }
    }

    /// Whether this adapter keeps looking at the theme it was built from. It
    /// does not.
    pub fn is_a_live_view() -> bool {
        false
    }
}

/// Why a text form field's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFormFieldError {
    InitialValueWithController,
    ObscuringCharacterNotSingle,
    NonPositiveMaxLines,
    NonPositiveMinLines,
    MinLinesAboveMaxLines,
    ExpandsWithLineCount,
    ObscuredAndMultiline,
    NonPositiveMaxLength,
    TwoWaysToReportAnError,
}

/// Upstream `TextFormField`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextFormField {
    pub has_initial_value: bool,
    pub has_controller: bool,
    pub obscuring_character_len: usize,
    pub max_lines: Option<u32>,
    pub min_lines: Option<u32>,
    pub expands: bool,
    pub obscure_text: bool,
    pub max_length: Option<i32>,
    pub has_error_builder: bool,
    pub has_decoration_error_text: bool,
}

impl TextFormField {
    /// Upstream `TextField.noMaxLength`.
    ///
    /// **A negative number standing for "no limit"**, carved out of the
    /// positivity check as its own clause:
    /// `maxLength == null || maxLength == TextField.noMaxLength || maxLength > 0`.
    /// Null already means "no counter at all", so this second no-limit value
    /// means something else again: *show the counter, count up, never stop*.
    pub const NO_MAX_LENGTH: i32 = -1;

    pub fn new() -> TextFormField {
        TextFormField {
            has_initial_value: false,
            has_controller: false,
            obscuring_character_len: 1,
            max_lines: Some(1),
            min_lines: None,
            expands: false,
            obscure_text: false,
            max_length: None,
            has_error_builder: false,
            has_decoration_error_text: false,
        }
    }

    /// Upstream's nine constructor asserts, in order.
    pub fn validate(&self) -> Result<(), TextFormFieldError> {
        if self.has_initial_value && self.has_controller {
            return Err(TextFormFieldError::InitialValueWithController);
        }
        if self.obscuring_character_len != 1 {
            return Err(TextFormFieldError::ObscuringCharacterNotSingle);
        }
        if self.max_lines == Some(0) {
            return Err(TextFormFieldError::NonPositiveMaxLines);
        }
        if self.min_lines == Some(0) {
            return Err(TextFormFieldError::NonPositiveMinLines);
        }
        if let (Some(max), Some(min)) = (self.max_lines, self.min_lines) {
            if max < min {
                return Err(TextFormFieldError::MinLinesAboveMaxLines);
            }
        }
        if self.expands && (self.max_lines.is_some() || self.min_lines.is_some()) {
            return Err(TextFormFieldError::ExpandsWithLineCount);
        }
        if self.obscure_text && self.max_lines != Some(1) {
            return Err(TextFormFieldError::ObscuredAndMultiline);
        }
        if self
            .max_length
            .is_some_and(|length| length != TextFormField::NO_MAX_LENGTH && length <= 0)
        {
            return Err(TextFormFieldError::NonPositiveMaxLength);
        }
        if self.has_error_builder && self.has_decoration_error_text {
            return Err(TextFormFieldError::TwoWaysToReportAnError);
        }
        Ok(())
    }

    /// Whether the field shows a character counter, and whether it stops you.
    pub fn counter(&self) -> Option<Option<i32>> {
        match self.max_length {
            None => None,
            Some(TextFormField::NO_MAX_LENGTH) => Some(None),
            Some(limit) => Some(Some(limit)),
        }
    }
}

impl Default for TextFormField {
    fn default() -> Self {
        TextFormField::new()
    }
}

/// Upstream `UserAccountsDrawerHeader`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UserAccountsDrawerHeader {
    pub has_on_details_pressed: bool,
    pub is_open: bool,
}

impl UserAccountsDrawerHeader {
    /// Upstream `_kAccountDetailsHeight`, used both as the strip's height and,
    /// squared, as the default icon size.
    pub const ACCOUNT_DETAILS_HEIGHT: f32 = 56.0;

    pub fn new() -> UserAccountsDrawerHeader {
        UserAccountsDrawerHeader {
            has_on_details_pressed: false,
            is_open: false,
        }
    }

    /// Upstream's `build` opens with three asserts, and **two of them are the
    /// same one**:
    ///
    /// ```dart
    /// assert(debugCheckHasDirectionality(context));
    /// assert(debugCheckHasMaterialLocalizations(context));
    /// assert(debugCheckHasMaterialLocalizations(context));
    /// ```
    ///
    /// Harmless -- the check has no side effects and always returns true -- and
    /// worth a line here only because it is the sort of thing a reader stares at
    /// looking for the difference. There isn't one.
    pub fn debug_checks() -> [&'static str; 3] {
        [
            "debugCheckHasDirectionality",
            "debugCheckHasMaterialLocalizations",
            "debugCheckHasMaterialLocalizations",
        ]
    }

    /// The arrow only appears when there is something for it to do.
    pub fn shows_arrow(&self) -> bool {
        self.has_on_details_pressed
    }
}

impl Default for UserAccountsDrawerHeader {
    fn default() -> Self {
        UserAccountsDrawerHeader::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- One direction is a view, the other a snapshot -----------------------------

    #[test]
    fn the_material_based_theme_answers_from_material_wherever_cupertino_is_silent() {
        let material = MaterialThemeColors::new();
        let theme = MaterialBasedCupertinoThemeData::new(material, None);

        assert_eq!(theme.primary_color(), material.color_scheme_primary);
        assert_eq!(
            theme.primary_contrasting_color(),
            material.color_scheme_on_primary
        );
        assert_eq!(theme.brightness(), material.brightness);
    }

    #[test]
    fn and_an_override_that_was_set_wins_at_each_access() {
        let material = MaterialThemeColors::new();
        let theme = MaterialBasedCupertinoThemeData::new(
            material,
            Some(NoDefaultCupertinoThemeData {
                primary_color: Some(0xFF00FF00),
                ..NoDefaultCupertinoThemeData::default()
            }),
        );

        assert_eq!(theme.primary_color(), 0xFF00FF00);
        assert_eq!(
            theme.primary_contrasting_color(),
            material.color_scheme_on_primary,
            "while the ones nobody set still come from Material"
        );
    }

    #[test]
    fn stripping_the_defaults_is_what_lets_the_fallback_happen_at_all() {
        // Every field being optional is the mechanism: a theme carrying resolved
        // iOS defaults would answer every question itself.
        let stripped = NoDefaultCupertinoThemeData::default();
        assert_eq!(stripped.primary_color, None);
        assert_eq!(stripped.brightness, None);

        let material = MaterialThemeColors {
            color_scheme_primary: 0xFF123456,
            ..MaterialThemeColors::new()
        };
        assert_eq!(
            MaterialBasedCupertinoThemeData::new(material, Some(stripped)).primary_color(),
            0xFF123456
        );
    }

    #[test]
    fn a_property_material_has_no_opinion_about_falls_back_to_nothing() {
        let theme = MaterialBasedCupertinoThemeData::new(MaterialThemeColors::new(), None);
        assert_eq!(theme.bar_background_color(), None);
    }

    #[test]
    fn copying_carries_what_it_was_told_not_what_it_answers() {
        let material = MaterialThemeColors::new();
        let theme = MaterialBasedCupertinoThemeData::new(material, None);
        let copied = theme.copy_with(None);

        assert_eq!(
            copied.cupertino_override_theme,
            NoDefaultCupertinoThemeData::default(),
            "the derived primary colour was not copied into the override"
        );
        assert_eq!(
            copied.primary_color(),
            theme.primary_color(),
            "though it still answers the same, because the base is the same"
        );
    }

    #[test]
    fn the_other_direction_is_computed_once_and_stops_looking() {
        assert!(!CupertinoBasedMaterialThemeData::is_a_live_view());

        let derived = CupertinoBasedMaterialThemeData::new(0xFF00FF00, 0xFF000000, None);
        assert_eq!(derived.material_theme.color_scheme_primary, 0xFF00FF00);
        assert_eq!(derived.material_theme.color_scheme_on_primary, 0xFF000000);
        assert_eq!(
            derived.material_theme.brightness,
            Brightness::Light,
            "and a null brightness resolves now rather than staying open"
        );
    }

    #[test]
    fn the_stated_colours_overrule_the_ones_the_seed_generated() {
        // fromSeed is given the primary colour as seed AND as primary, so the
        // generation fills in everything nobody said and nothing they did.
        let derived = CupertinoBasedMaterialThemeData::new(0xFFABCDEF, 0xFF111111, None);
        assert_eq!(derived.material_theme.color_scheme_primary, 0xFFABCDEF);
    }

    #[test]
    fn a_dark_cupertino_theme_makes_a_dark_material_one() {
        let derived =
            CupertinoBasedMaterialThemeData::new(0xFF00FF00, 0xFF000000, Some(Brightness::Dark));
        assert_eq!(derived.material_theme.brightness, Brightness::Dark);
    }

    // -- A negative number meaning no limit ------------------------------------------

    #[test]
    fn there_are_two_different_ways_to_have_no_maximum_length() {
        let mut field = TextFormField::new();
        assert_eq!(field.counter(), None, "null is no counter at all");

        field.max_length = Some(TextFormField::NO_MAX_LENGTH);
        assert_eq!(
            field.counter(),
            Some(None),
            "and -1 is a counter that never stops"
        );
        assert_eq!(field.validate(), Ok(()));

        field.max_length = Some(10);
        assert_eq!(field.counter(), Some(Some(10)));
    }

    #[test]
    fn but_no_other_non_positive_length_is_allowed_through() {
        let mut field = TextFormField::new();
        for length in [0, -2, -100] {
            field.max_length = Some(length);
            assert_eq!(
                field.validate(),
                Err(TextFormFieldError::NonPositiveMaxLength),
                "length {length}"
            );
        }
    }

    // -- What the form field refuses --------------------------------------------------

    #[test]
    fn an_obscured_field_cannot_be_multiline() {
        let mut field = TextFormField::new();
        field.obscure_text = true;
        assert_eq!(field.validate(), Ok(()), "one line is the default");

        field.max_lines = Some(3);
        assert_eq!(
            field.validate(),
            Err(TextFormFieldError::ObscuredAndMultiline)
        );

        field.max_lines = None;
        assert_eq!(
            field.validate(),
            Err(TextFormFieldError::ObscuredAndMultiline),
            "and unbounded counts as multiline too"
        );
    }

    #[test]
    fn expanding_and_counting_lines_are_two_ways_of_saying_the_height() {
        let mut field = TextFormField::new();
        field.expands = true;
        assert_eq!(
            field.validate(),
            Err(TextFormFieldError::ExpandsWithLineCount)
        );

        field.max_lines = None;
        assert_eq!(field.validate(), Ok(()));

        field.min_lines = Some(2);
        assert_eq!(
            field.validate(),
            Err(TextFormFieldError::ExpandsWithLineCount)
        );
    }

    #[test]
    fn a_line_range_has_to_face_the_right_way() {
        let mut field = TextFormField::new();
        field.max_lines = Some(2);
        field.min_lines = Some(5);
        assert_eq!(
            field.validate(),
            Err(TextFormFieldError::MinLinesAboveMaxLines)
        );

        field.min_lines = Some(2);
        assert_eq!(field.validate(), Ok(()), "equal is fine");
    }

    #[test]
    fn the_obscuring_character_is_exactly_one_character() {
        let mut field = TextFormField::new();
        for length in [0, 2, 5] {
            field.obscuring_character_len = length;
            assert_eq!(
                field.validate(),
                Err(TextFormFieldError::ObscuringCharacterNotSingle)
            );
        }
    }

    #[test]
    fn there_is_one_way_to_say_what_is_in_the_field_and_one_way_to_say_what_is_wrong() {
        let mut field = TextFormField::new();
        field.has_initial_value = true;
        field.has_controller = true;
        assert_eq!(
            field.validate(),
            Err(TextFormFieldError::InitialValueWithController)
        );

        let mut other = TextFormField::new();
        other.has_error_builder = true;
        other.has_decoration_error_text = true;
        assert_eq!(
            other.validate(),
            Err(TextFormFieldError::TwoWaysToReportAnError)
        );
    }

    // -- The same assert twice ---------------------------------------------------------

    #[test]
    fn the_localizations_check_is_written_twice_and_there_is_no_difference() {
        let checks = UserAccountsDrawerHeader::debug_checks();
        assert_eq!(checks[1], checks[2]);
        assert_ne!(checks[0], checks[1]);
    }

    #[test]
    fn the_arrow_appears_only_when_there_is_something_for_it_to_do() {
        let mut header = UserAccountsDrawerHeader::new();
        assert!(!header.shows_arrow());
        header.has_on_details_pressed = true;
        assert!(header.shows_arrow());
    }

    #[test]
    fn one_constant_serves_as_both_a_height_and_an_icon_size() {
        assert_eq!(UserAccountsDrawerHeader::ACCOUNT_DETAILS_HEIGHT, 56.0);
    }
}
