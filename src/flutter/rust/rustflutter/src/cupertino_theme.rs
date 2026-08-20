//! Ports of `cupertino/theme.dart`'s `CupertinoThemeData` and
//! `InheritedCupertinoTheme`, and `cupertino/text_theme.dart`'s
//! `CupertinoTextThemeData`.
//!
//! The other half of the seam ported last tick. Last tick's
//! [`crate::theme_bridge::MaterialBasedCupertinoThemeData`] leaned on
//! `noDefault()` without being able to see it; here is the machinery it was
//! leaning on.

use crate::platform::Brightness;
use crate::theme_bridge::NoDefaultCupertinoThemeData;

/// Upstream `CupertinoColors.activeBlue`, the colour the whole default theme is
/// built around.
pub const ACTIVE_BLUE: u32 = 0xFF007AFF;

/// Upstream's private `_CupertinoThemeDefaults`.
///
/// Every field is non-null **except one**. See
/// [`CupertinoThemeDefaults::brightness`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoThemeDefaults {
    /// **`Brightness?` -- nullable, alone among the defaults.**
    ///
    /// Even the defaults have no default brightness, because the real one is not
    /// a constant. `CupertinoTheme.brightnessOf` ends:
    ///
    /// ```dart
    /// return inheritedTheme?.theme.data.brightness ?? MediaQuery.platformBrightnessOf(context);
    /// ```
    ///
    /// **A theme that does not state a brightness means "follow the device",**
    /// and that has to survive every layer between here and the `MediaQuery` --
    /// including the layer whose whole job is filling in what nobody said. So
    /// this one field stays open all the way down.
    pub brightness: Option<Brightness>,
    pub primary_color: u32,
    pub primary_contrasting_color: u32,
    pub bar_background_color: u32,
    pub scaffold_background_color: u32,
    pub selection_handle_color: u32,
    pub apply_theme_to_all: bool,
    pub text_theme_defaults: CupertinoTextThemeDefaults,
}

impl CupertinoThemeDefaults {
    /// Upstream's `_kDefaultTheme`.
    pub fn new() -> CupertinoThemeDefaults {
        CupertinoThemeDefaults {
            brightness: None,
            primary_color: ACTIVE_BLUE,
            primary_contrasting_color: 0xFFFFFFFF,
            bar_background_color: 0xF0F9F9F9,
            scaffold_background_color: 0xFFFFFFFF,
            selection_handle_color: ACTIVE_BLUE,
            apply_theme_to_all: false,
            text_theme_defaults: CupertinoTextThemeDefaults::new(),
        }
    }

    /// Upstream `resolveFrom`, which runs every colour through
    /// `CupertinoDynamicColor.resolve` and leaves `brightness` and
    /// `applyThemeToAll` alone -- neither is a colour, so neither has anything
    /// to resolve.
    ///
    /// The text theme defaults are resolved **conditionally**, on a flag the
    /// caller passes, because a text theme the caller supplied has already been
    /// resolved on its own.
    pub fn resolve_from(&self, dark: bool, resolve_text_theme: bool) -> CupertinoThemeDefaults {
        CupertinoThemeDefaults {
            brightness: self.brightness,
            primary_color: resolve_dynamic(self.primary_color, dark),
            primary_contrasting_color: resolve_dynamic(self.primary_contrasting_color, dark),
            bar_background_color: resolve_dynamic(self.bar_background_color, dark),
            scaffold_background_color: resolve_dynamic(self.scaffold_background_color, dark),
            selection_handle_color: resolve_dynamic(self.selection_handle_color, dark),
            apply_theme_to_all: self.apply_theme_to_all,
            text_theme_defaults: if resolve_text_theme {
                self.text_theme_defaults.resolve_from(dark)
            } else {
                self.text_theme_defaults
            },
        }
    }
}

impl Default for CupertinoThemeDefaults {
    fn default() -> Self {
        CupertinoThemeDefaults::new()
    }
}

