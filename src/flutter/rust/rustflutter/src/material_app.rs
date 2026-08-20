//! Ports of `material/app.dart` and `material/material_localizations.dart`.
//!
//! The application at the top of a Material tree, the scroll behaviour it
//! installs, and the strings it falls back to.

/// The platforms `MaterialScrollBehavior` distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollPlatform {
    Android,
    Fuchsia,
    IOS,
    Linux,
    MacOS,
    Windows,
}

impl ScrollPlatform {
    /// The three upstream treats as desktop in `buildScrollbar`.
    pub fn is_desktop(self) -> bool {
        matches!(
            self,
            ScrollPlatform::Linux | ScrollPlatform::MacOS | ScrollPlatform::Windows
        )
    }
}

/// Which axis a scrollable runs along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAxis {
    Horizontal,
    Vertical,
}

/// What `buildOverscrollIndicator` wraps a scrollable in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverscrollDecoration {
    /// Upstream's `AndroidOverscrollIndicator.stretch`, the Material 3 default.
    Stretching,
    /// Upstream's `AndroidOverscrollIndicator.glow`.
    Glowing,
    /// The child, undecorated.
    None,
}

/// Upstream `MaterialScrollBehavior`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialScrollBehavior;

impl MaterialScrollBehavior {
    /// Upstream `buildScrollbar`, returning whether a `Scrollbar` is wrapped
    /// around the child.
    ///
    /// Two things fall out of the switch that are easier to see stated:
    ///
    /// * **A horizontal scrollable never gets a scrollbar**, on any platform.
    ///   The `Axis.horizontal` arm returns the child before the platform is
    ///   even consulted.
    /// * Of the vertical ones, only the three desktops get one. On a touch
    ///   platform you move a list by dragging the list, so a bar to grab would
    ///   be furniture nobody uses.
    ///
    /// The `assert(details.controller != null)` sits inside the desktop arm
    /// alone: a scrollbar needs something to attach to, and the platforms that
    /// do not build one have no such requirement.
    pub fn builds_scrollbar(axis: ScrollAxis, platform: ScrollPlatform) -> bool {
        match axis {
            ScrollAxis::Horizontal => false,
            ScrollAxis::Vertical => platform.is_desktop(),
        }
    }

    /// Whether a controller is required for this combination -- exactly the
    /// cases that build a scrollbar.
    pub fn requires_controller(axis: ScrollAxis, platform: ScrollPlatform) -> bool {
        MaterialScrollBehavior::builds_scrollbar(axis, platform)
    }

    /// Upstream `buildOverscrollIndicator`.
    ///
    /// **Only Android is decorated.** Which reads oddly until you line it up
    /// with the physics: iOS scrollables bounce, and the bounce *is* the
    /// feedback that you have reached the end. Android's clamp instead, so
    /// something has to be drawn to say so. The desktops scroll under a cursor
    /// and get nothing either.
    ///
    /// So the platforms that get an overscroll indicator are exactly the ones
    /// whose physics do not already show you the edge.
    ///
    /// `useMaterial3` chooses between the two Android treatments, and the
    /// stretch is the newer one.
    pub fn overscroll_indicator(
        platform: ScrollPlatform,
        use_material3: bool,
    ) -> OverscrollDecoration {
        match platform {
            ScrollPlatform::Android if use_material3 => OverscrollDecoration::Stretching,
            ScrollPlatform::Android => OverscrollDecoration::Glowing,
            _ => OverscrollDecoration::None,
        }
    }

    /// Both methods carry the same note -- *"When modifying this function,
    /// consider modifying the implementation in the base class `ScrollBehavior`
    /// as well."* -- which is a duplication upstream has chosen to flag rather
    /// than factor out.
    pub fn mirrors_the_base_class() -> bool {
        true
    }
}

/// Upstream `MaterialApp`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialApp {
    pub has_home: bool,
    pub has_routes: bool,
    pub has_on_generate_route: bool,
    pub has_router_delegate: bool,
    pub has_router_config: bool,
    /// Upstream's `debugShowMaterialGrid`.
    pub debug_show_material_grid: bool,
    pub debug_show_checked_mode_banner: bool,
}

