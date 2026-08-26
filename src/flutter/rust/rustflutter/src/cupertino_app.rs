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

/// One column of a Cupertino date picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatePickerColumn {
    Day,
    Month,
    Year,
}

/// Upstream `DatePickerDateOrder`: which order the date columns run in, left
/// to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DatePickerDateOrder {
    /// 12 | March | 1996.
    Dmy,
    /// March | 12 | 1996. What `DefaultCupertinoLocalizations` reports.
    #[default]
    Mdy,
    /// 1996 | March | 12.
    Ymd,
    /// 1996 | 12 | March.
    Ydm,
}

impl DatePickerDateOrder {
    pub const ALL: [DatePickerDateOrder; 4] = [
        DatePickerDateOrder::Dmy,
        DatePickerDateOrder::Mdy,
        DatePickerDateOrder::Ymd,
        DatePickerDateOrder::Ydm,
    ];

    /// The three columns in order.
    pub fn columns(self) -> [DatePickerColumn; 3] {
        use DatePickerColumn::{Day, Month, Year};
        match self {
            DatePickerDateOrder::Dmy => [Day, Month, Year],
            DatePickerDateOrder::Mdy => [Month, Day, Year],
            DatePickerDateOrder::Ymd => [Year, Month, Day],
            DatePickerDateOrder::Ydm => [Year, Day, Month],
        }
    }

    /// The columns a `monthYear` picker shows.
    ///
    /// Upstream writes this as a second `switch` pairing the cases up --
    /// `mdy` with `dmy` giving month|year, `ymd` with `ydm` giving year|month
    /// -- and its doc says so outright: "both `DatePickerDateOrder.dmy` and
    /// `DatePickerDateOrder.mdy` will result in the month|year order".
    ///
    /// **It is not four cases, it is the same order with the day struck
    /// out.** Removing `Day` from `Mdy` and from `Dmy` leaves month, year
    /// either way; from `Ymd` and `Ydm` it leaves year, month. Deriving it
    /// rather than restating it is what makes the pairing a consequence
    /// instead of a coincidence two lists happen to share.
    pub fn month_year_columns(self) -> [DatePickerColumn; 2] {
        let mut kept = self
            .columns()
            .into_iter()
            .filter(|column| *column != DatePickerColumn::Day);
        [
            kept.next().expect("a month and a year remain"),
            kept.next().expect("a month and a year remain"),
        ]
    }
}

/// One column of a Cupertino date-and-time picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateTimeColumn {
    Date,
    Hour,
    Minute,
    DayPeriod,
}

/// Upstream `DatePickerDateTimeOrder`: where the date sits relative to the
/// time, and which side the am/pm marker takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DatePickerDateTimeOrder {
    /// Fri Aug 31 | 02 | 08 | PM. What `DefaultCupertinoLocalizations`
    /// reports.
    #[default]
    DateTimeDayPeriod,
    /// Fri Aug 31 | PM | 02 | 08.
    DateDayPeriodTime,
    /// 02 | 08 | PM | Fri Aug 31.
    TimeDayPeriodDate,
    /// PM | 02 | 08 | Fri Aug 31.
    DayPeriodTimeDate,
}

impl DatePickerDateTimeOrder {
    pub const ALL: [DatePickerDateTimeOrder; 4] = [
        DatePickerDateTimeOrder::DateTimeDayPeriod,
        DatePickerDateTimeOrder::DateDayPeriodTime,
        DatePickerDateTimeOrder::TimeDayPeriodDate,
        DatePickerDateTimeOrder::DayPeriodTimeDate,
    ];

    /// The four columns in order.
    ///
    /// Upstream names only four of the twenty-four arrangements, and the ones
    /// it leaves out are the point: **the hour always sits immediately before
    /// the minute**, and **the date is always at one end or the other**. A
    /// clock reads left to right and a date is not something to wade through
    /// to reach the minutes.
    pub fn columns(self) -> [DateTimeColumn; 4] {
        use DateTimeColumn::{Date, DayPeriod, Hour, Minute};
        match self {
            DatePickerDateTimeOrder::DateTimeDayPeriod => [Date, Hour, Minute, DayPeriod],
            DatePickerDateTimeOrder::DateDayPeriodTime => [Date, DayPeriod, Hour, Minute],
            DatePickerDateTimeOrder::TimeDayPeriodDate => [Hour, Minute, DayPeriod, Date],
            DatePickerDateTimeOrder::DayPeriodTimeDate => [DayPeriod, Hour, Minute, Date],
        }
    }