/// A stand-in for `CupertinoDynamicColor.resolve`: a dynamic colour picks its
/// dark variant under a dark brightness, and a plain colour is itself.
pub fn resolve_dynamic(color: u32, dark: bool) -> u32 {
    if dark && color == ACTIVE_BLUE {
        0xFF0A84FF
    } else {
        color
    }
}

/// Upstream's private `_CupertinoTextThemeDefaults`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoTextThemeDefaults {
    pub label_color: u32,
    pub inactive_gray: u32,
}

impl CupertinoTextThemeDefaults {
    pub fn new() -> CupertinoTextThemeDefaults {
        CupertinoTextThemeDefaults {
            label_color: 0xFF000000,
            inactive_gray: 0xFF999999,
        }
    }

    pub fn resolve_from(&self, dark: bool) -> CupertinoTextThemeDefaults {
        CupertinoTextThemeDefaults {
            label_color: if dark { 0xFFFFFFFF } else { self.label_color },
            inactive_gray: self.inactive_gray,
        }
    }
}

impl Default for CupertinoTextThemeDefaults {
    fn default() -> Self {
        CupertinoTextThemeDefaults::new()
    }
}

/// Upstream `CupertinoTextThemeData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoTextThemeData {
    /// `None` cascades from the theme's `primaryColor`, per the class doc on
    /// [`CupertinoThemeData`].
    pub primary_color: Option<u32>,
    pub defaults: CupertinoTextThemeDefaults,
}

impl CupertinoTextThemeData {
    pub fn new() -> CupertinoTextThemeData {
        CupertinoTextThemeData {
            primary_color: None,
            defaults: CupertinoTextThemeDefaults::new(),
        }
    }

    /// The colour an action-styled label ends up with, which is where the
    /// cascade shows: **a theme given only a `primaryColor` still changes the
    /// text**, because the text theme it did not specify inherits it.
    pub fn action_text_color(&self, theme_primary_color: u32) -> u32 {
        self.primary_color.unwrap_or(theme_primary_color)
    }

    pub fn text_color(&self) -> u32 {
        self.defaults.label_color
    }
}

impl Default for CupertinoTextThemeData {
    fn default() -> Self {
        CupertinoTextThemeData::new()
    }
}

/// Upstream `CupertinoThemeData`.
///
/// Declared `class CupertinoThemeData extends NoDefaultCupertinoThemeData`, and
/// the direction of that inheritance is the thing to hold on to: **the class
/// *with* defaults is the subclass of the one without.**
///
/// Which is right -- filling in defaults is behaviour added, not taken away --
/// but it means that inside this class `super.primaryColor` is what the caller
/// actually said and `self.primary_color()` is what it answers, and every getter
/// here is written as one in terms of the other:
///
/// ```dart
/// Color get primaryColor => super.primaryColor ?? _defaults.primaryColor;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoThemeData {
    /// The `super` half: exactly what the caller stated, nothing filled in.
    pub stated: NoDefaultCupertinoThemeData,
    pub defaults: CupertinoThemeDefaults,
    pub text_theme: Option<CupertinoTextThemeData>,
}

impl CupertinoThemeData {
    pub fn new() -> CupertinoThemeData {
        CupertinoThemeData {
            stated: NoDefaultCupertinoThemeData::default(),
            defaults: CupertinoThemeDefaults::new(),
            text_theme: None,
        }
    }

    pub fn with_primary_color(primary_color: u32) -> CupertinoThemeData {
        CupertinoThemeData {
            stated: NoDefaultCupertinoThemeData {
                primary_color: Some(primary_color),
                ..NoDefaultCupertinoThemeData::default()
            },
            ..CupertinoThemeData::new()
        }
    }

    /// Upstream's `brightness` getter is the one that **does not** fall back to
    /// a default, because the defaults object has none. An unstated brightness
    /// stays unstated, and `CupertinoTheme.brightnessOf` resolves it against the
    /// `MediaQuery` at the point of use.
    pub fn brightness(&self) -> Option<Brightness> {
        self.stated.brightness.or(self.defaults.brightness)
    }