impl MaterialApp {
    /// Upstream's `GridPaper` interval: the Material 8dp grid.
    pub const MATERIAL_GRID_INTERVAL: f32 = 8.0;

    pub fn new() -> MaterialApp {
        MaterialApp {
            has_home: true,
            has_routes: false,
            has_on_generate_route: false,
            has_router_delegate: false,
            has_router_config: false,
            debug_show_material_grid: false,
            debug_show_checked_mode_banner: true,
        }
    }

    /// Upstream's `MaterialApp.router` constructor:
    /// `assert(routerDelegate != null || routerConfig != null)`.
    ///
    /// An **at least one**, which is a third shape again after the exclusions
    /// and the implication -- a router with neither has no way to turn a URL
    /// into a screen.
    pub fn router_is_configured(&self) -> bool {
        self.has_router_delegate || self.has_router_config
    }

    /// Upstream wraps the app in a `GridPaper` **inside an `assert(() { ... }())`
    /// block**, so the overlay is not merely defaulted off in release -- the
    /// code that draws it is not there at all.
    pub fn shows_material_grid(&self, debug_build: bool) -> bool {
        debug_build && self.debug_show_material_grid
    }
}

impl Default for MaterialApp {
    fn default() -> Self {
        MaterialApp::new()
    }
}

/// Upstream `MaterialLocalizations`: the interface every Material widget reads
/// its strings through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialLocalizations;

impl MaterialLocalizations {
    /// Upstream's `of` asserts `debugCheckHasMaterialLocalizations` and then
    /// force-unwraps, so a missing delegate is a debug error with an
    /// explanation rather than a null in release.
    pub fn of(present: bool) -> Option<MaterialLocalizations> {
        present.then_some(MaterialLocalizations)
    }
}

/// Upstream `DefaultMaterialLocalizations`, whose own doc says what it is:
/// *"Constructs an object that defines the material widgets' localized strings
/// for **US English (only)**."*
///
/// It `implements MaterialLocalizations` rather than extending it, so every
/// member has to be written out and a new one added upstream breaks this class
/// loudly rather than inheriting something wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultMaterialLocalizations;

impl DefaultMaterialLocalizations {
    /// Upstream's `_shortWeekdays`, and above it -- twice, once for this list
    /// and once for the long one -- sits this comment:
    ///
    /// ```dart
    /// // Ordered to match DateTime.monday=1, DateTime.sunday=6
    /// ```
    ///
    /// **`DateTime.sunday` is 7.** The list is seven entries indexed by
    /// `date.weekday - DateTime.monday`, so Sunday lands at *index* 6, and the
    /// comment has written the index where it names the constant.
    ///
    /// The code is right and only the comment is wrong, which is the kind of
    /// wrong that costs a reader real time: taken at its word it gives
    /// `sunday - monday = 6 - 1 = 5`, and index 5 is Saturday.
    pub const SHORT_WEEKDAYS: [&'static str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    pub const SHORT_MONTHS: [&'static str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    /// Dart's `DateTime.monday`.
    pub const MONDAY: u32 = 1;
    /// Dart's `DateTime.sunday`, which is what the comment above got wrong.
    pub const SUNDAY: u32 = 7;

    /// Upstream's indexing, `_shortWeekdays[date.weekday - DateTime.monday]`.
    pub fn short_weekday(weekday: u32) -> &'static str {
        DefaultMaterialLocalizations::SHORT_WEEKDAYS
            [(weekday - DefaultMaterialLocalizations::MONDAY) as usize]
    }

    /// Upstream `_formatTwoDigitZeroPad`, which asserts its own range:
    /// *"Formats `number` using two digits, assuming it's in the 0-99 inclusive
    /// range. Not designed to format values outside this range."*
    pub fn format_two_digit_zero_pad(number: u32) -> Option<String> {
        if number >= 100 {
            return None;
        }
        Some(format!("{number:02}"))
    }

    /// Upstream `formatMinute`, which does the same padding **inline** rather
    /// than calling the helper above -- so the two-digit rule exists twice in
    /// one class, once with a range assert and once without.
    pub fn format_minute(minute: u32) -> String {
        if minute < 10 {
            format!("0{minute}")
        } else {
            minute.to_string()
        }
    }

