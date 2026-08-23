//! Ports of `cupertino/app.dart`'s `CupertinoApp` and `CupertinoScrollBehavior`,
//! and `cupertino/localizations.dart`'s `CupertinoLocalizations` and
//! `DefaultCupertinoLocalizations`.
//!
//! Tick 89 ported the Material four. These are their opposite numbers, and
//! reading them side by side is most of what there is to say.

use crate::material_app::ScrollPlatform;

/// Upstream `ScrollDecelerationRate`, declared with the thing it describes in
/// [`crate::scroll_physics`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::scroll_physics::ScrollDecelerationRate;

/// Upstream `MultitouchDragStrategy`, declared with the thing it describes in
/// [`crate::scroll_plumbing`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::scroll_plumbing::MultitouchDragStrategy;

/// Upstream `CupertinoScrollBehavior`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CupertinoScrollBehavior;

impl CupertinoScrollBehavior {
    /// Upstream `buildScrollbar`, and the interesting part is what is *missing*
    /// beside its Material counterpart.
    ///
    /// [`crate::material_app::MaterialScrollBehavior::builds_scrollbar`] opens
    /// by switching on the axis and returning the child for anything
    /// horizontal, before the platform is consulted at all. **This one has no
    /// axis check.** Neither does the base `ScrollBehavior`. So a horizontal
    /// scrollable on macOS gets a Cupertino scrollbar where the same list in a
    /// Material app would get none.
    ///
    /// All three carry the same note -- *"When modifying this function, consider
    /// modifying the implementation in the base class as well"* -- and **the
    /// three have already drifted apart on exactly this point.** A comment
    /// asking three copies to stay in step is doing the work a shared helper
    /// would have done, and it has not been enough.
    ///
    /// The platform half is identical to both of them: the three desktops get a
    /// bar, the three touch platforms do not, and the
    /// `assert(details.controller != null)` sits inside the desktop arm alone.
    pub fn builds_scrollbar(platform: ScrollPlatform) -> bool {
        platform.is_desktop()
    }

    /// Upstream `buildOverscrollIndicator`, whose entire body is a comment and a
    /// return:
    ///
    /// ```dart
    /// // No overscroll indicator.
    /// return child;
    /// ```
    ///
    /// Tick 89 read Material's Android-only indicator and guessed the rule was
    /// "the platforms that get one are those whose physics do not already show
    /// you the edge". This is that guess confirmed and widened: a Cupertino app
    /// installs bouncing physics on **every** platform, so **nowhere needs one**.
    /// The rule was never about iOS; it was about the bounce.
    pub fn overscroll_indicator(_platform: ScrollPlatform) -> bool {
        false
    }

    /// Upstream `getScrollPhysics`: bouncing everywhere, and macOS alone gets
    /// `ScrollDecelerationRate.fast`.
    ///
    /// Which is the trackpad. A flick with a thumb is meant to coast; a flick
    /// with two fingers on a glass pad is a smaller, more repeatable gesture,
    /// and coasting as far would overshoot everything.
    pub fn deceleration_rate(platform: ScrollPlatform) -> ScrollDecelerationRate {
        match platform {
            ScrollPlatform::MacOS => ScrollDecelerationRate::Fast,
            _ => ScrollDecelerationRate::Normal,
        }
    }

    /// Whether the physics bounce. They always do.
    pub fn bounces(_platform: ScrollPlatform) -> bool {
        true
    }

    /// Upstream `getMultitouchDragStrategy`, which returns
    /// `averageBoundaryPointers` unconditionally: with several fingers down the
    /// scroll follows the average of the outermost two rather than whichever
    /// pointer moved last.
    pub fn multitouch_drag_strategy() -> MultitouchDragStrategy {
        MultitouchDragStrategy::AverageBoundaryPointers
    }
}

/// Upstream `CupertinoApp`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoApp {
    pub has_home: bool,
    pub has_router_delegate: bool,
    pub has_router_config: bool,
    pub debug_show_checked_mode_banner: bool,
}

impl CupertinoApp {
    pub fn new() -> CupertinoApp {
        CupertinoApp {
            has_home: true,
            has_router_delegate: false,
            has_router_config: false,
            debug_show_checked_mode_banner: true,
        }
    }

    /// Upstream's only constructor assert, on the `.router` form:
    /// `assert(routerDelegate != null || routerConfig != null)` -- the same "at
    /// least one" as `MaterialApp.router`.
    pub fn router_is_configured(&self) -> bool {
        self.has_router_delegate || self.has_router_config
    }