    /// Upstream's `CupertinoTheme.brightnessOf`.
    pub fn brightness_of(&self, platform_brightness: Brightness) -> Brightness {
        self.brightness().unwrap_or(platform_brightness)
    }

    pub fn primary_color(&self) -> u32 {
        self.stated
            .primary_color
            .unwrap_or(self.defaults.primary_color)
    }

    pub fn primary_contrasting_color(&self) -> u32 {
        self.stated
            .primary_contrasting_color
            .unwrap_or(self.defaults.primary_contrasting_color)
    }

    pub fn bar_background_color(&self) -> u32 {
        self.stated
            .bar_background_color
            .unwrap_or(self.defaults.bar_background_color)
    }

    pub fn scaffold_background_color(&self) -> u32 {
        self.stated
            .scaffold_background_color
            .unwrap_or(self.defaults.scaffold_background_color)
    }

    pub fn selection_handle_color(&self) -> u32 {
        self.stated
            .selection_handle_color
            .unwrap_or(self.defaults.selection_handle_color)
    }

    /// The class doc: *"Parameters can also be partially specified, in which
    /// case some parameters will cascade down to other dependent parameters to
    /// create a cohesive visual effect. For instance, if a `primaryColor` is
    /// specified, it would cascade down to affect some fonts in `textTheme` if
    /// `textTheme` is not specified."*
    ///
    /// So a partly-filled theme is not "the parts you gave plus stock defaults
    /// for the rest" -- **what you gave changes what the rest defaults to.**
    pub fn effective_text_theme(&self) -> CupertinoTextThemeData {
        let mut text_theme = self.text_theme.unwrap_or_default();
        if text_theme.primary_color.is_none() {
            text_theme.primary_color = Some(self.primary_color());
        }
        text_theme
    }

    /// Upstream `noDefault`, which is where last tick's Material adapter got its
    /// override from:
    ///
    /// ```dart
    /// NoDefaultCupertinoThemeData noDefault() {
    ///   return NoDefaultCupertinoThemeData(
    ///     brightness: super.brightness,
    ///     primaryColor: super.primaryColor,
    ///     ...
    /// ```
    ///
    /// **Every field read through `super.`, deliberately bypassing this class's
    /// own defaulting getters.** It answers "what was I told", not "what do I
    /// say" -- and that distinction is the entire reason
    /// [`crate::theme_bridge::MaterialBasedCupertinoThemeData`] can fall through
    /// to a Material theme at all. Strip the defaults and the holes reappear for
    /// somebody else to fill.
    ///
    /// The base class's own `noDefault()` is `=> this`: it never had any to
    /// remove.
    pub fn no_default(&self) -> NoDefaultCupertinoThemeData {
        self.stated
    }

    /// Upstream `resolveFrom`, and it mixes `super.` and plain access **field by
    /// field**: the colours are read through `super.` and resolved, while
    /// `brightness` and `applyThemeToAll` are read plainly.
    ///
    /// Not an inconsistency. A colour is resolved by being handed a context, so
    /// what must be preserved is whether the caller stated one -- resolve a
    /// default here and it is baked in, and `noDefault()` afterwards would hand
    /// out a value nobody asked for. Brightness and the flag have nothing to
    /// resolve, so the defaulted reading is simply the useful one.
    pub fn resolve_from(&self, dark: bool) -> CupertinoThemeData {
        CupertinoThemeData {
            stated: NoDefaultCupertinoThemeData {
                brightness: self.stated.brightness,
                primary_color: self.stated.primary_color.map(|c| resolve_dynamic(c, dark)),
                primary_contrasting_color: self
                    .stated
                    .primary_contrasting_color
                    .map(|c| resolve_dynamic(c, dark)),
                bar_background_color: self
                    .stated
                    .bar_background_color
                    .map(|c| resolve_dynamic(c, dark)),
                scaffold_background_color: self
                    .stated
                    .scaffold_background_color
                    .map(|c| resolve_dynamic(c, dark)),
                selection_handle_color: self
                    .stated
                    .selection_handle_color
                    .map(|c| resolve_dynamic(c, dark)),
            },
            defaults: self.defaults.resolve_from(dark, self.text_theme.is_none()),
            text_theme: self.text_theme,
        }
    }
}