    /// Upstream `formatCompactDate`, carrying the comment
    /// `// Assumes US mm/dd/yyyy format` -- the default localization saying out
    /// loud that it is one locale rather than a neutral one.
    ///
    /// Note the year takes a different route: month and day go through the
    /// 0-99 helper, and the year is padded with `padLeft(4, '0')` because it is
    /// not a two-digit number.
    pub fn format_compact_date(year: i32, month: u32, day: u32) -> Option<String> {
        let month = DefaultMaterialLocalizations::format_two_digit_zero_pad(month)?;
        let day = DefaultMaterialLocalizations::format_two_digit_zero_pad(day)?;
        Some(format!("{month}/{day}/{year:04}"))
    }

    /// Upstream `tabLabel`, which asserts `tabIndex >= 1` and `tabCount >= 1`.
    ///
    /// **One-based, where [`crate::tabs::TabController::index`] is zero-based.**
    /// The number a reader hears is not the number the code counts with, and
    /// the assert is what stops the two conventions being confused at the
    /// boundary: passing the controller's index straight through would announce
    /// "Tab 0 of 3" and trip on the very first tab.
    pub fn tab_label(tab_index: u32, tab_count: u32) -> Option<String> {
        if tab_index < 1 || tab_count < 1 {
            return None;
        }
        Some(format!("Tab {tab_index} of {tab_count}"))
    }