    /// `MaterialApp` carries a `debugShowMaterialGrid` and wraps a `GridPaper`
    /// inside an assert block. **`CupertinoApp` has no counterpart** -- no
    /// Cupertino grid overlay exists, because the iOS design language is not
    /// specified against an 8dp grid the way Material is.
    pub fn has_a_design_grid_overlay() -> bool {
        false
    }
}

impl Default for CupertinoApp {
    fn default() -> Self {
        CupertinoApp::new()
    }
}

/// Upstream `CupertinoLocalizations`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CupertinoLocalizations;

impl CupertinoLocalizations {
    /// Upstream's `of` asserts `debugCheckHasCupertinoLocalizations` and then
    /// force-unwraps, exactly as the Material one does.
    pub fn of(present: bool) -> Option<CupertinoLocalizations> {
        present.then_some(CupertinoLocalizations)
    }
}

/// Upstream `DefaultCupertinoLocalizations`, documented -- like its Material
/// opposite number -- as being *"for US English (only)"*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultCupertinoLocalizations;

impl DefaultCupertinoLocalizations {
    /// The same seven strings as `DefaultMaterialLocalizations`, indexed the
    /// same way, by `weekDay - DateTime.monday`.
    ///
    /// And **with no comment above them at all** -- where the Material file
    /// explains the ordering twice and gets it wrong both times
    /// (`// Ordered to match DateTime.monday=1, DateTime.sunday=6`, and Sunday
    /// is 7). Two files doing the identical correct thing; only one of them
    /// explains itself, and that is the one a reader can be misled by.
    pub const SHORT_WEEKDAYS: [&'static str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    pub const SHORT_MONTHS: [&'static str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    pub const MONTHS: [&'static str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];

    /// Dart's `DateTime.monday`.
    pub const MONDAY: u32 = 1;

    /// Upstream `datePickerMonth`, one-based like the `DateTime` constant it is
    /// fed from.
    pub fn date_picker_month(month_index: usize) -> &'static str {
        DefaultCupertinoLocalizations::MONTHS[month_index - 1]
    }

    /// Upstream `datePickerYear`, which is `yearIndex.toString()` -- no padding,
    /// where the Material `formatCompactDate` pads to four digits.
    pub fn date_picker_year(year_index: i32) -> String {
        year_index.to_string()
    }

    /// Upstream `datePickerDayOfMonth`.
    ///
    /// Note the returned string: `' ${_shortWeekdays[...]} $dayIndex '` --
    /// **a leading and a trailing space baked into the localisation itself**,
    /// for the spinner it will be dropped into. A string that carries its own
    /// padding, which is layout hiding inside a translation.
    pub fn date_picker_day_of_month(day_index: u32, week_day: Option<u32>) -> String {
        match week_day {
            Some(week_day) => format!(
                " {} {day_index} ",
                DefaultCupertinoLocalizations::SHORT_WEEKDAYS
                    [(week_day - DefaultCupertinoLocalizations::MONDAY) as usize]
            ),
            None => day_index.to_string(),
        }
    }

    /// Upstream `datePickerMediumDate`, the other use of the weekday list.
    pub fn date_picker_medium_date(week_day: u32, month: usize, day: u32) -> String {
        format!(
            "{} {} {day}",
            DefaultCupertinoLocalizations::SHORT_WEEKDAYS
                [(week_day - DefaultCupertinoLocalizations::MONDAY) as usize],
            DefaultCupertinoLocalizations::SHORT_MONTHS[month - 1]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_app::{MaterialScrollBehavior, ScrollAxis};

    const ALL: [ScrollPlatform; 6] = [
        ScrollPlatform::Android,
        ScrollPlatform::Fuchsia,
        ScrollPlatform::IOS,
        ScrollPlatform::Linux,
        ScrollPlatform::MacOS,
        ScrollPlatform::Windows,
    ];

    // -- Three copies of one function, already out of step --------------------------

    #[test]
    fn a_horizontal_list_gets_a_scrollbar_here_and_none_in_a_material_app() {
        // Material's implementation opens with an axis check; this one and the
        // base class have none.
        assert!(CupertinoScrollBehavior::builds_scrollbar(
            ScrollPlatform::MacOS
        ));
        assert!(!MaterialScrollBehavior::builds_scrollbar(
            ScrollAxis::Horizontal,
            ScrollPlatform::MacOS
        ));
    }

    #[test]
    fn on_the_vertical_axis_the_two_agree_platform_for_platform() {
        for platform in ALL {
            assert_eq!(
                CupertinoScrollBehavior::builds_scrollbar(platform),
                MaterialScrollBehavior::builds_scrollbar(ScrollAxis::Vertical, platform),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn only_the_desktops_get_a_bar_in_either_design() {
        for platform in ALL {
            assert_eq!(
                CupertinoScrollBehavior::builds_scrollbar(platform),
                platform.is_desktop(),
                "{platform:?}"
            );
        }
    }

    // -- The guess from tick 89, confirmed and widened -------------------------------

    #[test]
    fn nowhere_gets_an_overscroll_indicator_because_everywhere_bounces() {
        for platform in ALL {
            assert!(!CupertinoScrollBehavior::overscroll_indicator(platform));
            assert!(
                CupertinoScrollBehavior::bounces(platform),
                "{platform:?} bounces, so it needs none"
            );
        }
    }

    #[test]
    fn android_is_the_case_that_shows_the_rule_is_about_physics_not_platform() {
        use crate::material_app::OverscrollDecoration;
        // The same platform, decorated in a Material app and bare in a Cupertino
        // one, because only one of the two gives it bouncing physics.
        assert_eq!(
            MaterialScrollBehavior::overscroll_indicator(ScrollPlatform::Android, true),
            OverscrollDecoration::Stretching
        );
        assert!(!CupertinoScrollBehavior::overscroll_indicator(
            ScrollPlatform::Android
        ));
    }

    // -- The trackpad ----------------------------------------------------------------

    #[test]
    fn macos_alone_gives_up_faster() {
        for platform in ALL {
            let expected = if platform == ScrollPlatform::MacOS {
                ScrollDecelerationRate::Fast
            } else {
                ScrollDecelerationRate::Normal
            };
            assert_eq!(
                CupertinoScrollBehavior::deceleration_rate(platform),
                expected,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn several_fingers_are_averaged_rather_than_the_last_one_winning() {
        assert_eq!(
            CupertinoScrollBehavior::multitouch_drag_strategy(),
            MultitouchDragStrategy::AverageBoundaryPointers
        );
    }

    // -- The application ---------------------------------------------------------------

    #[test]
    fn the_router_needs_at_least_one_of_its_two_configurations() {
        let mut app = CupertinoApp::new();
        assert!(!app.router_is_configured());
        app.has_router_config = true;
        assert!(app.router_is_configured());
    }

    #[test]
    fn there_is_no_cupertino_grid_overlay_to_match_the_material_one() {
        assert!(!CupertinoApp::has_a_design_grid_overlay());
    }

    // -- The same list, one of them explained wrongly ---------------------------------

    #[test]
    fn the_weekday_lists_of_the_two_defaults_are_identical() {
        use crate::material_app::DefaultMaterialLocalizations;
        assert_eq!(
            DefaultCupertinoLocalizations::SHORT_WEEKDAYS,
            DefaultMaterialLocalizations::SHORT_WEEKDAYS
        );
    }

    #[test]
    fn and_are_indexed_the_same_way_from_the_same_constant() {
        use crate::material_app::DefaultMaterialLocalizations;
        for weekday in 1..=7u32 {
            let cupertino =
                DefaultCupertinoLocalizations::date_picker_day_of_month(1, Some(weekday));
            assert!(
                cupertino.contains(DefaultMaterialLocalizations::short_weekday(weekday)),
                "weekday {weekday}"
            );
        }
    }

    #[test]
    fn sunday_lands_where_it_should_in_the_file_that_says_nothing_about_it() {
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_day_of_month(9, Some(7)),
            " Sun 9 "
        );
    }

    #[test]
    fn the_string_carries_its_own_padding() {
        // A leading and trailing space baked into the localisation, for the
        // spinner it lands in.
        let padded = DefaultCupertinoLocalizations::date_picker_day_of_month(1, Some(1));
        assert!(padded.starts_with(' '));
        assert!(padded.ends_with(' '));
        assert_eq!(padded.trim(), "Mon 1");

        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_day_of_month(1, None),
            "1",
            "and the form without a weekday does not"
        );
    }

    #[test]
    fn the_months_are_one_based_like_the_datetime_constants_that_feed_them() {
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_month(1),
            "January"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_month(12),
            "December"
        );
    }

    #[test]
    fn the_year_is_not_padded_here_where_the_material_compact_date_pads_to_four() {
        use crate::material_app::DefaultMaterialLocalizations;
        assert_eq!(DefaultCupertinoLocalizations::date_picker_year(7), "7");
        assert_eq!(
            DefaultMaterialLocalizations::format_compact_date(7, 1, 2).as_deref(),
            Some("01/02/0007")
        );
    }

    #[test]
    fn a_medium_date_names_both_the_day_and_the_month() {
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_medium_date(3, 9, 27),
            "Wed Sep 27",
            "the example from the doc comment"
        );
    }

    #[test]
    fn localizations_are_fetched_through_a_check_rather_than_a_null() {
        assert!(CupertinoLocalizations::of(true).is_some());
        assert!(CupertinoLocalizations::of(false).is_none());
    }
}
