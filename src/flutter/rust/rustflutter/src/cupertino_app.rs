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
///
/// Its `title` and `onGenerateTitle` are not here because they are not its:
/// both are forwarded to the `WidgetsApp` it builds, unchanged, on both the
/// `.router` and the navigator path. What decides between them lives at
/// [`crate::widgets_app::WidgetsApp::app_title`], and `MaterialApp` forwards
/// the identical pair -- there is no Cupertino-specific naming behaviour to
/// port, which is itself the answer to "where did the title go".
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
    /// Upstream `clearButtonLabel`, and it sits in this list without being
    /// one of its neighbours: the cut/copy/paste words are **painted** on
    /// toolbar buttons, and this one is never drawn at all.
    ///
    /// `CupertinoTextField`'s clear button is an icon --
    /// `CupertinoIcons.clear_thick_circled` -- and the word exists only for a
    /// screen reader:
    ///
    /// ```dart
    /// final String clearLabel =
    ///     widget.clearButtonSemanticLabel ?? CupertinoLocalizations.of(context).clearButtonLabel;
    /// return Semantics(button: true, label: clearLabel, child: ...);
    /// ```
    ///
    /// So it is a localization whose whole audience cannot see the control it
    /// names, which is why the per-widget override is a
    /// `clearButtonSemanticLabel` rather than a `clearButtonText`.
    pub const CLEAR_BUTTON_LABEL: &'static str = "Clear";
    /// Upstream `searchTextFieldPlaceholderLabel`, and **the opposite case**:
    /// this one is painted.
    ///
    /// ```dart
    /// final String placeholder =
    ///     widget.placeholder ?? CupertinoLocalizations.of(context).searchTextFieldPlaceholderLabel;
    /// ```
    ///
    /// The two are written the same way -- a widget property falling back to
    /// the localizations -- and land in different halves of the widget. A
    /// search field with no placeholder set is not an empty well: it says
    /// "Search".
    pub const SEARCH_TEXT_FIELD_PLACEHOLDER_LABEL: &'static str = "Search";
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
    ///
    /// The day is `padRight(2)`, not `padLeft`: a single-digit day carries a
    /// **trailing** space, so "Fri Aug 1 " is as wide as "Fri Aug 31" and the
    /// column does not shuffle as the wheel turns. The same layout-inside-a-
    /// translation as [`DefaultCupertinoLocalizations::date_picker_day_of_month`].
    pub fn date_picker_medium_date(week_day: u32, month: usize, day: u32) -> String {
        format!(
            "{} {} {:<2}",
            DefaultCupertinoLocalizations::SHORT_WEEKDAYS
                [(week_day - DefaultCupertinoLocalizations::MONDAY) as usize],
            DefaultCupertinoLocalizations::SHORT_MONTHS[month - 1],
            day
        )
    }

    /// Upstream `datePickerHour`, which is `hour.toString()` -- **unpadded**,
    /// where the minute beside it is padded to two. A twelve-hour clock reads
    /// "9:05", not "09:05".
    pub fn date_picker_hour(hour: u32) -> String {
        hour.to_string()
    }

    /// Upstream `datePickerMinute`: `padLeft(2, '0')`.
    pub fn date_picker_minute(minute: u32) -> String {
        format!("{minute:02}")
    }

    pub const ANTE_MERIDIEM_ABBREVIATION: &'static str = "AM";
    pub const POST_MERIDIEM_ABBREVIATION: &'static str = "PM";

    /// Upstream `todayLabel`, which the `dateAndTime` date column shows in
    /// place of today's date.
    pub const TODAY_LABEL: &'static str = "Today";

    /// Upstream `timerPickerHour`/`timerPickerMinute`/`timerPickerSecond`:
    /// all three are `toString()`, **none of them padded** -- a countdown's
    /// columns carry their own unit labels, so there is nothing to line up
    /// against.
    pub fn timer_picker_hour(hour: u32) -> String {
        hour.to_string()
    }

    pub fn timer_picker_minute(minute: u32) -> String {
        minute.to_string()
    }

    pub fn timer_picker_second(second: u32) -> String {
        second.to_string()
    }

    /// Upstream `timerPickerHourLabel`, **the only one of the three that is
    /// plural-sensitive**: `hour == 1 ? 'hour' : 'hours'`. The other two are
    /// abbreviations with a full stop and do not inflect.
    pub fn timer_picker_hour_label(hour: u32) -> &'static str {
        if hour == 1 { "hour" } else { "hours" }
    }

    pub fn timer_picker_minute_label(_minute: u32) -> &'static str {
        "min."
    }

    pub fn timer_picker_second_label(_second: u32) -> &'static str {
        "sec."
    }

    /// Upstream `timerPickerHourLabels` / `timerPickerMinuteLabels` /
    /// `timerPickerSecondLabels`: **every** form the label above can take,
    /// not the one it is taking.
    ///
    /// They exist for one job, and it is a layout job. `CupertinoTimerPicker`
    /// measures its label column with
    ///
    /// ```dart
    /// hourLabelWidth = _measureLabelsMaxWidth(localizations.timerPickerHourLabels, textStyle);
    /// ```
    ///
    /// -- the widest of *all* of them. Measuring the label currently on screen
    /// instead would size the column to "hour" while the wheel sits on 1, and
    /// then the column would have to grow the moment the reader spun to 2:
    /// the number beside it would shift sideways as they scrolled, which is
    /// the one thing a spinner must not do.
    ///
    /// So the singular list is not an oversight. English minutes and seconds
    /// are abbreviations that do not inflect, so their lists hold one entry;
    /// hours inflect, so that list holds two. A language whose hour has six
    /// forms would return six here and the column would be sized for the
    /// longest of them.
    ///
    /// [`DefaultCupertinoLocalizations::labels_cover_every_form`] is the
    /// invariant that keeps these lists honest.
    pub const TIMER_PICKER_HOUR_LABELS: [&'static str; 2] = ["hour", "hours"];
    pub const TIMER_PICKER_MINUTE_LABELS: [&'static str; 1] = ["min."];
    pub const TIMER_PICKER_SECOND_LABELS: [&'static str; 1] = ["sec."];

    // -- What a screen reader is told about a spinner ----------------------
    //
    // A date picker's columns show numbers, and a number read out on its own
    // says nothing: "3" in a column of hours and "3" in a column of minutes
    // are the same utterance for two different things. Upstream gives each
    // column a `semanticsLabel` distinct from the text it paints.

    /// Upstream `datePickerHourSemanticsLabel`, `"$hour o'clock"`.
    ///
    /// **The number is the displayed hour, not the wheel's index.** Upstream
    /// converts first --
    ///
    /// ```dart
    /// final int displayHour = widget.use24hFormat ? hour : (hour + 11) % 12 + 1;
    /// ...
    /// semanticsLabel: localizations.datePickerHourSemanticsLabel(displayHour),
    /// ```
    ///
    /// -- so in twelve-hour mode the row that paints "1" is heard as "1
    /// o'clock" even though it stands for 13:00. Handing it the raw hour would
    /// make the reader and the screen disagree by twelve.
    ///
    /// It does not inflect. The generated English class carries the plural
    /// machinery anyway and fills both categories with the same string
    /// (`datePickerHourSemanticsLabelOne` and `...Other` are both
    /// `"$hour o'clock"`), which is the same shape as the timer picker's
    /// one-entry label lists: the categories exist for languages that need
    /// them, not because English does.
    pub fn date_picker_hour_semantics_label(display_hour: u32) -> String {
        format!("{display_hour} o'clock")
    }

    /// Upstream `datePickerMinuteSemanticsLabel`, and **the one in this group
    /// that does inflect**:
    ///
    /// ```dart
    /// if (minute == 1) {
    ///   return '1 minute';
    /// }
    /// return '$minute minutes';
    /// ```
    ///
    /// Zero takes the plural -- "0 minutes" -- which is English's rule and not
    /// every language's.
    pub fn date_picker_minute_semantics_label(minute: u32) -> String {
        if minute == 1 {
            "1 minute".to_string()
        } else {
            format!("{minute} minutes")
        }
    }

    /// Upstream `tabSemanticsLabel`, `"Tab $tabIndex of $tabCount"`, with both
    /// of its asserts:
    ///
    /// ```dart
    /// assert(tabIndex >= 1);
    /// assert(tabCount >= 1);
    /// ```
    ///
    /// **The index is one-based and the caller's is not.** `CupertinoTabBar`
    /// passes `tabIndex: index + 1` from a zero-based loop, and the assert is
    /// what stands between a forgotten `+ 1` and a reader hearing "Tab 0 of
    /// 3". Returns `None` rather than asserting, on this port's usual grounds
    /// -- a wrong number is a bug to catch in a test, not a reason to take the
    /// application down in front of somebody using a screen reader.
    pub fn tab_semantics_label(tab_index: u32, tab_count: u32) -> Option<String> {
        if tab_index < 1 || tab_count < 1 {
            return None;
        }
        Some(format!("Tab {tab_index} of {tab_count}"))
    }

    // -- What a screen reader is told about an expansion tile --------------
    //
    // Upstream declares these six on `CupertinoLocalizations` **and** on
    // `MaterialLocalizations`, with the same English values, and
    // `CupertinoExpansionTile` reads the Cupertino ones:
    //
    //     final CupertinoLocalizations localizations = CupertinoLocalizations.of(context);
    //
    // They are the same words today and two independent contracts, because a
    // locale supplies each class separately -- so a translation could give the
    // Cupertino tile a different phrasing from the Material one and neither
    // class would know. Reading the Material copy from a Cupertino widget
    // happens to be right in English and is not the thing upstream wrote.
    //
    // The crossing between the names and the values is explained where the
    // pairing happens, at
    // [`crate::material_app::DefaultMaterialLocalizations::expansion_tile_hint`].

    /// Upstream's `expandedHint`, which describes the **expanded** state and
    /// whose value is "Collapsed".
    pub const EXPANDED_HINT: &'static str = "Collapsed";
    /// Upstream's `collapsedHint`, which describes the **collapsed** state and
    /// whose value is "Expanded".
    pub const COLLAPSED_HINT: &'static str = "Expanded";
    pub const EXPANSION_TILE_EXPANDED_HINT: &'static str = "double tap to collapse";
    pub const EXPANSION_TILE_COLLAPSED_HINT: &'static str = "double tap to expand";
    /// Not crossed -- see
    /// [`crate::material_app::DefaultMaterialLocalizations::EXPANSION_TILE_EXPANDED_TAP_HINT`].
    pub const EXPANSION_TILE_EXPANDED_TAP_HINT: &'static str = "Collapse";
    pub const EXPANSION_TILE_COLLAPSED_TAP_HINT: &'static str = "Expand for more details";

    /// Upstream's `'${state}\n ${action}'`, on iOS and macOS only.
    pub fn expansion_tile_hint(expanded: bool) -> String {
        if expanded {
            format!(
                "{}\n {}",
                Self::COLLAPSED_HINT,
                Self::EXPANSION_TILE_EXPANDED_HINT
            )
        } else {
            format!(
                "{}\n {}",
                Self::EXPANDED_HINT,
                Self::EXPANSION_TILE_COLLAPSED_HINT
            )
        }
    }

    /// Upstream's `onTapHint`, which is a separate semantics field and is
    /// **not** crossed.
    pub fn expansion_tile_tap_hint(expanded: bool) -> &'static str {
        if expanded {
            Self::EXPANSION_TILE_EXPANDED_TAP_HINT
        } else {
            Self::EXPANSION_TILE_COLLAPSED_TAP_HINT
        }
    }

    /// Whether the three lists above really cover what the three functions
    /// above can return, over `0..=count`.
    ///
    /// Upstream cannot check this -- the list and the function are separate
    /// overrides in every one of its eighty-odd locale classes -- and this
    /// port had the same pair written twice with nothing between them: the
    /// picker's own metrics carried a private `["hour", "hours"]` of their
    /// own, so renaming a label would have left the column measured for the
    /// old word and nothing would have said so.
    pub fn labels_cover_every_form(count: u32) -> bool {
        (0..=count).all(|n| {
            DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS
                .contains(&DefaultCupertinoLocalizations::timer_picker_hour_label(n))
                && DefaultCupertinoLocalizations::TIMER_PICKER_MINUTE_LABELS
                    .contains(&DefaultCupertinoLocalizations::timer_picker_minute_label(n))
                && DefaultCupertinoLocalizations::TIMER_PICKER_SECOND_LABELS
                    .contains(&DefaultCupertinoLocalizations::timer_picker_second_label(n))
        })
    }
}

/// Upstream `CupertinoLocalizationEn`, the English member of
/// `flutter_localizations`' `GlobalCupertinoLocalizations`.
///
/// # Why there are two of these
///
/// [`DefaultCupertinoLocalizations`] above is the class in `packages/flutter`,
/// and it is what an application gets when it installs **no** localisation
/// delegates. Every real application installs `GlobalCupertinoLocalizations`,
/// and the strings differ where it matters most to a picker: the default class
/// writes an hour as `hour.toString()`, and this one runs it through
/// `intl.DateFormat('HH')` -- so a date picker shows `01` under one and `1`
/// under the other. The gallery installs the delegate, so these are the
/// strings on its screen, and they are the ones [`crate::cupertino_pickers`]
/// reads.
///
/// The formats are the skeletons `_GlobalCupertinoLocalizationsDelegate.
/// loadFormats` builds, resolved for `en_US`. This crate compiles one locale,
/// so they are resolved once here rather than looked up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CupertinoLocalizationEn;

impl CupertinoLocalizationEn {
    /// `intl.DateFormat.y` -- the year, unpadded.
    pub fn date_picker_year(year_index: i32) -> String {
        year_index.to_string()
    }

    /// `_fullYearFormat.dateSymbols.MONTHS[monthIndex - 1]`, which for English
    /// is the same list [`DefaultCupertinoLocalizations`] carries.
    pub fn date_picker_month(month_index: usize) -> &'static str {
        DefaultCupertinoLocalizations::MONTHS[month_index - 1]
    }

    /// `intl.DateFormat.d` for the day, with `intl.DateFormat.E` in front of
    /// it when a weekday is asked for -- **no leading or trailing spaces**,
    /// where the default class bakes them into the string.
    pub fn date_picker_day_of_month(day_index: u32, week_day: Option<u32>) -> String {
        match week_day {
            Some(week_day) => format!(
                "{} {day_index}",
                DefaultCupertinoLocalizations::SHORT_WEEKDAYS
                    [(week_day - DefaultCupertinoLocalizations::MONDAY) as usize]
            ),
            None => day_index.to_string(),
        }
    }

    /// `intl.DateFormat.MMMEd`, which for `en_US` is `EEE, MMM d` -- **with a
    /// comma**, where the default class's hand-written version has none and
    /// pads the day instead.
    pub fn date_picker_medium_date(week_day: u32, month: usize, day: u32) -> String {
        format!(
            "{}, {} {day}",
            DefaultCupertinoLocalizations::SHORT_WEEKDAYS
                [(week_day - DefaultCupertinoLocalizations::MONDAY) as usize],
            DefaultCupertinoLocalizations::SHORT_MONTHS[month - 1]
        )
    }

    /// `intl.DateFormat('HH')`: **two digits**. Upstream's own comment on the
    /// field explains the odd skeleton -- "We don't want any additional
    /// decoration here. The am/pm is handled in the date picker. We just want
    /// an hour number localized" -- with a TODO pointing at the intl issue
    /// that would let it ask for the hour alone.
    pub fn date_picker_hour(hour: u32) -> String {
        format!("{hour:02}")
    }

    /// `intl.DateFormat('mm')`: two digits.
    pub fn date_picker_minute(minute: u32) -> String {
        format!("{minute:02}")
    }

    /// `intl.DateFormat('HH')` again -- a countdown's hours are padded even
    /// though its minutes and seconds are not.
    pub fn timer_picker_hour(hour: u32) -> String {
        format!("{hour:02}")
    }

    /// `intl.DateFormat.m`: the minute alone, unpadded.
    pub fn timer_picker_minute(minute: u32) -> String {
        minute.to_string()
    }

    /// `intl.DateFormat.s`: the second alone, unpadded.
    pub fn timer_picker_second(second: u32) -> String {
        second.to_string()
    }

    /// The four strings the generated `CupertinoLocalizationEn` carries
    /// verbatim, which are the same as the default class's.
    pub const ANTE_MERIDIEM_ABBREVIATION: &'static str = "AM";
    pub const POST_MERIDIEM_ABBREVIATION: &'static str = "PM";
    pub const TODAY_LABEL: &'static str = "Today";

    pub fn timer_picker_hour_label(hour: u32) -> &'static str {
        DefaultCupertinoLocalizations::timer_picker_hour_label(hour)
    }

    pub fn timer_picker_minute_label(minute: u32) -> &'static str {
        DefaultCupertinoLocalizations::timer_picker_minute_label(minute)
    }

    pub fn timer_picker_second_label(second: u32) -> &'static str {
        DefaultCupertinoLocalizations::timer_picker_second_label(second)
    }

    /// The generated English class overrides these three the same way it
    /// overrides the singular labels: with the same words. Forwarded rather
    /// than repeated, so the pair cannot drift apart here either.
    pub const TIMER_PICKER_HOUR_LABELS: [&'static str; 2] =
        DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS;
    pub const TIMER_PICKER_MINUTE_LABELS: [&'static str; 1] =
        DefaultCupertinoLocalizations::TIMER_PICKER_MINUTE_LABELS;
    pub const TIMER_PICKER_SECOND_LABELS: [&'static str; 1] =
        DefaultCupertinoLocalizations::TIMER_PICKER_SECOND_LABELS;

    /// Both orders are the generated class's, and both are the default
    /// class's too.
    pub fn date_picker_date_order() -> DatePickerDateOrder {
        DatePickerDateOrder::Mdy
    }

    pub fn date_picker_date_time_order() -> DatePickerDateTimeOrder {
        DatePickerDateTimeOrder::DateTimeDayPeriod
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

    // -- Two English localisations, and where they disagree --------------------------

    #[test]
    fn the_delegate_pads_a_picker_hour_and_the_default_class_does_not() {
        // The difference an application sees the moment it installs
        // `GlobalCupertinoLocalizations`, and the reason
        // `cupertino_pickers` reads the delegate's strings: a date picker
        // shows `01` under one and `1` under the other.
        assert_eq!(DefaultCupertinoLocalizations::date_picker_hour(1), "1");
        assert_eq!(CupertinoLocalizationEn::date_picker_hour(1), "01");
        assert_eq!(CupertinoLocalizationEn::date_picker_hour(12), "12");

        // A countdown's hours are padded and its minutes and seconds are not,
        // in the delegate; the default class pads none of the three.
        assert_eq!(CupertinoLocalizationEn::timer_picker_hour(5), "05");
        assert_eq!(CupertinoLocalizationEn::timer_picker_minute(5), "5");
        assert_eq!(CupertinoLocalizationEn::timer_picker_second(5), "5");
        assert_eq!(DefaultCupertinoLocalizations::timer_picker_hour(5), "5");
    }

    #[test]
    fn the_delegates_medium_date_has_a_comma_and_the_default_classs_has_padding() {
        // `intl.DateFormat.MMMEd` against a string written out by hand: the
        // one place the two Englishes differ by punctuation rather than by
        // width.
        assert_eq!(
            CupertinoLocalizationEn::date_picker_medium_date(4, 8, 27),
            "Thu, Aug 27"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_medium_date(4, 8, 27),
            "Thu Aug 27"
        );
        // And the default class's `padRight(2)`, which is what keeps its own
        // column from shuffling: a one-digit day carries a trailing space.
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_medium_date(4, 8, 1),
            "Thu Aug 1 "
        );
    }

    #[test]
    fn a_spinners_number_is_said_as_a_quantity_not_as_a_digit() {
        // "3" alone is the same utterance in a column of hours and a column
        // of minutes. The label is what tells them apart.
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_hour_semantics_label(3),
            "3 o'clock"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_minute_semantics_label(3),
            "3 minutes"
        );
    }

    #[test]
    fn the_minute_inflects_and_the_hour_does_not() {
        // The minute is the only one in this group that changes shape. The
        // hour keeps "o'clock" at every value -- the generated English class
        // fills both plural categories with the same string, which is the
        // same shape as the timer picker's one-entry label lists.
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_minute_semantics_label(1),
            "1 minute"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_minute_semantics_label(2),
            "2 minutes"
        );
        // Zero takes the plural, which is English's rule and not every
        // language's.
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_minute_semantics_label(0),
            "0 minutes"
        );
        for hour in [1, 2, 12, 23] {
            assert!(
                DefaultCupertinoLocalizations::date_picker_hour_semantics_label(hour)
                    .ends_with("o'clock"),
                "hour {hour} keeps its shape"
            );
        }
    }

    #[test]
    fn a_tab_is_counted_from_one_because_it_is_said_out_loud() {
        assert_eq!(
            DefaultCupertinoLocalizations::tab_semantics_label(1, 3).as_deref(),
            Some("Tab 1 of 3")
        );
        assert_eq!(
            DefaultCupertinoLocalizations::tab_semantics_label(3, 3).as_deref(),
            Some("Tab 3 of 3")
        );
        // Upstream's two asserts. A zero index means a caller forgot the
        // `+ 1`; answering None is this port's usual choice over taking the
        // application down in front of somebody using a screen reader.
        assert_eq!(
            DefaultCupertinoLocalizations::tab_semantics_label(0, 3),
            None
        );
        assert_eq!(
            DefaultCupertinoLocalizations::tab_semantics_label(1, 0),
            None
        );
    }

    #[test]
    fn only_the_hour_label_of_a_countdown_inflects() {
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_hour_label(1),
            "hour"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_hour_label(2),
            "hours"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_hour_label(0),
            "hours"
        );
        // The other two are abbreviations with a full stop, and do not.
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_minute_label(1),
            "min."
        );
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_second_label(1),
            "sec."
        );
    }

    #[test]
    fn the_label_lists_cover_every_form_the_labels_can_take() {
        // The invariant the picker's column width rests on. Upstream cannot
        // check it -- the list and the function are separate overrides in
        // every locale class -- and this port had the pair written twice with
        // nothing between them, so renaming a label would have left the
        // column measured for the old word.
        assert!(DefaultCupertinoLocalizations::labels_cover_every_form(99));
        assert!(CupertinoLocalizationEn::TIMER_PICKER_HOUR_LABELS.len() == 2);
    }

    #[test]
    fn the_singular_lists_are_a_fact_about_english_not_an_omission() {
        // English minutes and seconds are abbreviations that do not inflect,
        // so one entry each is the whole truth; hours inflect, so two. A
        // language whose hour had six forms would list six and the column
        // would be sized for the longest.
        assert_eq!(
            DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS,
            ["hour", "hours"]
        );
        assert_eq!(
            DefaultCupertinoLocalizations::TIMER_PICKER_MINUTE_LABELS,
            ["min."]
        );
        assert_eq!(
            DefaultCupertinoLocalizations::TIMER_PICKER_SECOND_LABELS,
            ["sec."]
        );
        // The generated English class forwards rather than repeating them.
        assert_eq!(
            CupertinoLocalizationEn::TIMER_PICKER_HOUR_LABELS,
            DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS
        );
    }

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