    /// Upstream `licensesPackageDetailText`'s `assert(licenseCount >= 0)`,
    /// which in Dart is a real check and here is the type.
    pub fn license_count_is_valid(_count: u32) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ScrollPlatform; 6] = [
        ScrollPlatform::Android,
        ScrollPlatform::Fuchsia,
        ScrollPlatform::IOS,
        ScrollPlatform::Linux,
        ScrollPlatform::MacOS,
        ScrollPlatform::Windows,
    ];

    // -- What gets decorated and what does not -------------------------------------

    #[test]
    fn a_horizontal_scrollable_never_gets_a_scrollbar_on_any_platform() {
        for platform in ALL {
            assert!(
                !MaterialScrollBehavior::builds_scrollbar(ScrollAxis::Horizontal, platform),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn but_a_vertical_one_does_on_the_three_desktops() {
        for platform in ALL {
            assert_eq!(
                MaterialScrollBehavior::builds_scrollbar(ScrollAxis::Vertical, platform),
                platform.is_desktop(),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_controller_is_demanded_exactly_where_a_scrollbar_is_built() {
        // The assert lives inside the desktop arm, not above the switch.
        for platform in ALL {
            for axis in [ScrollAxis::Horizontal, ScrollAxis::Vertical] {
                assert_eq!(
                    MaterialScrollBehavior::requires_controller(axis, platform),
                    MaterialScrollBehavior::builds_scrollbar(axis, platform),
                    "{platform:?} {axis:?}"
                );
            }
        }
    }

    #[test]
    fn only_the_platform_whose_physics_hide_the_edge_is_told_where_the_edge_is() {
        // iOS bounces, and the bounce is the feedback. Android clamps.
        for platform in ALL {
            let decoration = MaterialScrollBehavior::overscroll_indicator(platform, true);
            if platform == ScrollPlatform::Android {
                assert_eq!(decoration, OverscrollDecoration::Stretching);
            } else {
                assert_eq!(decoration, OverscrollDecoration::None, "{platform:?}");
            }
        }
    }

    #[test]
    fn material_three_stretches_where_material_two_glowed() {
        assert_eq!(
            MaterialScrollBehavior::overscroll_indicator(ScrollPlatform::Android, true),
            OverscrollDecoration::Stretching
        );
        assert_eq!(
            MaterialScrollBehavior::overscroll_indicator(ScrollPlatform::Android, false),
            OverscrollDecoration::Glowing
        );
        assert_eq!(
            MaterialScrollBehavior::overscroll_indicator(ScrollPlatform::IOS, false),
            OverscrollDecoration::None,
            "and the choice does not reach the other platforms at all"
        );
    }

    // -- The comment that is wrong about a constant --------------------------------

    #[test]
    fn sunday_is_seven_and_lands_at_index_six() {
        assert_eq!(DefaultMaterialLocalizations::SUNDAY, 7);
        assert_eq!(
            DefaultMaterialLocalizations::SUNDAY - DefaultMaterialLocalizations::MONDAY,
            6
        );
        assert_eq!(DefaultMaterialLocalizations::short_weekday(7), "Sun");
    }

    #[test]
    fn reading_the_comment_literally_gives_the_wrong_day() {
        // "Ordered to match DateTime.monday=1, DateTime.sunday=6" -- taken at
        // its word, 6 - 1 = 5, and index 5 is Saturday.
        assert_eq!(DefaultMaterialLocalizations::SHORT_WEEKDAYS[5], "Sat");
        assert_eq!(DefaultMaterialLocalizations::SHORT_WEEKDAYS[6], "Sun");
        assert_ne!(
            DefaultMaterialLocalizations::SHORT_WEEKDAYS[5],
            DefaultMaterialLocalizations::short_weekday(7)
        );
    }

    #[test]
    fn every_weekday_maps_to_its_own_name() {
        let names: Vec<&str> = (1..=7)
            .map(DefaultMaterialLocalizations::short_weekday)
            .collect();
        assert_eq!(names, vec!["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
    }

    // -- One-based where the controller is zero-based -------------------------------

    #[test]
    fn the_number_a_reader_hears_is_not_the_number_the_code_counts_with() {
        assert_eq!(
            DefaultMaterialLocalizations::tab_label(1, 3).as_deref(),
            Some("Tab 1 of 3")
        );
        assert_eq!(
            DefaultMaterialLocalizations::tab_label(0, 3),
            None,
            "which is what a controller's index would have handed it"
        );
        assert_eq!(DefaultMaterialLocalizations::tab_label(1, 0), None);
    }

    // -- Formatting -----------------------------------------------------------------

    #[test]
    fn the_two_digit_helper_is_only_for_two_digit_numbers() {
        assert_eq!(
            DefaultMaterialLocalizations::format_two_digit_zero_pad(7).as_deref(),
            Some("07")
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_two_digit_zero_pad(99).as_deref(),
            Some("99")
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_two_digit_zero_pad(100),
            None
        );
    }

    #[test]
    fn the_same_padding_rule_is_written_twice_in_one_class() {
        // formatMinute inlines it rather than calling the helper, so it has no
        // range assert of its own.
        for minute in [0, 5, 9, 10, 59] {
            assert_eq!(
                DefaultMaterialLocalizations::format_minute(minute),
                DefaultMaterialLocalizations::format_two_digit_zero_pad(minute).unwrap(),
                "minute {minute}"
            );
        }
    }

    #[test]
    fn the_default_localization_is_one_locale_and_says_so() {
        assert_eq!(
            DefaultMaterialLocalizations::format_compact_date(2026, 8, 20).as_deref(),
            Some("08/20/2026"),
            "US mm/dd/yyyy, per the comment above it"
        );
    }

    #[test]
    fn the_year_is_padded_by_a_different_mechanism_because_it_is_not_two_digits() {
        assert_eq!(
            DefaultMaterialLocalizations::format_compact_date(7, 1, 2).as_deref(),
            Some("01/02/0007")
        );
    }

    // -- The application ------------------------------------------------------------

    #[test]
    fn a_router_needs_at_least_one_of_the_two_ways_to_configure_it() {
        let mut app = MaterialApp::new();
        assert!(!app.router_is_configured());

        app.has_router_delegate = true;
        assert!(app.router_is_configured());

        app.has_router_config = true;
        assert!(app.router_is_configured(), "and both is allowed");

        app.has_router_delegate = false;
        assert!(app.router_is_configured());
    }

    #[test]
    fn the_material_grid_is_not_merely_off_in_release_it_is_absent() {
        let mut app = MaterialApp::new();
        app.debug_show_material_grid = true;
        assert!(app.shows_material_grid(true));
        assert!(
            !app.shows_material_grid(false),
            "the GridPaper lives inside an assert block"
        );
        assert_eq!(MaterialApp::MATERIAL_GRID_INTERVAL, 8.0);
    }

    #[test]
    fn localizations_are_fetched_through_a_check_rather_than_a_null() {
        assert!(MaterialLocalizations::of(true).is_some());
        assert!(MaterialLocalizations::of(false).is_none());
    }
}