impl Default for CupertinoThemeData {
    fn default() -> Self {
        CupertinoThemeData::new()
    }
}

/// Upstream `InheritedCupertinoTheme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InheritedCupertinoTheme {
    pub theme: CupertinoThemeData,
    pub child: u64,
}

impl InheritedCupertinoTheme {
    pub fn new(theme: CupertinoThemeData, child: u64) -> InheritedCupertinoTheme {
        InheritedCupertinoTheme { theme, child }
    }

    /// Upstream `updateShouldNotify`, which compares
    /// `theme.data != oldWidget.theme.data` -- **the data, not the widget.**
    /// A rebuilt `CupertinoTheme` carrying an equal `CupertinoThemeData` wakes
    /// nobody.
    pub fn update_should_notify(&self, old: &InheritedCupertinoTheme) -> bool {
        self.theme != old.theme
    }

    /// Upstream `wrap`, an `InheritedTheme` requirement: it rebuilds a
    /// `CupertinoTheme` around the child so the theme survives being carried
    /// into a route that is not a descendant.
    pub fn wrap(&self, child: u64) -> InheritedCupertinoTheme {
        InheritedCupertinoTheme::new(self.theme, child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The one field that stays open all the way down ----------------------------

    #[test]
    fn even_the_defaults_have_no_default_brightness() {
        assert_eq!(CupertinoThemeDefaults::new().brightness, None);
        assert_eq!(CupertinoThemeData::new().brightness(), None);
    }

    #[test]
    fn an_unstated_brightness_means_follow_the_device() {
        let theme = CupertinoThemeData::new();
        assert_eq!(theme.brightness_of(Brightness::Dark), Brightness::Dark);
        assert_eq!(theme.brightness_of(Brightness::Light), Brightness::Light);
    }

    #[test]
    fn and_a_stated_one_overrules_the_device() {
        let mut theme = CupertinoThemeData::new();
        theme.stated.brightness = Some(Brightness::Light);
        assert_eq!(
            theme.brightness_of(Brightness::Dark),
            Brightness::Light,
            "an app that insists on light stays light on a dark phone"
        );
    }

    #[test]
    fn every_other_default_is_a_value_rather_than_a_hole() {
        let theme = CupertinoThemeData::new();
        assert_eq!(theme.primary_color(), ACTIVE_BLUE);
        assert_eq!(theme.primary_contrasting_color(), 0xFFFFFFFF);
        assert_eq!(theme.scaffold_background_color(), 0xFFFFFFFF);
    }

    // -- What I was told, not what I say ---------------------------------------------

    #[test]
    fn no_default_hands_back_the_holes_rather_than_the_answers() {
        let theme = CupertinoThemeData::new();
        assert_eq!(theme.primary_color(), ACTIVE_BLUE, "it answers blue");
        assert_eq!(
            theme.no_default().primary_color,
            None,
            "and reports that nobody said so"
        );
    }

    #[test]
    fn which_is_exactly_what_last_ticks_material_adapter_needed() {
        use crate::theme_bridge::{MaterialBasedCupertinoThemeData, MaterialThemeColors};

        let material = MaterialThemeColors {
            color_scheme_primary: 0xFF123456,
            ..MaterialThemeColors::new()
        };
        let cupertino = CupertinoThemeData::new();

        let bridged = MaterialBasedCupertinoThemeData::new(material, Some(cupertino.no_default()));
        assert_eq!(
            bridged.primary_color(),
            0xFF123456,
            "the hole let Material through"
        );

        // Had the defaults come along, iOS blue would have won and the Material
        // theme would never have been consulted.
        assert_eq!(cupertino.primary_color(), ACTIVE_BLUE);
        assert_ne!(cupertino.primary_color(), material.color_scheme_primary);
    }

    #[test]
    fn a_stated_colour_survives_the_stripping() {
        let theme = CupertinoThemeData::with_primary_color(0xFFFF0000);
        assert_eq!(theme.no_default().primary_color, Some(0xFFFF0000));
        assert_eq!(theme.primary_color(), 0xFFFF0000);
    }

    // -- Partial specification cascades ----------------------------------------------

    #[test]
    fn giving_only_a_primary_colour_still_changes_the_text() {
        // The class doc's own example.
        let theme = CupertinoThemeData::with_primary_color(0xFFFF0000);
        assert_eq!(
            theme.effective_text_theme().action_text_color(0),
            0xFFFF0000
        );
    }

    #[test]
    fn but_a_text_theme_that_states_its_own_colour_is_left_alone() {
        let mut theme = CupertinoThemeData::with_primary_color(0xFFFF0000);
        theme.text_theme = Some(CupertinoTextThemeData {
            primary_color: Some(0xFF00FF00),
            ..CupertinoTextThemeData::new()
        });
        assert_eq!(
            theme.effective_text_theme().action_text_color(0),
            0xFF00FF00
        );
    }

    // -- resolveFrom mixes the two readings on purpose --------------------------------

    #[test]
    fn resolving_a_theme_leaves_the_holes_as_holes() {
        // Resolve a default here and it would be baked in, and noDefault()
        // afterwards would hand out a value nobody asked for.
        let resolved = CupertinoThemeData::new().resolve_from(true);
        assert_eq!(resolved.no_default().primary_color, None);
        assert_eq!(
            resolved.primary_color(),
            0xFF0A84FF,
            "while it now answers with the dark variant"
        );
    }

    #[test]
    fn a_stated_dynamic_colour_is_resolved_in_place() {
        let theme = CupertinoThemeData::with_primary_color(ACTIVE_BLUE);
        let resolved = theme.resolve_from(true);
        assert_eq!(resolved.no_default().primary_color, Some(0xFF0A84FF));
    }

    #[test]
    fn brightness_is_carried_across_resolution_untouched() {
        let mut theme = CupertinoThemeData::new();
        theme.stated.brightness = Some(Brightness::Light);
        assert_eq!(
            theme.resolve_from(true).brightness(),
            Some(Brightness::Light)
        );
        assert_eq!(
            CupertinoThemeData::new().resolve_from(true).brightness(),
            None
        );
    }

    #[test]
    fn a_supplied_text_theme_is_not_resolved_by_the_defaults_object() {
        // It resolves itself; the flag exists so it is not done twice.
        let plain = CupertinoThemeData::new();
        let resolved_defaults = plain.resolve_from(true);
        assert_eq!(
            resolved_defaults.defaults.text_theme_defaults.label_color,
            0xFFFFFFFF
        );

        let mut supplied = CupertinoThemeData::new();
        supplied.text_theme = Some(CupertinoTextThemeData::new());
        assert_eq!(
            supplied
                .resolve_from(true)
                .defaults
                .text_theme_defaults
                .label_color,
            0xFF000000,
            "left alone"
        );
    }

    // -- The inherited widget ----------------------------------------------------------

    #[test]
    fn an_equal_theme_wakes_nobody_however_often_the_widget_is_rebuilt() {
        let theme = CupertinoThemeData::with_primary_color(0xFF112233);
        let first = InheritedCupertinoTheme::new(theme, 1);
        let rebuilt = InheritedCupertinoTheme::new(theme, 2);
        assert!(
            !first.update_should_notify(&rebuilt),
            "the data is compared, not the widget"
        );
    }

    #[test]
    fn a_changed_theme_does_notify() {
        let first = InheritedCupertinoTheme::new(CupertinoThemeData::new(), 1);
        let changed =
            InheritedCupertinoTheme::new(CupertinoThemeData::with_primary_color(0xFF112233), 1);
        assert!(first.update_should_notify(&changed));
    }

    #[test]
    fn wrapping_carries_the_theme_to_a_child_that_is_not_a_descendant() {
        let theme = CupertinoThemeData::with_primary_color(0xFF112233);
        let wrapped = InheritedCupertinoTheme::new(theme, 1).wrap(99);
        assert_eq!(wrapped.theme, theme);
        assert_eq!(wrapped.child, 99);
    }
}