    /// Whether the date leads. The only other place it can be is last.
    pub fn date_comes_first(self) -> bool {
        self.columns()[0] == DateTimeColumn::Date
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

    /// The seven standard context menu entries that have a label of their
    /// own on this platform, which arrived with
    /// [`crate::icon_data::ContextMenuButtonType::cupertino_label`]. Only
    /// strings something already says are here, which is the rule
    /// `tools/unread_strings.py` exists to keep.
    pub const CUT_BUTTON_LABEL: &'static str = "Cut";
    pub const COPY_BUTTON_LABEL: &'static str = "Copy";
    pub const PASTE_BUTTON_LABEL: &'static str = "Paste";
    /// Capital A, where the Material table writes "Select all". The two are
    /// translated separately, so the difference is not a typo that would be
    /// caught downstream -- it survives into every locale.
    pub const SELECT_ALL_BUTTON_LABEL: &'static str = "Select All";
    pub const LOOK_UP_BUTTON_LABEL: &'static str = "Look Up";
    pub const SEARCH_WEB_BUTTON_LABEL: &'static str = "Search Web";
    /// With the ellipsis. On iOS sharing opens a sheet, and the three dots
    /// are that platform's promise that the button will not act on its own.
    pub const SHARE_BUTTON_LABEL: &'static str = "Share...";

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

    /// Upstream's `datePickerDateOrder` for US English.
    pub fn date_picker_date_order() -> DatePickerDateOrder {
        DatePickerDateOrder::Mdy
    }

    /// Upstream's `datePickerDateTimeOrder` for US English.
    pub fn date_picker_date_time_order() -> DatePickerDateTimeOrder {
        DatePickerDateTimeOrder::DateTimeDayPeriod
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

#[cfg(test)]
mod date_order_tests {
    use super::{
        DatePickerColumn, DatePickerDateOrder, DatePickerDateTimeOrder, DateTimeColumn,
        DefaultCupertinoLocalizations,
    };

    #[test]
    fn every_order_shows_each_column_exactly_once() {
        for order in DatePickerDateOrder::ALL {
            let columns = order.columns();
            for wanted in [
                DatePickerColumn::Day,
                DatePickerColumn::Month,
                DatePickerColumn::Year,
            ] {
                assert_eq!(
                    columns.iter().filter(|c| **c == wanted).count(),
                    1,
                    "{order:?} {wanted:?}"
                );
            }
        }
        // And the four are four different arrangements.
        let mut seen: Vec<[DatePickerColumn; 3]> = DatePickerDateOrder::ALL
            .iter()
            .map(|o| o.columns())
            .collect();
        seen.dedup();
        assert_eq!(seen.len(), 4);
    }

    #[test]
    fn a_month_year_picker_is_the_same_order_with_the_day_struck_out() {
        for order in DatePickerDateOrder::ALL {
            let expected: Vec<DatePickerColumn> = order
                .columns()
                .into_iter()
                .filter(|c| *c != DatePickerColumn::Day)
                .collect();
            assert_eq!(order.month_year_columns().to_vec(), expected, "{order:?}");
        }
    }

    #[test]
    fn and_that_is_why_two_pairs_of_orders_collapse() {
        // Upstream's second switch pairs mdy with dmy and ymd with ydm. The
        // pairing is a consequence of removing the day, not two lists that
        // happen to agree.
        assert_eq!(
            DatePickerDateOrder::Mdy.month_year_columns(),
            DatePickerDateOrder::Dmy.month_year_columns()
        );
        assert_eq!(
            DatePickerDateOrder::Ymd.month_year_columns(),
            DatePickerDateOrder::Ydm.month_year_columns()
        );
        // The two pairs are still different from each other, or the collapse
        // would have flattened everything.
        assert_ne!(
            DatePickerDateOrder::Mdy.month_year_columns(),
            DatePickerDateOrder::Ymd.month_year_columns()
        );
        assert_eq!(
            DatePickerDateOrder::Mdy.month_year_columns(),
            [DatePickerColumn::Month, DatePickerColumn::Year]
        );
        assert_eq!(
            DatePickerDateOrder::Ymd.month_year_columns(),
            [DatePickerColumn::Year, DatePickerColumn::Month]
        );
        // And the full orders they came from were not equal to begin with.
        assert_ne!(
            DatePickerDateOrder::Mdy.columns(),
            DatePickerDateOrder::Dmy.columns()
        );
    }

    #[test]
    fn the_hour_always_sits_immediately_before_the_minute() {
        // Upstream names four of the twenty-four arrangements. What it leaves
        // out is the rule.
        for order in DatePickerDateTimeOrder::ALL {
            let columns = order.columns();
            let hour = columns
                .iter()
                .position(|c| *c == DateTimeColumn::Hour)
                .expect("an hour");
            let minute = columns
                .iter()
                .position(|c| *c == DateTimeColumn::Minute)
                .expect("a minute");
            assert_eq!(minute, hour + 1, "{order:?}");
        }
    }

    #[test]
    fn and_the_date_is_always_at_one_end() {
        for order in DatePickerDateTimeOrder::ALL {
            let columns = order.columns();
            let date = columns
                .iter()
                .position(|c| *c == DateTimeColumn::Date)
                .expect("a date");
            assert!(date == 0 || date == columns.len() - 1, "{order:?} {date}");
            assert_eq!(order.date_comes_first(), date == 0, "{order:?}");
        }
        // Both ends are actually used, so the rule is not vacuous.
        assert!(DatePickerDateTimeOrder::DateTimeDayPeriod.date_comes_first());
        assert!(!DatePickerDateTimeOrder::TimeDayPeriodDate.date_comes_first());
    }

    #[test]
    fn and_every_date_time_order_shows_four_distinct_columns() {
        for order in DatePickerDateTimeOrder::ALL {
            let columns = order.columns();
            for wanted in [
                DateTimeColumn::Date,
                DateTimeColumn::Hour,
                DateTimeColumn::Minute,
                DateTimeColumn::DayPeriod,
            ] {
                assert_eq!(
                    columns.iter().filter(|c| **c == wanted).count(),
                    1,
                    "{order:?} {wanted:?}"
                );
            }
        }
    }

    #[test]
    fn us_english_puts_the_month_first_and_the_marker_last() {
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_date_order(),
            DatePickerDateOrder::Mdy
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_date_time_order(),
            DatePickerDateTimeOrder::DateTimeDayPeriod
        );
        assert_eq!(DatePickerDateOrder::default(), DatePickerDateOrder::Mdy);
        assert_eq!(
            DatePickerDateTimeOrder::default(),
            DatePickerDateTimeOrder::DateTimeDayPeriod
        );
    }
}
