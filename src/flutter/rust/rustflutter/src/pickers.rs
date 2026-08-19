// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Date and time pickers.
//!
//! Ports of upstream's `packages/flutter/lib/src/material/date_picker.dart`,
//! `calendar_date_picker.dart`, `input_date_picker_form_field.dart`,
//! `time_picker.dart`, `date.dart` and `time.dart`. Each item names its
//! upstream anchor symbol in a comment.
//!
//! # Overlays are the app's, not the framework's
//!
//! Upstream's `showDatePicker`/`showTimePicker` push a route on the
//! `Navigator` and hand back a `Future`. This framework has no `Navigator`
//! route stack for overlays and no `Future`: a picker is composed into the
//! page's `Stack` like any other overlay (see [`crate::controls`]'s header).
//! [`show_date_picker`], [`show_time_picker`] and [`show_date_range_picker`]
//! therefore *return the overlay widget* -- a scrim under the centred dialog,
//! the overlay the caller stacks last -- and the result arrives through the
//! dialog's `with_on_confirm`/`with_on_cancel` callbacks instead of a future.
//!
//! # Deviations that apply to the whole module
//!
//! - **Localization.** Upstream formats and parses through
//!   `MaterialLocalizations`/`DateFormat`; this crate has no localization
//!   layer, so the English (`en_US`) strings and the `mm/dd/yyyy` compact
//!   format are compiled in. The affected spots say so where they are.
//! - **Material icons.** Upstream's chevrons, edit and calendar icons are
//!   glyphs from the Material icons font, which this engine bridge does not
//!   load. They are text here (`<`, `>`, `v`, `^`, `Edit`, `Calendar`), each
//!   noted at its site.
//! - **Text scale clamps.** Upstream clamps `textScaleFactor` at several
//!   points of the dialogs; the fixed dialog sizes here are the 1.0 scale
//!   sizes and no scaling is applied inside them.
//! - **Semantics and keyboard traversal** of the day grid and dial
//!   (upstream's `_CalendarKeyboardNavigator`, day `FocusNode`s, semantics
//!   announcements) are not ported; pointer input is.
//! - **Animations.** The dialog size and the dial hand snap instead of
//!   animating over upstream's 200ms; the crate's frame clock could drive
//!   them, but a picker that snaps is strictly simpler and the end states
//!   are identical.

use std::cell::Cell;
use std::rc::Rc;

use crate::components::{Button, ButtonVariant, theme_of};
use crate::direction::current_direction;
use crate::editable::TextField;
use crate::engine::{Color, Paint, Paragraph, Rect, Style, TextStyle};
use crate::framework::{
    AnyWidget, BuildContext, Key, StateHandle, StatefulComponent, component, leaf, many, stateful,
};
use crate::gestures::PointerHandlers;
use crate::media_query::media_query_of;
use crate::render::{
    Alignment, BoxConstraints, CrossAxisAlignment, EdgeInsets, FlexChild, MainAxisAlignment,
    MainAxisSize, Offset, PaintContext, RenderBox, RenderFlex, RenderRef, RenderStack, Size,
};
use crate::scrolling::Scroll;
use crate::widgets::{Center, Column, Container, Empty, ListView, Pointer, Positioned, Row, Text};

// -- Dates --------------------------------------------------------------------
//
// Anchor: dart:core's `DateTime`, date-only. Upstream's pickers ignore the
// time fields of every `DateTime` they take (`CalendarDelegate.dateOnly`),
// so the value type here is a date and nothing else.

/// A calendar day: a year, a month (1-12) and a day of the month.
///
/// Ordering is chronological. Construction normalizes overflow the way
/// Dart's `DateTime` constructor does: `Date::new(2024, 13, 1)` is
/// 2025-01-01 and `Date::new(2024, 1, 32)` is 2024-02-01.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

/// Days between the civil date and 1970-01-01. Howard Hinnant's
/// `days_from_civil`; this is the arithmetic `DateUtils.addDaysToDate` gets
/// from Dart's `DateTime` for free.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let shifted = if month <= 2 { year - 1 } else { year } as i64;
    let era = if shifted >= 0 { shifted } else { shifted - 399 } / 400;
    let year_of_era = shifted - era * 400;
    let month_prime = (month as i64 + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

/// The inverse of [`days_from_civil`]: Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let shifted = days + 719468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146096
    } / 146097;
    let day_of_era = shifted - era * 146097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year } as i32, month, day)
}

impl Date {
    /// A date, rolling over out-of-range months and days the way Dart's
    /// `DateTime(year, month, day)` does.
    pub fn new(year: i32, month: i32, day: i32) -> Date {
        let total_months = year * 12 + (month - 1);
        let year = total_months.div_euclid(12);
        let month = total_months.rem_euclid(12) as u32 + 1;
        let days = days_from_civil(year, month, 1) + (day as i64 - 1);
        let (year, month, day) = civil_from_days(days);
        Date { year, month, day }
    }

    /// Today, in UTC. Upstream uses the local `DateTime.now()`; the engine
    /// bridge has no local-time query, so the UTC date stands in for it --
    /// the two differ for a few hours around midnight at the dateline's
    /// side of the day, which shifts which day is ringed as "today".
    pub fn today() -> Date {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
        Date { year, month, day }
    }

    /// Days since 1970-01-01, the form day arithmetic is done in.
    pub fn to_days(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    /// The day of the week: Monday is 1, Sunday is 7, as `DateTime.weekday`.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday (4).
        ((self.to_days() + 3).rem_euclid(7) + 1) as u32
    }

    /// The first of this date's month, upstream's
    /// `GregorianCalendarDelegate.getMonth`.
    pub fn first_of_month(self) -> Date {
        Date {
            year: self.year,
            month: self.month,
            day: 1,
        }
    }
}

/// The number of days in a month of the proleptic Gregorian calendar.
///
/// Anchor: `DateUtils.getDaysInMonth` in `date.dart`, including its leap
/// year rule.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    if month == 2 {
        let is_leap_year = (year % 4 == 0) && (year % 100 != 0) || (year % 400 == 0);
        return if is_leap_year { 29 } else { 28 };
    }
    const DAYS_IN_MONTH: [u32; 12] = [31, 0, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    DAYS_IN_MONTH[(month - 1) as usize]
}

/// The offset from the first day of the week that the first of the month
/// falls on: the number of leading blanks in the calendar grid.
///
/// Anchor: `DateUtils.firstDayOffset` in `date.dart`.
/// `first_day_of_week_index` is upstream's
/// `MaterialLocalizations.firstDayOfWeekIndex` -- a 0-based index into the
/// Sunday-first weekday list, 0 (Sunday) for `en_US`, which is the only
/// locale this crate compiles in.
pub fn first_day_offset(year: i32, month: u32, first_day_of_week_index: u32) -> u32 {
    // 0-based day of week for the first of the month, with 0 being Monday.
    let weekday_from_monday = Date::new(year, month as i32, 1).weekday() as i32 - 1;
    // The start-of-week index recomputed to be Monday-based, to compare.
    let first = (first_day_of_week_index as i32 - 1).rem_euclid(7);
    (weekday_from_monday - first).rem_euclid(7) as u32
}

/// The number of months between two dates.
///
/// Anchor: `DateUtils.monthDelta` in `date.dart`.
pub fn month_delta(start: Date, end: Date) -> i32 {
    (end.year - start.year) * 12 + end.month as i32 - start.month as i32
}

/// `month_date` moved by some months, with the day set to 1.
///
/// Anchor: `DateUtils.addMonthsToMonthDate` in `date.dart`.
pub fn add_months_to_month_date(month_date: Date, months_to_add: i32) -> Date {
    Date::new(month_date.year, month_date.month as i32 + months_to_add, 1)
}

/// `date` moved by some days.
///
/// Anchor: `DateUtils.addDaysToDate` in `date.dart`.
pub fn add_days_to_date(date: Date, days: i32) -> Date {
    let (year, month, day) = civil_from_days(date.to_days() + days as i64);
    Date { year, month, day }
}

/// Whether two optional dates are the same day, both null included.
///
/// Anchor: `DateUtils.isSameDay` in `date.dart`.
pub fn is_same_day(a: Option<Date>, b: Option<Date>) -> bool {
    a.map(|a| a.year) == b.map(|b| b.year)
        && a.map(|a| a.month) == b.map(|b| b.month)
        && a.map(|a| a.day) == b.map(|b| b.day)
}

/// Whether two optional dates are in the same month, both null included.
///
/// Anchor: `DateUtils.isSameMonth` in `date.dart`.
pub fn is_same_month(a: Option<Date>, b: Option<Date>) -> bool {
    a.map(|a| a.year) == b.map(|b| b.year) && a.map(|a| a.month) == b.map(|b| b.month)
}

// -- Date formatting ------------------------------------------------------------
//
// Anchors: `DefaultMaterialLocalizations` (`material_localizations.dart`)
// and `GregorianCalendarDelegate` (`date.dart`). Only the `en_US` forms are
// compiled in; see the module header.

const MONTH_NAMES: [&str; 12] = [
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

const SHORT_MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Monday-first, matching `DateTime.weekday`'s 1-based numbering.
const WEEKDAY_NAMES: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

const SHORT_WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Sunday-first, as `MaterialLocalizations.narrowWeekdays`.
const NARROW_WEEKDAYS: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];

/// `en_US`'s first day of the week is Sunday.
///
/// Anchor: `DefaultMaterialLocalizations.firstDayOfWeekIndex`.
pub const FIRST_DAY_OF_WEEK_INDEX: u32 = 0;

/// "August 2026".
///
/// Anchor: `GregorianCalendarDelegate.formatMonthYear`.
pub fn format_month_year(date: Date) -> String {
    format!("{} {}", MONTH_NAMES[(date.month - 1) as usize], date.year)
}

/// "Mon, Aug 17" -- the header title of a date picker dialog.
///
/// Anchor: `GregorianCalendarDelegate.formatMediumDate`.
pub fn format_medium_date(date: Date) -> String {
    format!(
        "{}, {} {}",
        SHORT_WEEKDAY_NAMES[(date.weekday() - 1) as usize],
        SHORT_MONTH_NAMES[(date.month - 1) as usize],
        date.day,
    )
}

/// "Monday, August 17, 2026".
///
/// Anchor: `GregorianCalendarDelegate.formatFullDate`.
pub fn format_full_date(date: Date) -> String {
    format!(
        "{}, {} {}, {}",
        WEEKDAY_NAMES[(date.weekday() - 1) as usize],
        MONTH_NAMES[(date.month - 1) as usize],
        date.day,
        date.year,
    )
}

/// "08/17/2026" -- the text the input form shows for a date.
///
/// Anchor: `GregorianCalendarDelegate.formatCompactDate`, which is
/// `DateFormat.yMd()` for `en_US`.
pub fn format_compact_date(date: Date) -> String {
    format!("{:02}/{:02}/{:04}", date.month, date.day, date.year)
}

/// Parses what [`format_compact_date`] writes: "mm/dd/yyyy".
///
/// Anchor: `GregorianCalendarDelegate.parseCompactDate`. Upstream goes
/// through `DateFormat`, which accepts a few more shapes than this; the
/// strict month/day/year-with-slashes form is the one the field itself
/// shows as its hint, so it is the one accepted here.
pub fn parse_compact_date(text: &str) -> Option<Date> {
    let mut parts = text.split('/');
    let month: i32 = parts.next()?.trim().parse().ok()?;
    let day: i32 = parts.next()?.trim().parse().ok()?;
    let year: i32 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || year <= 0 {
        return None;
    }
    if !(1..=days_in_month(year, month as u32) as i32).contains(&day) {
        return None;
    }
    Some(Date::new(year, month, day))
}

// -- Picker enums ---------------------------------------------------------------

/// Mode of date entry method for the date picker dialog.
///
/// Anchor: `DatePickerEntryMode` in `date.dart`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DatePickerEntryMode {
    /// User picks a date from a calendar grid, and can switch to `Input`.
    #[default]
    Calendar,
    /// User types the date into a text field, and can switch to `Calendar`.
    Input,
    /// Calendar grid only; no mode switch.
    CalendarOnly,
    /// Text field only; no mode switch.
    InputOnly,
}

/// Initial display of a calendar date picker: the day grid or the year grid.
///
/// Anchor: `DatePickerMode` in `date.dart`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DatePickerMode {
    /// Choosing a month and day.
    #[default]
    Day,
    /// Choosing a year.
    Year,
}

/// Interactive input mode of the time picker dialog.
///
/// Anchor: `TimePickerEntryMode` in `time_picker.dart`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TimePickerEntryMode {
    /// User picks a time from a clock dial, and can switch to `Input`.
    #[default]
    Dial,
    /// User types the time into text fields, and can switch to `Dial`.
    Input,
    /// Dial only; no mode switch.
    DialOnly,
    /// Text fields only; no mode switch.
    InputOnly,
}

/// Whether a day can be selected.
///
/// Anchor: `SelectableDayPredicate` in `date.dart`.
pub type SelectableDayPredicate = fn(Date) -> bool;

/// Whether a day can be selected, given the range selected so far.
///
/// Anchor: `SelectableDayForRangePredicate` in `date_picker.dart`.
pub type SelectableDayForRangePredicate = fn(Date, Option<Date>, Option<Date>) -> bool;

/// A start and an end date, both inclusive.
///
/// Anchor: `DateTimeRange` in `date.dart`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DateTimeRange {
    pub start: Date,
    pub end: Date,
}

impl DateTimeRange {
    pub fn new(start: Date, end: Date) -> DateTimeRange {
        debug_assert!(
            start <= end,
            "the start of a range must not be after its end"
        );
        DateTimeRange { start, end }
    }

    /// The length of the range in days. Upstream's `DateTimeRange.duration`,
    /// as a count of days rather than a `Duration`, the value type here
    /// carrying no time of day.
    pub fn duration_days(&self) -> i64 {
        self.end.to_days() - self.start.to_days()
    }
}

// -- Time of day ------------------------------------------------------------------
//
// Anchor: `TimeOfDay` and `DayPeriod` in `time.dart`.

/// Ante or post meridiem.
///
/// Anchor: `DayPeriod` in `time.dart`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DayPeriod {
    Am,
    Pm,
}

/// A time of day, independent of a date.
///
/// Anchor: `TimeOfDay` in `time.dart`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct TimeOfDay {
    pub hour: u32,
    pub minute: u32,
}

impl TimeOfDay {
    /// Anchor: `TimeOfDay.hoursPerDay`.
    pub const HOURS_PER_DAY: u32 = 24;
    /// Anchor: `TimeOfDay.hoursPerPeriod`.
    pub const HOURS_PER_PERIOD: u32 = 12;
    /// Anchor: `TimeOfDay.minutesPerHour`.
    pub const MINUTES_PER_HOUR: u32 = 60;

    pub fn new(hour: u32, minute: u32) -> TimeOfDay {
        debug_assert!(hour < Self::HOURS_PER_DAY && minute < Self::MINUTES_PER_HOUR);
        TimeOfDay { hour, minute }
    }

    /// The current time, in UTC. The same caveat as [`Date::today`]:
    /// upstream's `TimeOfDay.now()` is local and this one is not.
    pub fn now() -> TimeOfDay {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        TimeOfDay {
            hour: ((seconds / 3600) % 24) as u32,
            minute: ((seconds / 60) % 60) as u32,
        }
    }

    /// A copy with the hour and/or minute replaced.
    ///
    /// Anchor: `TimeOfDay.replacing`.
    pub fn replacing(self, hour: Option<u32>, minute: Option<u32>) -> TimeOfDay {
        TimeOfDay {
            hour: hour.unwrap_or(self.hour),
            minute: minute.unwrap_or(self.minute),
        }
    }

    /// The period this hour falls in.
    ///
    /// Anchor: `TimeOfDay.period`.
    pub fn period(&self) -> DayPeriod {
        if self.hour < Self::HOURS_PER_PERIOD {
            DayPeriod::Am
        } else {
            DayPeriod::Pm
        }
    }

    /// The hour in 1-12 form: 0 and 12 both read as 12.
    ///
    /// Anchor: `TimeOfDay.hourOfPeriod`.
    pub fn hour_of_period(&self) -> u32 {
        if self.hour == 0 || self.hour == 12 {
            12
        } else {
            self.hour - self.period_offset()
        }
    }

    /// 0 for AM, 12 for PM: what `hourOfPeriod` is shifted by.
    ///
    /// Anchor: `TimeOfDay.periodOffset`.
    pub fn period_offset(&self) -> u32 {
        match self.period() {
            DayPeriod::Am => 0,
            DayPeriod::Pm => Self::HOURS_PER_PERIOD,
        }
    }

    /// "2:30 PM", or "14:30" with the 24-hour format.
    ///
    /// Anchor: `DefaultMaterialLocalizations.formatTimeOfDay` for `en_US`'s
    /// `h:mm a` and `HH:mm` formats.
    pub fn format(&self, always_use_24_hour: bool) -> String {
        if always_use_24_hour {
            format!("{:02}:{:02}", self.hour, self.minute)
        } else {
            let period = match self.period() {
                DayPeriod::Am => "AM",
                DayPeriod::Pm => "PM",
            };
            format!("{}:{:02} {}", self.hour_of_period(), self.minute, period)
        }
    }
}

// -- Orientation -------------------------------------------------------------------

/// Tall or wide.
///
/// Anchor: `Orientation` in `widgets/basic.dart`. Both pickers read it from
/// the ambient `MediaQuery` (`MediaQuery.orientationOf`), which derives it
/// from the view's aspect; [`TimePickerDialog::with_orientation`] overrides
/// it the way upstream's `orientation` parameter does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// What `MediaQuery.orientationOf` answers: wide is landscape, tall is not.
fn orientation_of(context: &BuildContext) -> Orientation {
    let size = media_query_of(context).size;
    if size.width > size.height {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    }
}

// -- The calendar grid -----------------------------------------------------------
//
// Anchors: `CalendarDatePicker`, `_MonthPicker`, `_DayPicker`, `_Day`,
// `_DatePickerModeToggleButton` and `YearPicker` in calendar_date_picker.dart.

/// `_dayPickerRowHeightM3`: the M3 row height this port follows throughout.
const DAY_PICKER_ROW_HEIGHT: f32 = 48.0;
/// `_subHeaderHeight`: the row the month title and the chevrons sit in.
const SUB_HEADER_HEIGHT: f32 = 52.0;
/// `_monthPickerHorizontalPaddingPortraitM3`.
const MONTH_PICKER_HORIZONTAL_PADDING: f32 = 12.0;
/// `_yearPickerColumnCount`.
const YEAR_PICKER_COLUMN_COUNT: usize = 3;
/// `_yearPickerPadding`.
const YEAR_PICKER_PADDING: f32 = 16.0;
/// `_yearPickerRowHeight`.
const YEAR_PICKER_ROW_HEIGHT: f32 = 52.0;
/// `_yearPickerRowSpacing`.
const YEAR_PICKER_ROW_SPACING: f32 = 8.0;
/// `_YearPickerState.minYears`: fewer years than this are padded out with
/// disabled cells so the grid does not look sparse.
const MIN_YEARS: i32 = 18;

/// Everything a day cell needs to draw, copied out of the theme so the
/// building closures can own it.
#[derive(Clone)]
struct DayCellPalette {
    body_size: f32,
    text: Color,
    primary: Color,
    on_primary: Color,
}

/// One day cell of the month grid.
///
/// Anchor: `_Day.build` in calendar_date_picker.dart: the selected day is a
/// filled primary circle, today is ringed, a disabled day is greyed out
/// (upstream's 38% opacity disabled color) and takes no tap. The cell is 48
/// tall with the M3 padding of 4, so the circle is 40 across.
fn day_cell(
    id: u64,
    date: Date,
    disabled: bool,
    selected: bool,
    today: bool,
    handle: StateHandle<CalendarDatePickerState>,
    on_date_changed: Option<Rc<dyn Fn(Date)>>,
    palette: &DayCellPalette,
) -> crate::render::RenderPointerRegion {
    let (fill, border, text_color) = if selected {
        (Some(palette.primary), None, palette.on_primary)
    } else if today {
        (None, Some(palette.primary), palette.primary)
    } else if disabled {
        (None, None, palette.text.with_alpha(0x61))
    } else {
        (None, None, palette.text)
    };
    let mut circle = Container::new()
        .with_size(40.0, 40.0)
        .with_corner_radius(20.0)
        .with_alignment(Alignment::CENTER)
        .with_child(
            Text::new(format!("{}", date.day))
                .with_size(palette.body_size)
                .with_color(text_color),
        );
    if let Some(fill) = fill {
        circle = circle.with_color(fill);
    }
    if let Some(border) = border {
        circle = circle.with_border(1.0, border);
    }
    let cell = Container::new()
        .with_height(DAY_PICKER_ROW_HEIGHT)
        .with_padding(EdgeInsets::all(4.0))
        .with_child(Center::new(circle));
    let mut region = Pointer::new(id, cell);
    if !disabled {
        region = region.with_handlers(PointerHandlers::new().with_tap(move |_| {
            // `_CalendarDatePickerState._handleDayChanged`.
            handle.set_state(move |state| state.selected_date = Some(date));
            if let Some(changed) = &on_date_changed {
                changed(date);
            }
        }));
    }
    region
}

/// The month grid: a weekday header row, then the weeks.
///
/// Anchor: `_DayPicker.build` in calendar_date_picker.dart. Upstream lays the
/// grid out with a `GridView.custom` under a grid delegate; a fixed
/// seven-column flex grid is the same layout without the scrolling the grid
/// never uses.
#[allow(clippy::too_many_arguments)]
fn day_grid(
    id: u64,
    displayed: Date,
    selected: Option<Date>,
    first_date: Date,
    last_date: Date,
    current_date: Date,
    predicate: Option<SelectableDayPredicate>,
    handle: StateHandle<CalendarDatePickerState>,
    on_date_changed: Option<Rc<dyn Fn(Date)>>,
    weekday_style: TextStyle,
    palette: DayCellPalette,
) -> RenderFlex {
    let days_in_month = days_in_month(displayed.year, displayed.month);
    let day_offset = first_day_offset(displayed.year, displayed.month, FIRST_DAY_OF_WEEK_INDEX);

    let mut grid = Column::new()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    // `_DayPickerState._dayHeaders`, starting at the first day of the week
    // of the locale (Sunday for the compiled-in en_US).
    let mut header = Row::new();
    for i in 0..7u32 {
        let weekday = NARROW_WEEKDAYS[((FIRST_DAY_OF_WEEK_INDEX + i) % 7) as usize];
        header = header.push_flex(FlexChild::expanded(
            Container::new()
                .with_height(DAY_PICKER_ROW_HEIGHT)
                .with_alignment(Alignment::CENTER)
                .with_child(Text::new(weekday).with_style(weekday_style.clone())),
            1,
        ));
    }
    grid = grid.push(header);

    // The `while (day < daysInMonth)` loop of `_DayPickerState.build`.
    let mut row = Row::new();
    let mut column_of_row = 0u32;
    let mut day = 1i32 - day_offset as i32;
    while day <= days_in_month as i32 {
        if day < 1 {
            row = row.push_flex(FlexChild::expanded(
                Container::new().with_height(DAY_PICKER_ROW_HEIGHT),
                1,
            ));
        } else {
            let date = Date::new(displayed.year, displayed.month as i32, day);
            let disabled =
                date > last_date || date < first_date || predicate.is_some_and(|p| !p(date));
            row = row.push_flex(FlexChild::expanded(
                day_cell(
                    id * 10000 + day as u64,
                    date,
                    disabled,
                    is_same_day(selected, Some(date)),
                    is_same_day(Some(current_date), Some(date)),
                    handle.clone(),
                    on_date_changed.clone(),
                    &palette,
                ),
                1,
            ));
        }
        column_of_row += 1;
        if column_of_row == 7 {
            grid = grid.push(row);
            row = Row::new();
            column_of_row = 0;
        }
        day += 1;
    }
    // The last week rarely ends on a Saturday: pad it out with blanks, the
    // same `day < 1` cells the loop opens with.
    if column_of_row > 0 {
        while column_of_row < 7 {
            row = row.push_flex(FlexChild::expanded(
                Container::new().with_height(DAY_PICKER_ROW_HEIGHT),
                1,
            ));
            column_of_row += 1;
        }
        grid = grid.push(row);
    }
    grid
}

/// The subheader: the mode-toggle title on the left, the month chevrons on
/// the right.
///
/// Anchor: `_MonthPicker.build`'s header row with `_DatePickerModeToggleButton`
/// floated over it -- upstream overlays the toggle on a `Stack`; one row
/// holds both here, which is the same layout without the overlap. The
/// chevrons and the drop-down arrow are text (`<`, `>`, `v`, `^`), the
/// Material icons font not being loaded.
#[allow(clippy::too_many_arguments)]
fn calendar_subheader(
    id: u64,
    mode: DatePickerMode,
    displayed: Date,
    first_date: Date,
    last_date: Date,
    handle: StateHandle<CalendarDatePickerState>,
    on_displayed_month_changed: Option<Rc<dyn Fn(Date)>>,
    body_size: f32,
    text: Color,
) -> RenderFlex {
    let title = format!(
        "{} {}",
        format_month_year(displayed),
        if mode == DatePickerMode::Day {
            "v"
        } else {
            "^"
        }
    );
    let toggle_handle = handle.clone();
    let toggle = Pointer::new(
        id * 10000 + 997,
        Container::new()
            .with_height(SUB_HEADER_HEIGHT - 8.0)
            .with_padding(EdgeInsets::symmetric(8.0, 0.0))
            .with_alignment(Alignment::CENTER_LEFT)
            .with_child(
                Text::new(title)
                    .with_size(body_size)
                    .with_weight(700)
                    .with_color(text),
            ),
    )
    .with_handlers(PointerHandlers::new().with_tap(move |_| {
        toggle_handle.set_state(|state| {
            // `_CalendarDatePickerState._handleModeChanged`.
            state.mode = match state.mode {
                DatePickerMode::Day => DatePickerMode::Year,
                DatePickerMode::Year => DatePickerMode::Day,
            };
        });
    }));

    let mut row = Row::new().with_cross_axis_alignment(CrossAxisAlignment::Center);
    row = row.push(toggle);
    row = row.push_flex(FlexChild::expanded(Empty, 1));

    if mode == DatePickerMode::Day {
        // `_isDisplayingFirstMonth` / `_isDisplayingLastMonth`.
        let at_first = displayed <= first_date.first_of_month();
        let at_last = displayed >= last_date.first_of_month();
        for (index, delta, enabled) in [(998u64, -1i32, !at_first), (999, 1, !at_last)] {
            let color = if enabled { text } else { text.with_alpha(0x61) };
            let mut button = Pointer::new(
                id * 10000 + index,
                Container::new()
                    .with_size(40.0, 40.0)
                    .with_alignment(Alignment::CENTER)
                    .with_child(
                        Text::new(if delta < 0 { "<" } else { ">" })
                            .with_size(body_size + 2.0)
                            .with_weight(700)
                            .with_color(color),
                    ),
            );
            if enabled {
                let nav_handle = handle.clone();
                let on_displayed_month_changed = on_displayed_month_changed.clone();
                button = button.with_handlers(PointerHandlers::new().with_tap(move |_| {
                    let nav_handle = nav_handle.clone();
                    let on_displayed_month_changed = on_displayed_month_changed.clone();
                    nav_handle.set_state(move |state| {
                        // `_handleMonthPageChanged`: show the neighbouring
                        // month and report it.
                        let month = add_months_to_month_date(
                            state.displayed_month.unwrap_or(Date::new(1970, 1, 1)),
                            delta,
                        );
                        state.displayed_month = Some(month);
                        if let Some(changed) = &on_displayed_month_changed {
                            changed(month);
                        }
                    });
                }));
            }
            row = row.push(button);
        }
    }
    row
}

/// A calendar grid for picking a day, with a month/year header.
///
/// Anchor: `CalendarDatePicker` in calendar_date_picker.dart.
///
/// Hit-test ids are derived from `id`: day cells are `id * 10000 + day`, the
/// header buttons `id * 10000 + 997..=999`, and the year grid gets
/// `id * 10 + 1` as its own base id.
pub struct CalendarDatePicker {
    id: u64,
    initial_date: Option<Date>,
    first_date: Date,
    last_date: Date,
    current_date: Option<Date>,
    initial_calendar_mode: DatePickerMode,
    selectable_day_predicate: Option<SelectableDayPredicate>,
    on_date_changed: Option<Rc<dyn Fn(Date)>>,
    on_displayed_month_changed: Option<Rc<dyn Fn(Date)>>,
}

/// What a [`CalendarDatePicker`] remembers between frames.
///
/// Anchor: `_CalendarDatePickerState`.
#[derive(Default)]
pub struct CalendarDatePickerState {
    /// Day grid or year grid. Upstream's `_mode`.
    pub mode: DatePickerMode,
    /// The month on display, as its first day. Upstream's
    /// `_currentDisplayedMonthDate`.
    pub displayed_month: Option<Date>,
    /// The picked day. Upstream's `_selectedDate`.
    pub selected_date: Option<Date>,
}

impl CalendarDatePicker {
    /// `first_date` and `last_date` bound what may be picked; the picker
    /// opens on `initial_date`'s month, or on today's.
    pub fn new(id: u64, first_date: Date, last_date: Date) -> CalendarDatePicker {
        // Upstream's constructor asserts.
        debug_assert!(
            first_date <= last_date,
            "lastDate {last_date:?} must be on or after firstDate {first_date:?}"
        );
        CalendarDatePicker {
            id,
            initial_date: None,
            first_date,
            last_date,
            current_date: None,
            initial_calendar_mode: DatePickerMode::Day,
            selectable_day_predicate: None,
            on_date_changed: None,
            on_displayed_month_changed: None,
        }
    }

    pub fn with_initial_date(mut self, date: Option<Date>) -> Self {
        if let Some(date) = date {
            debug_assert!(
                date >= self.first_date && date <= self.last_date,
                "initialDate {date:?} must be within [{:?}, {:?}]",
                self.first_date,
                self.last_date,
            );
        }
        self.initial_date = date;
        self
    }

    /// The day ringed as "today"; [`Date::today`] when unset.
    pub fn with_current_date(mut self, date: Date) -> Self {
        self.current_date = Some(date);
        self
    }

    pub fn with_initial_calendar_mode(mut self, mode: DatePickerMode) -> Self {
        self.initial_calendar_mode = mode;
        self
    }

    pub fn with_selectable_day_predicate(mut self, predicate: SelectableDayPredicate) -> Self {
        self.selectable_day_predicate = Some(predicate);
        self
    }

    /// Called when the user picks a day. Upstream's `onDateChanged`.
    pub fn with_on_date_changed(mut self, changed: impl Fn(Date) + 'static) -> Self {
        self.on_date_changed = Some(Rc::new(changed));
        self
    }

    /// Called when the displayed month changes. Upstream's
    /// `onDisplayedMonthChanged`.
    pub fn with_on_displayed_month_changed(mut self, changed: impl Fn(Date) + 'static) -> Self {
        self.on_displayed_month_changed = Some(Rc::new(changed));
        self
    }
}

impl StatefulComponent for CalendarDatePicker {
    type State = CalendarDatePickerState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    /// `_CalendarDatePickerState.initState`.
    fn initial_state(&self) -> CalendarDatePickerState {
        let displayed = self
            .initial_date
            .unwrap_or_else(|| self.current_date.unwrap_or_else(Date::today))
            .first_of_month();
        CalendarDatePickerState {
            mode: self.initial_calendar_mode,
            displayed_month: Some(displayed),
            selected_date: self.initial_date,
        }
    }

    fn build(
        &self,
        state: &CalendarDatePickerState,
        handle: StateHandle<CalendarDatePickerState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let mode = state.mode;
        let selected = state.selected_date;
        let displayed = state
            .displayed_month
            .unwrap_or(self.first_date.first_of_month());
        let id = self.id;
        let (first_date, last_date) = (self.first_date, self.last_date);
        let current_date = self.current_date.unwrap_or_else(Date::today);
        let predicate = self.selectable_day_predicate;

        let content: AnyWidget = match mode {
            DatePickerMode::Day => {
                let palette = DayCellPalette {
                    body_size: theme.body_size,
                    text: theme.text,
                    primary: theme.primary,
                    on_primary: theme.on_primary,
                };
                let weekday_style = theme.muted();
                let on_date_changed = self.on_date_changed.clone();
                let grid_handle = handle.clone();
                leaf(move || {
                    Container::new()
                        .with_padding(EdgeInsets::symmetric(MONTH_PICKER_HORIZONTAL_PADDING, 0.0))
                        .with_child(day_grid(
                            id,
                            displayed,
                            selected,
                            first_date,
                            last_date,
                            current_date,
                            predicate,
                            grid_handle.clone(),
                            on_date_changed.clone(),
                            weekday_style.clone(),
                            palette.clone(),
                        ))
                })
            }
            DatePickerMode::Year => {
                let year_handle = handle.clone();
                let on_date_changed = self.on_date_changed.clone();
                stateful(
                    YearPicker::new(id * 10 + 1, first_date, last_date)
                        .with_current_date(current_date)
                        .with_selected_date(selected.or(Some(displayed)))
                        .with_on_changed(move |month_date| {
                            // `_CalendarDatePickerState._handleYearChanged`.
                            let on_date_changed = on_date_changed.clone();
                            year_handle.set_state(move |state| {
                                let days = days_in_month(month_date.year, month_date.month);
                                let preferred_day =
                                    state.selected_date.map_or(1, |d| d.day).min(days);
                                let mut value = Date::new(
                                    month_date.year,
                                    month_date.month as i32,
                                    preferred_day as i32,
                                );
                                value = value.clamp(first_date, last_date);
                                state.mode = DatePickerMode::Day;
                                state.displayed_month = Some(value.first_of_month());
                                if predicate.is_none_or(|p| p(value)) {
                                    state.selected_date = Some(value);
                                    if let Some(changed) = &on_date_changed {
                                        changed(value);
                                    }
                                }
                            });
                        }),
                )
            }
        };

        let body_size = theme.body_size;
        let text = theme.text;
        let on_displayed_month_changed = self.on_displayed_month_changed.clone();
        let subheader_handle = handle.clone();
        many(vec![content], move |mut rendered| {
            let content = rendered.pop().unwrap_or_else(|| RenderRef::new(Empty));
            Box::new(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(
                        Container::new()
                            .with_height(SUB_HEADER_HEIGHT)
                            .with_padding(EdgeInsets::symmetric(
                                MONTH_PICKER_HORIZONTAL_PADDING + 4.0,
                                0.0,
                            ))
                            .with_child(calendar_subheader(
                                id,
                                mode,
                                displayed,
                                first_date,
                                last_date,
                                subheader_handle.clone(),
                                on_displayed_month_changed.clone(),
                                body_size,
                                text,
                            )),
                    )
                    .push(content),
            )
        })
    }
}

/// A scrollable grid of years to pick one from.
///
/// Anchor: `YearPicker` in calendar_date_picker.dart, including the backfill
/// of disabled cells when fewer than `_YearPickerState.minYears` years fit
/// the range and the initial scroll that centres the selected year
/// (`_scrollOffsetForYear`). Cells are `id * 10000 + index`.
pub struct YearPicker {
    id: u64,
    first_date: Date,
    last_date: Date,
    current_date: Option<Date>,
    selected_date: Option<Date>,
    on_changed: Option<Rc<dyn Fn(Date)>>,
}

/// What a [`YearPicker`] remembers: how far it is scrolled.
///
/// Anchor: `_YearPickerState`'s `ScrollController`.
#[derive(Default)]
pub struct YearPickerState {
    pub scroll: Scroll,
}

impl YearPicker {
    pub fn new(id: u64, first_date: Date, last_date: Date) -> YearPicker {
        debug_assert!(first_date <= last_date);
        YearPicker {
            id,
            first_date,
            last_date,
            current_date: None,
            selected_date: None,
            on_changed: None,
        }
    }

    pub fn with_current_date(mut self, date: Date) -> Self {
        self.current_date = Some(date);
        self
    }

    pub fn with_selected_date(mut self, date: Option<Date>) -> Self {
        self.selected_date = date;
        self
    }

    /// Called with the first of the picked year's month (the selected date's
    /// month where there is one). Upstream's `onChanged`.
    pub fn with_on_changed(mut self, changed: impl Fn(Date) + 'static) -> Self {
        self.on_changed = Some(Rc::new(changed));
        self
    }

    /// `_YearPickerState._itemCount`.
    fn item_count(&self) -> i32 {
        self.last_date.year - self.first_date.year + 1
    }

    /// `_YearPickerState._scrollOffsetForYear`: the selected year two rows
    /// down from the top, so it sits near the middle.
    fn scroll_offset_for_year(&self, date: Date) -> f32 {
        if self.item_count() < MIN_YEARS {
            return 0.0;
        }
        let year_row = (date.year - self.first_date.year) / YEAR_PICKER_COLUMN_COUNT as i32;
        (year_row - 2).max(0) as f32 * YEAR_PICKER_ROW_HEIGHT
    }
}

impl StatefulComponent for YearPicker {
    type State = YearPickerState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    /// `_YearPickerState.initState`: the list opens with the selected year
    /// centred.
    fn initial_state(&self) -> YearPickerState {
        let mut state = YearPickerState::default();
        state
            .scroll
            .jump_to(self.scroll_offset_for_year(self.selected_date.unwrap_or(self.first_date)));
        state
    }

    fn advance(&self, state: &mut YearPickerState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &YearPickerState,
        handle: StateHandle<YearPickerState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let offset = state.scroll.offset;
        let extent_sink = state.scroll.extent.clone();
        let current_date = self.current_date.unwrap_or_else(Date::today);
        let selected_year = self.selected_date.map(|d| d.year);
        let selected_month = self.selected_date.map(|d| d.month);
        let on_changed = self.on_changed.clone();
        let (first_date, last_date) = (self.first_date, self.last_date);
        let id = self.id;
        let body_size = theme.body_size;
        let (text, primary, on_primary) = (theme.text, theme.primary, theme.on_primary);

        // The same wiring a page gives a scrollable: drag, throw, wheel --
        // see the gallery's `scroll_handlers` for the pattern's source.
        let down_handle = handle.clone();
        let drag_handle = handle.clone();
        let end_handle = handle.clone();
        let wheel_handle = handle.clone();
        let scroll_handlers = PointerHandlers::new()
            .with_pointer_down(move |_| {
                down_handle.set_state(|state| state.scroll.stop());
            })
            .with_drag_update(move |drag| {
                drag_handle.set_state(move |state| state.scroll.scroll_by(-drag.delta.dy));
            })
            .with_drag_end(move |end| {
                end_handle.set_state(move |state| state.scroll.fling(-end.velocity.dy));
            })
            .with_scroll(move |scroll| {
                wheel_handle.set_state(move |state| state.scroll.scroll_by(scroll.delta.dy));
            });

        leaf(move || {
            // `_YearPickerState.build`: dividers around a grid of year cells.
            let extent_sink = extent_sink.clone();
            let mut list = ListView::new()
                .with_offset(offset)
                .with_extent_sink(extent_sink);
            let count = item_count_of(first_date, last_date);
            let backfill = if count < MIN_YEARS {
                (MIN_YEARS - count) / 2
            } else {
                0
            };
            let rows = (count + 2 * backfill) as usize / YEAR_PICKER_COLUMN_COUNT
                + usize::from((count + 2 * backfill) as usize % YEAR_PICKER_COLUMN_COUNT > 0);
            for row_index in 0..rows {
                let mut row = Row::new().with_spacing(YEAR_PICKER_ROW_SPACING);
                for column in 0..YEAR_PICKER_COLUMN_COUNT {
                    let index = (row_index * YEAR_PICKER_COLUMN_COUNT + column) as i32;
                    row = row.push_flex(FlexChild::expanded(
                        year_cell(
                            id * 10000 + index as u64,
                            index,
                            backfill,
                            first_date,
                            last_date,
                            current_date,
                            selected_year,
                            selected_month,
                            on_changed.clone(),
                            body_size,
                            text,
                            primary,
                            on_primary,
                        ),
                        1,
                    ));
                }
                list = list.push(
                    Container::new()
                        .with_height(YEAR_PICKER_ROW_HEIGHT)
                        .with_alignment(Alignment::CENTER)
                        .with_child(row),
                );
            }
            Pointer::new(
                id * 10000 + 9990,
                Container::new()
                    .with_padding(EdgeInsets::symmetric(YEAR_PICKER_PADDING, 0.0))
                    .with_child(list),
            )
            .with_handlers(scroll_handlers.clone())
        })
    }
}

/// `YearPicker`'s `_itemCount` as a free function, for the build closure.
fn item_count_of(first_date: Date, last_date: Date) -> i32 {
    last_date.year - first_date.year + 1
}

/// One cell of the year grid.
///
/// Anchor: `_YearPickerState._buildYearItem`: a 72x36 stadium, filled for
/// the selected year, ringed for the current one, greyed and untappable
/// when outside the range (the backfilled cells included).
#[allow(clippy::too_many_arguments)]
fn year_cell(
    id: u64,
    index: i32,
    backfill: i32,
    first_date: Date,
    last_date: Date,
    current_date: Date,
    selected_year: Option<i32>,
    selected_month: Option<u32>,
    on_changed: Option<Rc<dyn Fn(Date)>>,
    body_size: f32,
    text: Color,
    primary: Color,
    on_primary: Color,
) -> crate::render::RenderPointerRegion {
    let year = first_date.year + index - backfill;
    let disabled = year < first_date.year || year > last_date.year;
    let selected = Some(year) == selected_year;
    let is_current = year == current_date.year;

    let text_color = if disabled {
        text.with_alpha(0x61)
    } else if selected {
        on_primary
    } else if is_current {
        primary
    } else {
        text
    };
    let mut decoration = Container::new()
        .with_size(72.0, 36.0)
        // The stadium: as round as the cell is tall.
        .with_corner_radius(18.0)
        .with_alignment(Alignment::CENTER)
        .with_child(
            Text::new(format!("{year}"))
                .with_size(body_size)
                .with_color(text_color),
        );
    if selected {
        decoration = decoration.with_color(primary);
    }
    if is_current && !selected {
        decoration = decoration.with_border(1.0, text_color);
    }
    let cell = Container::new()
        .with_height(YEAR_PICKER_ROW_HEIGHT - 4.0)
        .with_alignment(Alignment::CENTER)
        .with_child(decoration);

    let mut region = Pointer::new(id, cell);
    if !disabled {
        region = region.with_handlers(PointerHandlers::new().with_tap(move |_| {
            if let Some(changed) = &on_changed {
                changed(month_in_year(year, selected_month, first_date, last_date));
            }
        }));
    }
    region
}

/// The month a year tap reports: the selected date's month (January when
/// nothing is selected), clamped into the range the way upstream's
/// `_YearPickerState._buildYearItem` clamps it.
fn month_in_year(
    year: i32,
    selected_month: Option<u32>,
    first_date: Date,
    last_date: Date,
) -> Date {
    // `DateTime.january`.
    let mut date = Date::new(year, selected_month.unwrap_or(1) as i32, 1);
    if date < first_date.first_of_month() {
        date = first_date.first_of_month();
    } else if date > last_date {
        date = last_date.first_of_month();
    }
    date
}

// -- The date input form ---------------------------------------------------------
//
// Anchor: `InputDatePickerFormField` in input_date_picker_form_field.dart.

/// Default texts, compiled in for `en_US`. Anchors: the like-named getters
/// of `DefaultMaterialLocalizations` in material_localizations.dart.
const INVALID_DATE_FORMAT_LABEL: &str = "Invalid format.";
const DATE_OUT_OF_RANGE_LABEL: &str = "Out of range.";
const DATE_HELP_TEXT: &str = "mm/dd/yyyy";
const DATE_INPUT_LABEL: &str = "Enter Date";
const DATE_PICKER_HELP_TEXT: &str = "Select date";
const CANCEL_BUTTON_LABEL: &str = "Cancel";
const OK_BUTTON_LABEL: &str = "OK";
const SAVE_BUTTON_LABEL: &str = "Save";
const DATE_RANGE_PICKER_HELP_TEXT: &str = "Select range";
const INVALID_DATE_RANGE_LABEL: &str = "Invalid range.";
const DATE_RANGE_START_LABEL: &str = "Start date";
const DATE_RANGE_END_LABEL: &str = "End date";
const TIME_PICKER_DIAL_HELP_TEXT: &str = "Select time";
const TIME_PICKER_INPUT_HELP_TEXT: &str = "Enter time";
const INVALID_TIME_LABEL: &str = "Enter a valid time";
const TIME_PICKER_HOUR_LABEL: &str = "Hour";
const TIME_PICKER_MINUTE_LABEL: &str = "Minute";

/// Parses and validates a date text, the shared body of
/// `InputDatePickerFormField`'s `_parseDate`/`_validateDate`/
/// `_isValidAcceptableDate`. On success the parsed date; on failure the
/// error string the field should show.
fn validate_date_text(
    text: &str,
    first_date: Date,
    last_date: Date,
    predicate: Option<SelectableDayPredicate>,
    error_format_text: Option<&str>,
    error_invalid_text: Option<&str>,
) -> Result<Date, String> {
    match parse_compact_date(text) {
        None => Err(error_format_text
            .unwrap_or(INVALID_DATE_FORMAT_LABEL)
            .to_string()),
        Some(date)
            if date < first_date || date > last_date || predicate.is_some_and(|p| !p(date)) =>
        {
            Err(error_invalid_text
                .unwrap_or(DATE_OUT_OF_RANGE_LABEL)
                .to_string())
        }
        Some(date) => Ok(date),
    }
}

/// A text field for entering a date.
///
/// Anchor: `InputDatePickerFormField` in input_date_picker_form_field.dart.
///
/// Deviations, both because this crate's [`TextField`] is a bare field with
/// no `InputDecorator` around it:
///
/// - The label (`fieldLabelText`, "Enter Date" by default) sits above the
///   field rather than on its border, and the error text under it.
/// - The field cannot be pre-filled with the initial date the way upstream
///   pre-fills from the controller; it starts empty, with the hint as its
///   placeholder, and the current selection is visible in the dialog header.
pub struct InputDatePickerFormField {
    id: u64,
    initial_date: Option<Date>,
    first_date: Date,
    last_date: Date,
    selectable_day_predicate: Option<SelectableDayPredicate>,
    error_format_text: Option<String>,
    error_invalid_text: Option<String>,
    field_hint_text: Option<String>,
    field_label_text: Option<String>,
    on_date_submitted: Option<Rc<dyn Fn(Date)>>,
    on_date_saved: Option<Rc<dyn Fn(Date)>>,
    /// Reports the raw text on every change.
    ///
    /// This is NOT an upstream callback: upstream's dialog reaches the
    /// field's text through `Form.save()` when OK is pressed, and this crate
    /// has no `Form`. Mirroring the text as it changes lets the dialog run
    /// the same parse and validate itself at that moment.
    on_text_changed: Option<Rc<dyn Fn(String)>>,
}

/// What an [`InputDatePickerFormField`] remembers.
///
/// Anchor: `_InputDatePickerFormFieldState`.
#[derive(Default)]
pub struct InputDatePickerState {
    /// The text as last typed. Upstream's `_inputText`.
    pub text: String,
    /// The validation error on show, if the last submit failed.
    pub error: Option<String>,
    /// The last valid date entered. Upstream's `_selectedDate`.
    pub selected: Option<Date>,
}

impl InputDatePickerFormField {
    pub fn new(id: u64, first_date: Date, last_date: Date) -> InputDatePickerFormField {
        debug_assert!(first_date <= last_date);
        InputDatePickerFormField {
            id,
            initial_date: None,
            first_date,
            last_date,
            selectable_day_predicate: None,
            error_format_text: None,
            error_invalid_text: None,
            field_hint_text: None,
            field_label_text: None,
            on_date_submitted: None,
            on_date_saved: None,
            on_text_changed: None,
        }
    }

    pub fn with_initial_date(mut self, date: Option<Date>) -> Self {
        self.initial_date = date;
        self
    }

    pub fn with_selectable_day_predicate(mut self, predicate: SelectableDayPredicate) -> Self {
        self.selectable_day_predicate = Some(predicate);
        self
    }

    pub fn with_error_format_text(mut self, text: impl Into<String>) -> Self {
        self.error_format_text = Some(text.into());
        self
    }

    pub fn with_error_invalid_text(mut self, text: impl Into<String>) -> Self {
        self.error_invalid_text = Some(text.into());
        self
    }

    pub fn with_field_hint_text(mut self, text: impl Into<String>) -> Self {
        self.field_hint_text = Some(text.into());
        self
    }

    pub fn with_field_label_text(mut self, text: impl Into<String>) -> Self {
        self.field_label_text = Some(text.into());
        self
    }

    /// Called with a valid date when the field is submitted (Enter).
    /// Upstream's `onDateSubmitted`.
    pub fn with_on_date_submitted(mut self, submitted: impl Fn(Date) + 'static) -> Self {
        self.on_date_submitted = Some(Rc::new(submitted));
        self
    }

    /// Called with a valid date when the field is saved. Upstream's
    /// `onDateSaved`; with no `Form` here, saved means the same moment as
    /// submitted, and the dialog validates again at OK.
    pub fn with_on_date_saved(mut self, saved: impl Fn(Date) + 'static) -> Self {
        self.on_date_saved = Some(Rc::new(saved));
        self
    }

    /// See the field; not upstream, see [`InputDatePickerFormField`].
    pub fn with_on_text_changed(mut self, changed: impl Fn(String) + 'static) -> Self {
        self.on_text_changed = Some(Rc::new(changed));
        self
    }
}

impl StatefulComponent for InputDatePickerFormField {
    type State = InputDatePickerState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn initial_state(&self) -> InputDatePickerState {
        InputDatePickerState {
            selected: self.initial_date,
            ..InputDatePickerState::default()
        }
    }

    fn build(
        &self,
        state: &InputDatePickerState,
        handle: StateHandle<InputDatePickerState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let (first_date, last_date) = (self.first_date, self.last_date);
        let predicate = self.selectable_day_predicate;
        let error_format_text = self.error_format_text.clone();
        let error_invalid_text = self.error_invalid_text.clone();
        let on_date_submitted = self.on_date_submitted.clone();
        let on_date_saved = self.on_date_saved.clone();
        let on_text_changed = self.on_text_changed.clone();
        let error = state.error.clone();

        let changed_handle = handle.clone();
        let submit_handle = handle.clone();
        let field = stateful(
            TextField::new(self.id * 10 + 1)
                .with_placeholder(
                    self.field_hint_text
                        .clone()
                        .unwrap_or_else(|| DATE_HELP_TEXT.into()),
                )
                .with_on_changed(move |text| {
                    if let Some(changed) = &on_text_changed {
                        changed(text.to_string());
                    }
                    let text = text.to_string();
                    changed_handle.set_state(move |state| {
                        state.text = text;
                    });
                })
                .with_on_submitted(move |text| {
                    // `_handleSubmitted`/`_handleSaved`, one path here.
                    let result = validate_date_text(
                        text,
                        first_date,
                        last_date,
                        predicate,
                        error_format_text.as_deref(),
                        error_invalid_text.as_deref(),
                    );
                    let on_date_submitted = on_date_submitted.clone();
                    let on_date_saved = on_date_saved.clone();
                    submit_handle.set_state(move |state| match result {
                        Ok(date) => {
                            state.selected = Some(date);
                            state.error = None;
                            if let Some(saved) = &on_date_saved {
                                saved(date);
                            }
                            if let Some(submitted) = &on_date_submitted {
                                submitted(date);
                            }
                        }
                        Err(message) => state.error = Some(message),
                    });
                }),
        );

        let label = self
            .field_label_text
            .clone()
            .unwrap_or_else(|| DATE_INPUT_LABEL.into());
        let muted = theme.muted();
        let error_style = TextStyle {
            font_size: theme.body_size - 2.0,
            color: theme.danger,
            ..TextStyle::default()
        };
        many(vec![field], move |mut rendered| {
            let field = rendered.pop().unwrap_or_else(|| RenderRef::new(Empty));
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(4.0)
                .push(Text::new(label.clone()).with_style(muted.clone()))
                .push(field);
            if let Some(error) = &error {
                column = column.push(Text::new(error.clone()).with_style(error_style.clone()));
            }
            Box::new(column)
        })
    }
}

// -- The date picker dialog ------------------------------------------------------
//
// Anchors: `DatePickerDialog`/`_DatePickerDialogState`/`_DatePickerHeader`
// in date_picker.dart. The dialog sizes are the M3 constants
// `_calendarPortraitDialogSizeM3` & friends; the M2 column of the size table
// is not ported.

const CALENDAR_PORTRAIT_DIALOG_SIZE: (f32, f32) = (360.0, 568.0);
const CALENDAR_LANDSCAPE_DIALOG_SIZE: (f32, f32) = (496.0, 346.0);
const INPUT_PORTRAIT_DIALOG_SIZE: (f32, f32) = (328.0, 270.0);
const INPUT_LANDSCAPE_DIALOG_SIZE: (f32, f32) = (496.0, 160.0);
/// `_DatePickerHeader._datePickerHeaderPortraitHeight`.
const HEADER_PORTRAIT_HEIGHT: f32 = 120.0;
/// `_DatePickerHeader._datePickerHeaderLandscapeWidth`.
const HEADER_LANDSCAPE_WIDTH: f32 = 152.0;
/// `_inputFormPortraitHeight`.
const INPUT_FORM_PORTRAIT_HEIGHT: f32 = 98.0;
/// `_inputFormLandscapeHeight`.
const INPUT_FORM_LANDSCAPE_HEIGHT: f32 = 108.0;

/// `_DatePickerDialogState._dialogSize`, the M3 column of the table.
fn date_dialog_size(entry_mode: DatePickerEntryMode, orientation: Orientation) -> (f32, f32) {
    let is_calendar = matches!(
        entry_mode,
        DatePickerEntryMode::Calendar | DatePickerEntryMode::CalendarOnly
    );
    match (is_calendar, orientation) {
        (true, Orientation::Portrait) => CALENDAR_PORTRAIT_DIALOG_SIZE,
        (false, Orientation::Portrait) => INPUT_PORTRAIT_DIALOG_SIZE,
        (true, Orientation::Landscape) => CALENDAR_LANDSCAPE_DIALOG_SIZE,
        (false, Orientation::Landscape) => INPUT_LANDSCAPE_DIALOG_SIZE,
    }
}

/// `_DatePickerDialogState._handleOk`, as a free function so the button's
/// tap closure can call it without borrowing the widget.
#[allow(clippy::too_many_arguments)]
fn date_dialog_handle_ok(
    state: &mut DatePickerDialogState,
    first_date: Date,
    last_date: Date,
    predicate: Option<SelectableDayPredicate>,
    error_format_text: Option<&str>,
    error_invalid_text: Option<&str>,
    on_confirm: Option<&Rc<dyn Fn(Date)>>,
    on_cancel: Option<&Rc<dyn Fn()>>,
) {
    if matches!(
        state.entry_mode,
        DatePickerEntryMode::Input | DatePickerEntryMode::InputOnly
    ) {
        // The input mode confirms what the field holds. An untouched field
        // stands for the current selection: upstream pre-fills the field's
        // controller from it, and this is that pre-fill's stand-in (see
        // InputDatePickerFormField's header note).
        if state.input_text.is_empty() {
            match state.selected_date {
                Some(date) => {
                    if let Some(confirm) = on_confirm {
                        confirm(date);
                    }
                }
                None => {
                    if let Some(cancel) = on_cancel {
                        cancel();
                    }
                }
            }
            return;
        }
        // `form.validate()` then `form.save()`.
        match validate_date_text(
            &state.input_text,
            first_date,
            last_date,
            predicate,
            error_format_text,
            error_invalid_text,
        ) {
            Ok(date) => {
                state.selected_date = Some(date);
                state.input_error = None;
                if let Some(confirm) = on_confirm {
                    confirm(date);
                }
            }
            // The failed validate flips autovalidate on; the error string
            // under the field is that state here.
            Err(message) => state.input_error = Some(message),
        }
        return;
    }
    // `Navigator.pop(context, _selectedDate.value)`: the date when there is
    // one, the null pop -- the cancel callback -- when there is not.
    match state.selected_date {
        Some(date) => {
            if let Some(confirm) = on_confirm {
                confirm(date);
            }
        }
        None => {
            if let Some(cancel) = on_cancel {
                cancel();
            }
        }
    }
}

/// `_DatePickerDialogState._handleEntryModeToggle`, free for the same reason.
fn date_dialog_entry_mode_toggle(
    state: &mut DatePickerDialogState,
    first_date: Date,
    last_date: Date,
    predicate: Option<SelectableDayPredicate>,
    on_mode_change: Option<&Rc<dyn Fn(DatePickerEntryMode)>>,
) {
    state.entry_mode = match state.entry_mode {
        DatePickerEntryMode::Calendar => {
            state.input_error = None;
            DatePickerEntryMode::Input
        }
        DatePickerEntryMode::Input => {
            // Upstream saves the form here, keeping a valid entry.
            if let Ok(date) = validate_date_text(
                &state.input_text,
                first_date,
                last_date,
                predicate,
                None,
                None,
            ) {
                state.selected_date = Some(date);
            }
            DatePickerEntryMode::Calendar
        }
        // Upstream asserts this is unreachable.
        mode => mode,
    };
    if let Some(changed) = on_mode_change {
        changed(state.entry_mode);
    }
}

/// A Material-style date picker dialog.
///
/// Anchor: `DatePickerDialog` in date_picker.dart.
///
/// There is no `Navigator` to pop the result through: `with_on_confirm`
/// hears the picked date when OK is pressed and `with_on_cancel` when the
/// dialog is dismissed (Cancel, or the scrim when shown through
/// [`show_date_picker`]). Pressing OK with nothing picked is upstream's null
/// pop, and lands on `on_cancel` here.
///
/// Hit-test ids derive from `id`: the calendar is `id * 10 + 1`, the input
/// field `id * 10 + 2`, the mode toggle `id * 10 + 3`, OK `id * 10 + 4`,
/// Cancel `id * 10 + 5`.
pub struct DatePickerDialog {
    id: u64,
    initial_date: Option<Date>,
    first_date: Date,
    last_date: Date,
    current_date: Option<Date>,
    initial_entry_mode: DatePickerEntryMode,
    selectable_day_predicate: Option<SelectableDayPredicate>,
    cancel_text: Option<String>,
    confirm_text: Option<String>,
    help_text: Option<String>,
    initial_calendar_mode: DatePickerMode,
    error_format_text: Option<String>,
    error_invalid_text: Option<String>,
    field_hint_text: Option<String>,
    field_label_text: Option<String>,
    on_date_picker_mode_change: Option<Rc<dyn Fn(DatePickerEntryMode)>>,
    on_confirm: Option<Rc<dyn Fn(Date)>>,
    on_cancel: Option<Rc<dyn Fn()>>,
}

/// What a [`DatePickerDialog`] remembers between frames.
///
/// Anchor: `_DatePickerDialogState`.
#[derive(Default)]
pub struct DatePickerDialogState {
    /// Upstream's `_entryMode`.
    pub entry_mode: DatePickerEntryMode,
    /// Upstream's `_selectedDate`.
    pub selected_date: Option<Date>,
    /// The input field's text, mirrored as it changes (see
    /// [`InputDatePickerFormField::with_on_text_changed`] for why).
    pub input_text: String,
    /// The validation error showing under the input. Setting it is upstream
    /// flipping `_autovalidateMode` to `always` after a failed OK.
    pub input_error: Option<String>,
}

impl DatePickerDialog {
    pub fn new(id: u64, first_date: Date, last_date: Date) -> DatePickerDialog {
        debug_assert!(first_date <= last_date);
        DatePickerDialog {
            id,
            initial_date: None,
            first_date,
            last_date,
            current_date: None,
            initial_entry_mode: DatePickerEntryMode::Calendar,
            selectable_day_predicate: None,
            cancel_text: None,
            confirm_text: None,
            help_text: None,
            initial_calendar_mode: DatePickerMode::Day,
            error_format_text: None,
            error_invalid_text: None,
            field_hint_text: None,
            field_label_text: None,
            on_date_picker_mode_change: None,
            on_confirm: None,
            on_cancel: None,
        }
    }

    pub fn with_initial_date(mut self, date: Option<Date>) -> Self {
        self.initial_date = date;
        self
    }

    /// The day ringed as "today"; [`Date::today`] when unset.
    pub fn with_current_date(mut self, date: Date) -> Self {
        self.current_date = Some(date);
        self
    }

    pub fn with_initial_entry_mode(mut self, mode: DatePickerEntryMode) -> Self {
        self.initial_entry_mode = mode;
        self
    }

    pub fn with_selectable_day_predicate(mut self, predicate: SelectableDayPredicate) -> Self {
        self.selectable_day_predicate = Some(predicate);
        self
    }

    pub fn with_cancel_text(mut self, text: impl Into<String>) -> Self {
        self.cancel_text = Some(text.into());
        self
    }

    pub fn with_confirm_text(mut self, text: impl Into<String>) -> Self {
        self.confirm_text = Some(text.into());
        self
    }

    pub fn with_help_text(mut self, text: impl Into<String>) -> Self {
        self.help_text = Some(text.into());
        self
    }

    pub fn with_initial_calendar_mode(mut self, mode: DatePickerMode) -> Self {
        self.initial_calendar_mode = mode;
        self
    }

    pub fn with_error_format_text(mut self, text: impl Into<String>) -> Self {
        self.error_format_text = Some(text.into());
        self
    }

    pub fn with_error_invalid_text(mut self, text: impl Into<String>) -> Self {
        self.error_invalid_text = Some(text.into());
        self
    }

    pub fn with_field_hint_text(mut self, text: impl Into<String>) -> Self {
        self.field_hint_text = Some(text.into());
        self
    }

    pub fn with_field_label_text(mut self, text: impl Into<String>) -> Self {
        self.field_label_text = Some(text.into());
        self
    }

    /// Called when the entry mode toggles. Upstream's
    /// `onDatePickerModeChange`.
    pub fn with_on_date_picker_mode_change(
        mut self,
        changed: impl Fn(DatePickerEntryMode) + 'static,
    ) -> Self {
        self.on_date_picker_mode_change = Some(Rc::new(changed));
        self
    }

    /// Called with the picked date when OK is pressed. Replaces the value
    /// upstream's `Navigator.pop(context, _selectedDate.value)` delivers.
    pub fn with_on_confirm(mut self, confirm: impl Fn(Date) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(confirm));
        self
    }

    /// Called when the dialog is dismissed without a date. Replaces
    /// upstream's null pop.
    pub fn with_on_cancel(mut self, cancel: impl Fn() + 'static) -> Self {
        self.on_cancel = Some(Rc::new(cancel));
        self
    }
}

impl StatefulComponent for DatePickerDialog {
    type State = DatePickerDialogState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    /// `_DatePickerDialogState`'s field initializers.
    fn initial_state(&self) -> DatePickerDialogState {
        DatePickerDialogState {
            entry_mode: self.initial_entry_mode,
            selected_date: self.initial_date,
            input_text: String::new(),
            input_error: None,
        }
    }

    fn build(
        &self,
        state: &DatePickerDialogState,
        handle: StateHandle<DatePickerDialogState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let orientation = orientation_of(context);
        let entry_mode = state.entry_mode;
        let (width, height) = date_dialog_size(entry_mode, orientation);
        let is_calendar = matches!(
            entry_mode,
            DatePickerEntryMode::Calendar | DatePickerEntryMode::CalendarOnly
        );

        // -- The entry-mode toggle of `_DatePickerDialogState.build`: an icon
        // upstream (`edit_outlined` / `calendar_today`), a short text button
        // here -- the Material icons font is not loaded. `calendarOnly` and
        // `inputOnly` have no toggle.
        let can_toggle = matches!(
            entry_mode,
            DatePickerEntryMode::Calendar | DatePickerEntryMode::Input
        );
        let toggle: AnyWidget = {
            let label = if is_calendar { "Edit" } else { "Calendar" };
            let toggle_handle = handle.clone();
            let (first_date, last_date, predicate) = (
                self.first_date,
                self.last_date,
                self.selectable_day_predicate,
            );
            let on_mode_change = self.on_date_picker_mode_change.clone();
            let id = self.id * 10 + 3;
            let color = theme.text_muted;
            leaf(move || {
                let tap_handle = toggle_handle.clone();
                let on_mode_change = on_mode_change.clone();
                Pointer::new(
                    id,
                    Container::new()
                        .with_padding(EdgeInsets::symmetric(8.0, 4.0))
                        .with_child(
                            Text::new(label)
                                .with_size(13.0)
                                .with_weight(500)
                                .with_color(color),
                        ),
                )
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    let on_mode_change = on_mode_change.clone();
                    tap_handle.set_state(move |state| {
                        date_dialog_entry_mode_toggle(
                            state,
                            first_date,
                            last_date,
                            predicate,
                            on_mode_change.as_ref(),
                        );
                    });
                }))
            })
        };

        // -- The content: the calendar grid or the input form.
        let content: AnyWidget = if is_calendar {
            let calendar_handle = handle.clone();
            let mut calendar =
                CalendarDatePicker::new(self.id * 10 + 1, self.first_date, self.last_date)
                    .with_initial_date(state.selected_date)
                    .with_current_date(self.current_date.unwrap_or_else(Date::today))
                    .with_initial_calendar_mode(self.initial_calendar_mode)
                    .with_on_date_changed(move |date| {
                        // `_handleDateChanged`.
                        calendar_handle.set_state(move |state| state.selected_date = Some(date));
                    });
            if let Some(predicate) = self.selectable_day_predicate {
                calendar = calendar.with_selectable_day_predicate(predicate);
            }
            stateful(calendar)
        } else {
            let text_handle = handle.clone();
            let submitted_handle = handle.clone();
            let mut field =
                InputDatePickerFormField::new(self.id * 10 + 2, self.first_date, self.last_date)
                    .with_initial_date(state.selected_date)
                    .with_on_text_changed(move |text| {
                        text_handle.set_state(move |state| state.input_text = text);
                    })
                    .with_on_date_submitted(move |date| {
                        submitted_handle.set_state(move |state| state.selected_date = Some(date));
                    });
            if let Some(predicate) = self.selectable_day_predicate {
                field = field.with_selectable_day_predicate(predicate);
            }
            if let Some(text) = &self.error_format_text {
                field = field.with_error_format_text(text.clone());
            }
            if let Some(text) = &self.error_invalid_text {
                field = field.with_error_invalid_text(text.clone());
            }
            if let Some(text) = &self.field_hint_text {
                field = field.with_field_hint_text(text.clone());
            }
            if let Some(text) = &self.field_label_text {
                field = field.with_field_label_text(text.clone());
            }
            stateful(field)
        };

        // -- The actions: CANCEL and OK, text buttons end-aligned.
        let ok: AnyWidget = {
            let ok_handle = handle.clone();
            let (first_date, last_date, predicate) = (
                self.first_date,
                self.last_date,
                self.selectable_day_predicate,
            );
            let error_format_text = self.error_format_text.clone();
            let error_invalid_text = self.error_invalid_text.clone();
            let on_confirm = self.on_confirm.clone();
            let on_cancel = self.on_cancel.clone();
            component(
                Button::new(
                    self.id * 10 + 4,
                    self.confirm_text
                        .clone()
                        .unwrap_or_else(|| OK_BUTTON_LABEL.into()),
                )
                .with_style(ButtonVariant::Text)
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    let error_format_text = error_format_text.clone();
                    let error_invalid_text = error_invalid_text.clone();
                    let on_confirm = on_confirm.clone();
                    let on_cancel = on_cancel.clone();
                    ok_handle.set_state(move |state| {
                        date_dialog_handle_ok(
                            state,
                            first_date,
                            last_date,
                            predicate,
                            error_format_text.as_deref(),
                            error_invalid_text.as_deref(),
                            on_confirm.as_ref(),
                            on_cancel.as_ref(),
                        );
                    });
                })),
            )
        };
        let cancel: AnyWidget = {
            let on_cancel = self.on_cancel.clone();
            component(
                Button::new(
                    self.id * 10 + 5,
                    self.cancel_text
                        .clone()
                        .unwrap_or_else(|| CANCEL_BUTTON_LABEL.into()),
                )
                .with_style(ButtonVariant::Text)
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    if let Some(cancel) = &on_cancel {
                        cancel();
                    }
                })),
            )
        };

        let title_text = state
            .selected_date
            .map(format_medium_date)
            .unwrap_or_default();
        let help_text = self
            .help_text
            .clone()
            .unwrap_or_else(|| DATE_PICKER_HELP_TEXT.into());
        let input_error = state.input_error.clone();
        let surface = theme.surface;
        let outline = theme.outline;
        let text_color = theme.text;
        let muted = theme.text_muted;
        let error_color = theme.danger;
        let body_size = theme.body_size;

        many(vec![toggle, content, ok, cancel], move |mut rendered| {
            let toggle = rendered.remove(0);
            let content = rendered.remove(0);
            let ok = rendered.remove(0);
            let cancel = rendered.remove(0);

            // `_DatePickerHeader`: help text over the selected date, the
            // toggle trailing. Portrait is a 120-tall band, landscape a
            // 152-wide column.
            let mut title_row = Row::new().with_cross_axis_alignment(CrossAxisAlignment::Center);
            title_row = title_row.push_flex(FlexChild::expanded(
                Text::new(title_text.clone())
                    .with_size(32.0)
                    .with_color(text_color),
                1,
            ));
            if can_toggle {
                title_row = title_row.push(toggle);
            }
            let header_column = Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(4.0)
                .push(
                    Text::new(help_text.clone())
                        .with_size(12.0)
                        .with_color(muted),
                )
                .push(title_row);
            let header: Box<dyn RenderBox> = match orientation {
                Orientation::Portrait => Box::new(
                    Container::new()
                        .with_height(HEADER_PORTRAIT_HEIGHT)
                        .with_padding(EdgeInsets::only(24.0, 16.0, 12.0, 12.0))
                        .with_alignment(Alignment::BOTTOM_LEFT)
                        .with_child(header_column),
                ),
                Orientation::Landscape => Box::new(
                    Container::new()
                        .with_width(HEADER_LANDSCAPE_WIDTH)
                        .with_padding(EdgeInsets::all(16.0))
                        .with_alignment(Alignment::BOTTOM_LEFT)
                        .with_child(header_column),
                ),
            };

            let actions = Container::new()
                .with_height(52.0)
                .with_padding(EdgeInsets::symmetric(8.0, 0.0))
                .with_alignment(Alignment::CENTER_RIGHT)
                .with_child(
                    Row::new()
                        .with_main_axis_alignment(MainAxisAlignment::End)
                        .with_spacing(8.0)
                        .push(cancel)
                        .push(ok),
                );

            // The M3 layout's `Divider(height: 0)`.
            let divider = Container::new().with_height(1.0).with_color(outline);
            let error_line: Option<RenderRef> = input_error.as_ref().map(|message| {
                RenderRef::new(
                    Container::new()
                        .with_padding(EdgeInsets::symmetric(24.0, 0.0))
                        .with_child(
                            Text::new(message.clone())
                                .with_size(body_size - 2.0)
                                .with_color(error_color),
                        ),
                )
            });

            // The picker's own area: the calendar expands; the input form is
            // its fixed `_inputFormPortraitHeight`/`_inputFormLandscapeHeight`.
            let mut picker_area =
                Column::new().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            picker_area = if is_calendar {
                picker_area.push_flex(FlexChild::expanded(content, 1))
            } else {
                picker_area.push(
                    Container::new()
                        .with_height(match orientation {
                            Orientation::Portrait => INPUT_FORM_PORTRAIT_HEIGHT,
                            Orientation::Landscape => INPUT_FORM_LANDSCAPE_HEIGHT,
                        })
                        .with_padding(EdgeInsets::symmetric(24.0, 0.0))
                        .with_alignment(Alignment::CENTER)
                        .with_child(content),
                )
            };
            if let Some(error_line) = error_line {
                picker_area = picker_area.push(error_line);
            }

            let dialog_body: Box<dyn RenderBox> = match orientation {
                Orientation::Portrait => Box::new(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(header)
                        .push(divider)
                        .push_flex(FlexChild::expanded(picker_area, 1))
                        .push(actions),
                ),
                Orientation::Landscape => Box::new(
                    Row::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(header)
                        .push(Container::new().with_width(1.0).with_color(outline))
                        .push_flex(FlexChild::expanded(picker_area, 1)),
                ),
            };

            Box::new(
                Container::new()
                    .with_size(width, height)
                    .with_color(surface)
                    // Material 3's dialog shape, a 28-radius corner.
                    .with_corner_radius(28.0)
                    .with_elevation(6)
                    .with_child(dialog_body),
            )
        })
    }
}

// ---------------------------------------------------------------------------
// showDatePicker (anchor: date_picker.dart showDatePicker).
// ---------------------------------------------------------------------------

/// The dialog over a scrim, ready to stack over the page.
///
/// Anchor: `showDatePicker`, which routes the dialog through `showDialog`
/// onto the `Navigator`. There is no `Navigator` here, so the overlay is
/// returned instead: the caller stacks it over the page and drops it from
/// the tree when `on_confirm`/`on_cancel` fires -- those callbacks are what
/// upstream's returned `Future` resolves with. Tapping the scrim is the
/// barrier dismiss, and lands on `on_cancel`.
pub fn show_date_picker(dialog: DatePickerDialog) -> AnyWidget {
    let on_cancel = dialog.on_cancel.clone();
    let scrim_id = dialog.id * 10;
    let scrim: AnyWidget = leaf(move || {
        // The same color and tap-swallowing as `controls::Scrim`.
        let on_cancel = on_cancel.clone();
        Pointer::new(
            scrim_id,
            Container::new().with_color(Color::argb(0x8A, 0, 0, 0)),
        )
        .with_handlers(PointerHandlers::new().with_tap(move |_| {
            if let Some(cancel) = &on_cancel {
                cancel();
            }
        }))
    });
    many(vec![scrim, stateful(dialog)], |mut rendered| {
        let scrim = rendered.remove(0);
        let dialog = rendered.remove(0);
        RenderStack::new()
            .push_boxed(scrim)
            .push_positioned(Center::new(dialog), Positioned::fill())
    })
}

// ---------------------------------------------------------------------------
// TimePickerDialog (anchor: time_picker.dart TimePickerDialog,
// _TimePickerDialogState, _Dial, _DialPainter and _TimePickerInput).
// ---------------------------------------------------------------------------

/// `_kTimePickerDialPadding`.
const DIAL_PADDING: f32 = 28.0;
/// `_kTimePickerInnerDialOffset`: how far the 24-hour inner ring sits inside
/// the outer one.
const INNER_DIAL_OFFSET: f32 = 28.0;
/// `_kTimePickerDialMinRadius`.
const DIAL_MIN_RADIUS: f32 = 50.0;
/// `_TimePickerDefaultsM3.dotRadius`.
const DIAL_DOT_RADIUS: f32 = 24.0;
/// `_TimePickerDefaultsM3.centerRadius`.
const DIAL_CENTER_RADIUS: f32 = 4.0;
/// `_TimePickerDefaultsM3.handWidth`.
const DIAL_HAND_WIDTH: f32 = 2.0;
/// `_kTwoPi`.
const TWO_PI: f32 = std::f32::consts::TAU;

// Dialog sizes (anchor: the consts on `_TimePickerDialogState`).
const TIME_PICKER_PORTRAIT_SIZE: (f32, f32) = (310.0, 468.0);
const TIME_PICKER_LANDSCAPE_SIZE: (f32, f32) = (524.0, 342.0);
const TIME_PICKER_INPUT_SIZE: (f32, f32) = (312.0, 252.0);
/// `_kTimePickerHeaderLandscapeWidth`.
const TIME_PICKER_HEADER_LANDSCAPE_WIDTH: f32 = 216.0;
/// The M3 hour/minute header boxes: `_TimePickerDefaultsM3`'s 96x80 hour
/// minute size, 114 across for the 24-hour format.
const HOUR_MINUTE_SIZE: (f32, f32) = (96.0, 80.0);
const HOUR_MINUTE_SIZE_24H: (f32, f32) = (114.0, 80.0);
/// `_TimePickerDefaultsM3.dayPeriod...`: the AM/PM column, 52x80 with a
/// divider halfway.
const DAY_PERIOD_SIZE: (f32, f32) = (52.0, 80.0);
/// M3's `displayLarge`, the size the header boxes show the time at.
const TIME_HEADER_TEXT_SIZE: f32 = 57.0;

/// Dart's `%` on doubles: the result takes the divisor's sign, where Rust's
/// `%` takes the dividend's. The dial math is ported from Dart and depends on
/// the difference.
fn dart_mod(a: f32, b: f32) -> f32 {
    let r = a % b;
    if r != 0.0 && ((r < 0.0) != (b < 0.0)) {
        r + b
    } else {
        r
    }
}

/// Which field of the time the dial is editing.
///
/// Anchor: `_HourMinuteMode` in time_picker.dart.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum HourMinuteMode {
    #[default]
    Hour,
    Minute,
}

/// `_DialState._getRadiusForTime`: 1 on the outer ring, 0 on the inner one.
/// Only the 24-hour dial has an inner ring.
fn radius_for_time(time: TimeOfDay, mode: HourMinuteMode, use_24h: bool) -> f32 {
    match mode {
        HourMinuteMode::Hour if use_24h => {
            if time.hour >= TimeOfDay::HOURS_PER_PERIOD {
                0.0
            } else {
                1.0
            }
        }
        _ => 1.0,
    }
}

/// `_DialState._getThetaForTime`, M3 only: the 24-hour dial is the double
/// ring, whose hours factor is `hoursPerPeriod` -- hour 15 points at the "3"
/// position and the inner ring carries it, so the dial type plays no part
/// here.
fn theta_for_time(time: TimeOfDay, mode: HourMinuteMode) -> f32 {
    let fraction = match mode {
        HourMinuteMode::Hour => {
            let hours_factor = TimeOfDay::HOURS_PER_PERIOD;
            dart_mod(time.hour as f32 / hours_factor as f32, hours_factor as f32)
        }
        HourMinuteMode::Minute => dart_mod(
            time.minute as f32 / TimeOfDay::MINUTES_PER_HOUR as f32,
            TimeOfDay::MINUTES_PER_HOUR as f32,
        ),
    };
    dart_mod(std::f32::consts::FRAC_PI_2 - fraction * TWO_PI, TWO_PI)
}

/// `_DialState._getTimeForTheta`.
fn time_for_theta(
    theta: f32,
    round_minutes: bool,
    radius: f32,
    mode: HourMinuteMode,
    use_24h: bool,
    selected: TimeOfDay,
) -> TimeOfDay {
    let fraction = dart_mod(0.25 - dart_mod(theta, TWO_PI) / TWO_PI, 1.0);
    match mode {
        HourMinuteMode::Hour => {
            let mut new_hour = (fraction * TimeOfDay::HOURS_PER_PERIOD as f32).round() as u32
                % TimeOfDay::HOURS_PER_PERIOD;
            if use_24h {
                // The double ring: the inner ring is the afternoon.
                if radius < 0.5 {
                    new_hour += TimeOfDay::HOURS_PER_PERIOD;
                }
            } else {
                new_hour = (new_hour + selected.period_offset()) % TimeOfDay::HOURS_PER_DAY;
            }
            selected.replacing(Some(new_hour), None)
        }
        HourMinuteMode::Minute => {
            let mut minute = (fraction * TimeOfDay::MINUTES_PER_HOUR as f32).round() as u32
                % TimeOfDay::MINUTES_PER_HOUR;
            if round_minutes {
                // Round to the nearest 5-minute interval.
                minute = (minute + 2) / 5 * 5 % TimeOfDay::MINUTES_PER_HOUR;
            }
            selected.replacing(None, Some(minute))
        }
    }
}

/// `_DialState._updateThetaForPan`: the (theta, radius) a pointer position
/// maps to, then the time that pair selects. `extent` is the dial's side.
fn time_for_pointer(
    position: Offset,
    extent: f32,
    round_minutes: bool,
    mode: HourMinuteMode,
    use_24h: bool,
    selected: TimeOfDay,
) -> TimeOfDay {
    let dx = position.dx - extent / 2.0;
    let dy = position.dy - extent / 2.0;
    let label_radius = extent / 2.0 - DIAL_PADDING;
    let inner_radius = label_radius - INNER_DIAL_OFFSET;
    // Upstream's `atan2(offset.dx, offset.dy)`: the arguments really are in
    // that (transposed) order -- it is what puts zero at twelve o'clock after
    // the -pi/2 below, together with Dart's sign-of-divisor `%`.
    let mut angle = dart_mod(dx.atan2(dy) - std::f32::consts::FRAC_PI_2, TWO_PI);
    let radius = (((dx * dx + dy * dy).sqrt() - inner_radius) / INNER_DIAL_OFFSET).clamp(0.0, 1.0);
    let mut time = time_for_theta(angle, round_minutes, radius, mode, use_24h, selected);
    if round_minutes {
        angle = theta_for_time(time, mode);
        time = time_for_theta(angle, true, radius, mode, use_24h, selected);
    }
    time
}

/// `_TimePickerInputState._parseHour`.
fn parse_hour_text(text: &str, use_24h: bool, period: DayPeriod) -> Option<u32> {
    let mut hour: u32 = text.trim().parse().ok()?;
    if use_24h {
        if hour < TimeOfDay::HOURS_PER_DAY {
            return Some(hour);
        }
    } else if hour > 0 && hour <= TimeOfDay::HOURS_PER_PERIOD {
        if (period == DayPeriod::Pm && hour != 12) || (period == DayPeriod::Am && hour == 12) {
            hour = (hour + TimeOfDay::HOURS_PER_PERIOD) % TimeOfDay::HOURS_PER_DAY;
        }
        return Some(hour);
    }
    None
}

/// `_TimePickerInputState._parseMinute`.
fn parse_minute_text(text: &str) -> Option<u32> {
    let minute: u32 = text.trim().parse().ok()?;
    if minute < TimeOfDay::MINUTES_PER_HOUR {
        Some(minute)
    } else {
        None
    }
}

/// One dial label: its text and whether it sits on the inner ring.
#[derive(Clone)]
struct DialLabel {
    text: String,
    inner: bool,
}

/// The ring of labels for the current mode. Anchors: `_build12HourRing`,
/// `_build24HourRing` (the M3 branch) and `_buildMinutes`.
fn dial_labels(mode: HourMinuteMode, use_24h: bool) -> Vec<DialLabel> {
    match mode {
        HourMinuteMode::Hour if use_24h => (0..TimeOfDay::HOURS_PER_DAY)
            .map(|hour| DialLabel {
                // The M3 spec draws 0 as "00" and 1-9 single-digit.
                text: if hour == 0 {
                    "00".to_string()
                } else {
                    hour.to_string()
                },
                inner: hour >= TimeOfDay::HOURS_PER_PERIOD,
            })
            .collect(),
        HourMinuteMode::Hour => (0..TimeOfDay::HOURS_PER_PERIOD)
            .map(|i| DialLabel {
                text: (if i == 0 { 12 } else { i }).to_string(),
                inner: false,
            })
            .collect(),
        HourMinuteMode::Minute => (0..TimeOfDay::MINUTES_PER_HOUR)
            .step_by(5)
            .map(|minute| DialLabel {
                text: format!("{minute:02}"),
                inner: false,
            })
            .collect(),
    }
}

/// The clock face.
///
/// Anchor: `_Dial`'s `CustomPaint` with `_DialPainter` (time_picker.dart),
/// as a leaf render box. The gestures live on the `Pointer` the dialog wraps
/// around it; what they need of the geometry -- the side the dial was laid
/// out at -- is published through `extent`.
struct TimeDial {
    labels: Vec<DialLabel>,
    theta: f32,
    radius: f32,
    extent: Rc<Cell<f32>>,
    background: Color,
    hand: Color,
    dot_text: Color,
    label_color: Color,
    label_size: f32,
    size: Size,
}

impl TimeDial {
    /// `_DialPainter.getOffsetForTheta`.
    fn point_for_theta(center: (f32, f32), theta: f32, radius: f32) -> (f32, f32) {
        (
            center.0 + radius * theta.cos(),
            center.1 - radius * theta.sin(),
        )
    }

    fn paint_labels(
        &self,
        canvas: &mut crate::engine::Canvas,
        center: (f32, f32),
        inner: bool,
        radius: f32,
        color: Color,
        only_index: Option<usize>,
    ) {
        let ring: Vec<(usize, &DialLabel)> = self
            .labels
            .iter()
            .enumerate()
            .filter(|(_, label)| label.inner == inner)
            .collect();
        if ring.is_empty() {
            return;
        }
        // `_DialPainter.paintLabels`: the first label at twelve o'clock, the
        // rest a `-_kTwoPi / len` step apart.
        let increment = -TWO_PI / ring.len() as f32;
        let mut theta = std::f32::consts::FRAC_PI_2;
        let direction = current_direction();
        for (index, label) in ring {
            if only_index.is_some_and(|only| only != index) {
                theta += increment;
                continue;
            }
            let style = TextStyle {
                font_size: self.label_size,
                color,
                ..Default::default()
            };
            let paragraph = Paragraph::new(
                &label.text,
                &style,
                Some(1),
                false,
                f32::MAX / 4.0,
                direction,
            );
            let (x, y) = Self::point_for_theta(center, theta, radius);
            canvas.draw_paragraph(
                &paragraph,
                x - paragraph.width() / 2.0,
                y - paragraph.height() / 2.0,
            );
            theta += increment;
        }
    }
}

impl RenderBox for TimeDial {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let biggest = constraints.biggest();
        // `_DialPainter`'s clamp: the dial never goes under the minimum
        // radius plus the focused dot.
        let side = biggest
            .width
            .min(biggest.height)
            .max(2.0 * (DIAL_MIN_RADIUS + DIAL_DOT_RADIUS));
        self.size = constraints.constrain(Size::new(side, side));
        self.extent.set(self.size.width.min(self.size.height));
        self.size
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        let biggest = constraints.biggest();
        let side = biggest
            .width
            .min(biggest.height)
            .max(2.0 * (DIAL_MIN_RADIUS + DIAL_DOT_RADIUS));
        constraints.constrain(Size::new(side, side))
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let canvas = context.canvas();
        let center = (
            offset.dx + self.size.width / 2.0,
            offset.dy + self.size.height / 2.0,
        );
        // `_DialPainter.paint`, constant by constant.
        let dial_radius =
            (self.size.width.min(self.size.height) / 2.0).max(DIAL_MIN_RADIUS + DIAL_DOT_RADIUS);
        let label_radius = (dial_radius - DIAL_PADDING).max(DIAL_MIN_RADIUS);
        let inner_label_radius = (label_radius - INNER_DIAL_OFFSET).max(0.0);
        let handle_radius = (label_radius
            - if self.radius < 0.5 { 1.0 } else { 0.0 } * (label_radius - inner_label_radius))
            .max(DIAL_MIN_RADIUS);

        canvas.draw_circle(
            center.0,
            center.1,
            dial_radius,
            &Paint::new(self.background),
        );
        self.paint_labels(canvas, center, false, label_radius, self.label_color, None);
        self.paint_labels(
            canvas,
            center,
            true,
            inner_label_radius,
            self.label_color,
            None,
        );

        let focused = Self::point_for_theta(center, self.theta, handle_radius);
        canvas.draw_circle(
            center.0,
            center.1,
            DIAL_CENTER_RADIUS,
            &Paint::new(self.hand),
        );
        canvas.draw_circle(
            focused.0,
            focused.1,
            DIAL_DOT_RADIUS,
            &Paint::new(self.hand),
        );
        canvas.draw_line(
            center,
            focused,
            &Paint::new(self.hand).with_style(Style::Stroke {
                width: DIAL_HAND_WIDTH,
            }),
        );

        // Upstream adds a dot inside the selector when its theta falls
        // between two labels, testing `theta % labelThetaIncrement` against
        // 0.1..0.45. The increment is negative, and Dart's `%` takes the
        // divisor's sign, so the remainder is never positive and the branch
        // is dead upstream too; `dart_mod` reproduces exactly that.
        let increment = -TWO_PI / self.labels.len() as f32;
        let remainder = dart_mod(self.theta, increment);
        if remainder > 0.1 && remainder < 0.45 {
            canvas.draw_circle(focused.0, focused.1, 2.0, &Paint::new(self.dot_text));
        }

        // Upstream repaints every label in the selected color clipped to the
        // focused dot; only the label under the dot survives the clip, and
        // the dot sits exactly on that label's ring position, so repainting
        // just that one label is the same picture. The label under theta is
        // the first of its ring -- theta is derived from the selected time,
        // which always starts a ring's sequence.
        let selected_ring_inner = self.radius < 0.5 && self.labels.iter().any(|l| l.inner);
        let ring_len = self
            .labels
            .iter()
            .filter(|l| l.inner == selected_ring_inner)
            .count();
        if ring_len > 0 {
            // The index of the selected label within its ring: the angle's
            // distance from twelve o'clock in increments.
            let from_top = dart_mod(std::f32::consts::FRAC_PI_2 - self.theta, TWO_PI);
            let index = (from_top / TWO_PI * ring_len as f32).round() as usize % ring_len;
            let ring_radius = if selected_ring_inner {
                inner_label_radius
            } else {
                label_radius
            };
            self.paint_labels(
                canvas,
                center,
                selected_ring_inner,
                ring_radius,
                self.dot_text,
                Some(index),
            );
        }
    }
}

/// A Material-style time picker dialog.
///
/// Anchor: `TimePickerDialog` in time_picker.dart. The dial is M3's: the
/// 24-hour format gets the double ring.
///
/// As with [`DatePickerDialog`] there is no `Navigator`: `with_on_confirm`
/// hears the picked time on OK and `with_on_cancel` a dismissal. Upstream
/// reads the 24-hour preference from `MediaQuery.alwaysUse24HourFormatOf`,
/// which `MediaQueryData` does not carry, so it is an explicit setting here:
/// [`TimePickerDialog::with_always_use_24_hour_format`], defaulting to the
/// compiled-in en_US locale's 12-hour clock.
///
/// Hit-test ids derive from `id`: the dial is `id * 100 + 1`, the hour and
/// minute header boxes `+ 2` and `+ 3`, AM/PM `+ 4` and `+ 5`, the input
/// fields `+ 6` and `+ 7`, the mode toggle `+ 8`, OK `+ 9`, Cancel `+ 10`.
pub struct TimePickerDialog {
    id: u64,
    initial_time: TimeOfDay,
    cancel_text: Option<String>,
    confirm_text: Option<String>,
    help_text: Option<String>,
    error_invalid_text: Option<String>,
    hour_label_text: Option<String>,
    minute_label_text: Option<String>,
    initial_entry_mode: TimePickerEntryMode,
    orientation: Option<Orientation>,
    on_entry_mode_changed: Option<Rc<dyn Fn(TimePickerEntryMode)>>,
    always_use_24_hour_format: Option<bool>,
    on_confirm: Option<Rc<dyn Fn(TimeOfDay)>>,
    on_cancel: Option<Rc<dyn Fn()>>,
}

/// What a [`TimePickerDialog`] remembers between frames.
///
/// Anchor: `_TimePickerDialogState`.
#[derive(Default)]
pub struct TimePickerDialogState {
    /// Upstream's `_entryMode`.
    pub entry_mode: TimePickerEntryMode,
    /// Upstream's `_selectedTime`.
    pub selected_time: TimeOfDay,
    /// Upstream's `_mode` (the `_HourMinuteMode` the dial edits).
    hour_minute_mode: HourMinuteMode,
    /// The input fields' texts, mirrored as they change (see
    /// [`InputDatePickerFormField::with_on_text_changed`] for why this mirror
    /// exists).
    pub hour_text: String,
    pub minute_text: String,
    /// Whether the input is showing its validation error: upstream flipping
    /// `_autovalidateMode` to `always` after a failed OK.
    pub input_error: bool,
    /// The side the dial was last laid out at, for the gesture math.
    dial_extent: Rc<Cell<f32>>,
}

impl TimePickerDialog {
    pub fn new(id: u64, initial_time: TimeOfDay) -> TimePickerDialog {
        TimePickerDialog {
            id,
            initial_time,
            cancel_text: None,
            confirm_text: None,
            help_text: None,
            error_invalid_text: None,
            hour_label_text: None,
            minute_label_text: None,
            initial_entry_mode: TimePickerEntryMode::Dial,
            orientation: None,
            on_entry_mode_changed: None,
            always_use_24_hour_format: None,
            on_confirm: None,
            on_cancel: None,
        }
    }

    pub fn with_cancel_text(mut self, text: impl Into<String>) -> Self {
        self.cancel_text = Some(text.into());
        self
    }
    pub fn with_confirm_text(mut self, text: impl Into<String>) -> Self {
        self.confirm_text = Some(text.into());
        self
    }
    pub fn with_help_text(mut self, text: impl Into<String>) -> Self {
        self.help_text = Some(text.into());
        self
    }
    pub fn with_error_invalid_text(mut self, text: impl Into<String>) -> Self {
        self.error_invalid_text = Some(text.into());
        self
    }
    pub fn with_hour_label_text(mut self, text: impl Into<String>) -> Self {
        self.hour_label_text = Some(text.into());
        self
    }
    pub fn with_minute_label_text(mut self, text: impl Into<String>) -> Self {
        self.minute_label_text = Some(text.into());
        self
    }
    pub fn with_initial_entry_mode(mut self, mode: TimePickerEntryMode) -> Self {
        self.initial_entry_mode = mode;
        self
    }
    /// Overrides the orientation read from the ambient `MediaQuery`.
    /// Upstream's `orientation` parameter.
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = Some(orientation);
        self
    }
    /// Upstream's `onEntryModeChanged`.
    pub fn with_on_entry_mode_changed(
        mut self,
        changed: impl Fn(TimePickerEntryMode) + 'static,
    ) -> Self {
        self.on_entry_mode_changed = Some(Rc::new(changed));
        self
    }
    /// See the type docs for why this is a setting and not a MediaQuery read.
    pub fn with_always_use_24_hour_format(mut self, always: bool) -> Self {
        self.always_use_24_hour_format = Some(always);
        self
    }
    /// Called with the picked time when OK is pressed. Replaces the value
    /// upstream's `Navigator.pop` delivers.
    pub fn with_on_confirm(mut self, confirm: impl Fn(TimeOfDay) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(confirm));
        self
    }
    /// Called when the dialog is dismissed without a time.
    pub fn with_on_cancel(mut self, cancel: impl Fn() + 'static) -> Self {
        self.on_cancel = Some(Rc::new(cancel));
        self
    }
}

/// The AM/PM switch, M3's `_DayPeriodControl`: a 52x80 outlined column of
/// two halves.
fn day_period_control(
    am_id: u64,
    pm_id: u64,
    selected: TimeOfDay,
    handle: StateHandle<TimePickerDialogState>,
    body_size: f32,
    text: Color,
    primary: Color,
    outline: Color,
) -> Container {
    let half = |id: u64, label: &'static str, period: DayPeriod| {
        let color = if selected.period() == period {
            primary
        } else {
            text
        };
        let half_handle = handle.clone();
        Pointer::new(
            id,
            Container::new()
                .with_alignment(Alignment::CENTER)
                .with_child(
                    Text::new(label)
                        .with_size(body_size + 2.0)
                        .with_weight(500)
                        .with_color(color),
                ),
        )
        .with_handlers(PointerHandlers::new().with_tap(move |_| {
            half_handle.set_state(move |state| {
                // `_DayPeriodControl._togglePeriod`.
                let hour = state.selected_time.hour;
                let new_hour = match (state.selected_time.period(), period) {
                    (DayPeriod::Am, DayPeriod::Pm) => hour + TimeOfDay::HOURS_PER_PERIOD,
                    (DayPeriod::Pm, DayPeriod::Am) => hour - TimeOfDay::HOURS_PER_PERIOD,
                    _ => hour,
                };
                state.selected_time = state.selected_time.replacing(Some(new_hour), None);
            });
        }))
    };
    Container::new()
        .with_size(DAY_PERIOD_SIZE.0, DAY_PERIOD_SIZE.1)
        .with_corner_radius(8.0)
        .with_border(1.0, outline)
        .with_child(
            Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push_flex(FlexChild::expanded(half(am_id, "AM", DayPeriod::Am), 1))
                .push(Container::new().with_height(1.0).with_color(outline))
                .push_flex(FlexChild::expanded(half(pm_id, "PM", DayPeriod::Pm), 1)),
        )
}

/// One of the big header boxes, `_HourControl`/`_MinuteControl`: tappable to
/// give the dial its mode.
fn time_header_box(
    id: u64,
    label: String,
    active: bool,
    width: f32,
    handle: StateHandle<TimePickerDialogState>,
    mode: HourMinuteMode,
    text: Color,
    primary: Color,
    surface_variant: Color,
) -> crate::render::RenderPointerRegion {
    let (fill, color) = if active {
        // The M3 selected container is primaryContainer; the crate's theme
        // has no such token, so it is the primary at the emphasis alpha the
        // spec gives it over a surface.
        (primary.with_alpha(0x3D), primary)
    } else {
        (surface_variant, text)
    };
    Pointer::new(
        id,
        Container::new()
            .with_size(width, HOUR_MINUTE_SIZE.1)
            .with_corner_radius(8.0)
            .with_color(fill)
            .with_alignment(Alignment::CENTER)
            .with_child(
                Text::new(label)
                    .with_size(TIME_HEADER_TEXT_SIZE)
                    .with_color(color),
            ),
    )
    .with_handlers(PointerHandlers::new().with_tap(move |_| {
        // `_HourMinuteControl.onTap` through `_TimePickerModel.setHourMinuteMode`.
        handle.set_state(move |state| state.hour_minute_mode = mode);
    }))
}

impl StatefulComponent for TimePickerDialog {
    type State = TimePickerDialogState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    /// `_TimePickerDialogState`'s field initializers.
    fn initial_state(&self) -> TimePickerDialogState {
        TimePickerDialogState {
            entry_mode: self.initial_entry_mode,
            selected_time: self.initial_time,
            hour_minute_mode: HourMinuteMode::Hour,
            hour_text: String::new(),
            minute_text: String::new(),
            input_error: false,
            dial_extent: Rc::new(Cell::new(0.0)),
        }
    }

    fn build(
        &self,
        state: &TimePickerDialogState,
        handle: StateHandle<TimePickerDialogState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let orientation = self.orientation.unwrap_or_else(|| orientation_of(context));
        let use_24h = self.always_use_24_hour_format.unwrap_or(false);
        let entry_mode = state.entry_mode;
        let is_dial = matches!(
            entry_mode,
            TimePickerEntryMode::Dial | TimePickerEntryMode::DialOnly
        );
        let (width, height) = match (is_dial, orientation) {
            (true, Orientation::Portrait) => TIME_PICKER_PORTRAIT_SIZE,
            (true, Orientation::Landscape) => TIME_PICKER_LANDSCAPE_SIZE,
            (false, _) => TIME_PICKER_INPUT_SIZE,
        };
        let id = self.id * 100;
        let body_size = theme.body_size;
        let (text, muted, primary, on_primary, surface, surface_variant, outline, danger) = (
            theme.text,
            theme.text_muted,
            theme.primary,
            theme.on_primary,
            theme.surface,
            theme.surface_variant,
            theme.outline,
            theme.danger,
        );
        let mode = state.hour_minute_mode;
        let selected = state.selected_time;

        // -- The hour and minute widgets: the big tappable boxes of
        // `_HourControl`/`_MinuteControl` in dial mode, labelled fields in
        // input mode (anchor: `_TimePickerInput`). The fields start empty --
        // upstream pre-fills its controllers from the selected time, which a
        // controller-less field cannot do; an empty field at OK time stands
        // for the selected time's component, as the docs on the OK path say.
        let box_width = if use_24h {
            HOUR_MINUTE_SIZE_24H.0
        } else {
            HOUR_MINUTE_SIZE.0
        };
        let hour_widget: AnyWidget = if is_dial {
            let box_handle = handle.clone();
            let hour_label = if use_24h {
                format!("{:02}", selected.hour)
            } else {
                format!("{}", selected.hour_of_period())
            };
            leaf(move || {
                time_header_box(
                    id + 2,
                    hour_label.clone(),
                    mode == HourMinuteMode::Hour,
                    box_width,
                    box_handle.clone(),
                    HourMinuteMode::Hour,
                    text,
                    primary,
                    surface_variant,
                )
            })
        } else {
            let field_handle = handle.clone();
            let label = self
                .hour_label_text
                .clone()
                .unwrap_or(TIME_PICKER_HOUR_LABEL.into());
            many(
                vec![stateful(TextField::new(id + 6).with_on_changed(
                    move |text| {
                        let text = text.to_string();
                        field_handle.set_state(move |state| state.hour_text = text);
                    },
                ))],
                move |mut rendered| {
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(4.0)
                        .push(
                            Text::new(label.clone())
                                .with_size(body_size)
                                .with_color(text),
                        )
                        .push(
                            Container::new()
                                .with_width(HOUR_MINUTE_SIZE.0)
                                .with_child(rendered.remove(0)),
                        )
                },
            )
        };
        let minute_widget: AnyWidget = if is_dial {
            let box_handle = handle.clone();
            let minute_label = format!("{:02}", selected.minute);
            leaf(move || {
                time_header_box(
                    id + 3,
                    minute_label.clone(),
                    mode == HourMinuteMode::Minute,
                    box_width,
                    box_handle.clone(),
                    HourMinuteMode::Minute,
                    text,
                    primary,
                    surface_variant,
                )
            })
        } else {
            let field_handle = handle.clone();
            let label = self
                .minute_label_text
                .clone()
                .unwrap_or(TIME_PICKER_MINUTE_LABEL.into());
            many(
                vec![stateful(TextField::new(id + 7).with_on_changed(
                    move |text| {
                        let text = text.to_string();
                        field_handle.set_state(move |state| state.minute_text = text);
                    },
                ))],
                move |mut rendered| {
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(4.0)
                        .push(
                            Text::new(label.clone())
                                .with_size(body_size)
                                .with_color(text),
                        )
                        .push(
                            Container::new()
                                .with_width(HOUR_MINUTE_SIZE.0)
                                .with_child(rendered.remove(0)),
                        )
                },
            )
        };

        // -- The AM/PM switch, 12-hour clocks only.
        let day_period: AnyWidget = if use_24h {
            leaf(|| Empty)
        } else {
            let period_handle = handle.clone();
            leaf(move || {
                day_period_control(
                    id + 4,
                    id + 5,
                    selected,
                    period_handle.clone(),
                    body_size,
                    text,
                    primary,
                    outline,
                )
            })
        };

        // -- The dial (input mode has no content below its header row, same
        // as upstream's `_TimePickerInput`).
        let content: AnyWidget = if is_dial {
            let theta = theta_for_time(selected, mode);
            let radius = radius_for_time(selected, mode, use_24h);
            let labels = dial_labels(mode, use_24h);
            let tap_handle = handle.clone();
            let drag_start_handle = handle.clone();
            let drag_update_handle = handle.clone();
            let drag_end_handle = handle.clone();
            let tap_extent = state.dial_extent.clone();
            let drag_start_extent = state.dial_extent.clone();
            let drag_update_extent = state.dial_extent.clone();
            // `_Dial`'s GestureDetector: taps snap to the nearest mark
            // (`_handleTapUp` rounds minutes to 5), pans track exactly
            // (`_handlePanStart`/`_handlePanUpdate`), and picking an hour
            // hands the dial to the minutes (`onHourSelected`).
            let handlers = PointerHandlers::new()
                .with_tap(move |event| {
                    let extent = tap_extent.get();
                    if extent <= 0.0 {
                        return;
                    }
                    tap_handle.set_state(move |state| {
                        state.selected_time = time_for_pointer(
                            event.local_position,
                            extent,
                            true,
                            state.hour_minute_mode,
                            use_24h,
                            state.selected_time,
                        );
                        if state.hour_minute_mode == HourMinuteMode::Hour {
                            state.hour_minute_mode = HourMinuteMode::Minute;
                        }
                    });
                })
                .with_drag_start(move |event| {
                    let extent = drag_start_extent.get();
                    if extent <= 0.0 {
                        return;
                    }
                    drag_start_handle.set_state(move |state| {
                        state.selected_time = time_for_pointer(
                            event.local_position,
                            extent,
                            false,
                            state.hour_minute_mode,
                            use_24h,
                            state.selected_time,
                        );
                    });
                })
                .with_drag_update(move |event| {
                    let extent = drag_update_extent.get();
                    if extent <= 0.0 {
                        return;
                    }
                    drag_update_handle.set_state(move |state| {
                        state.selected_time = time_for_pointer(
                            event.local_position,
                            extent,
                            false,
                            state.hour_minute_mode,
                            use_24h,
                            state.selected_time,
                        );
                    });
                })
                .with_drag_end(move |_| {
                    drag_end_handle.set_state(move |state| {
                        if state.hour_minute_mode == HourMinuteMode::Hour {
                            state.hour_minute_mode = HourMinuteMode::Minute;
                        }
                    });
                });
            let extent_sink = state.dial_extent.clone();
            leaf(move || {
                Pointer::new(
                    id + 1,
                    TimeDial {
                        labels: labels.clone(),
                        theta,
                        radius,
                        extent: extent_sink.clone(),
                        background: surface_variant,
                        hand: primary,
                        dot_text: on_primary,
                        label_color: text,
                        label_size: body_size + 2.0,
                        size: Size::ZERO,
                    },
                )
                .with_handlers(handlers.clone())
            })
        } else {
            leaf(|| Empty)
        };

        // -- The entry-mode toggle, `dial` and `input` only. Upstream's
        // icons (`keyboard`/`schedule`) are text here, the Material icons
        // font not being loaded.
        let can_toggle = matches!(
            entry_mode,
            TimePickerEntryMode::Dial | TimePickerEntryMode::Input
        );
        let toggle: AnyWidget = if can_toggle {
            let toggle_handle = handle.clone();
            let on_mode_change = self.on_entry_mode_changed.clone();
            let label = if is_dial { "Edit" } else { "Dial" };
            leaf(move || {
                let tap_handle = toggle_handle.clone();
                let on_mode_change = on_mode_change.clone();
                Pointer::new(
                    id + 8,
                    Container::new()
                        .with_padding(EdgeInsets::symmetric(8.0, 4.0))
                        .with_child(
                            Text::new(label)
                                .with_size(13.0)
                                .with_weight(500)
                                .with_color(muted),
                        ),
                )
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    // `_handleEntryModeToggle`.
                    let on_mode_change = on_mode_change.clone();
                    tap_handle.set_state(move |state| {
                        state.entry_mode = match state.entry_mode {
                            TimePickerEntryMode::Dial => TimePickerEntryMode::Input,
                            TimePickerEntryMode::Input => TimePickerEntryMode::Dial,
                            mode => mode,
                        };
                        state.input_error = false;
                        if let Some(changed) = &on_mode_change {
                            changed(state.entry_mode);
                        }
                    });
                }))
            })
        } else {
            leaf(|| Empty)
        };

        // -- The actions: CANCEL and OK. Anchor: `_TimePickerDialogState._handleOk`.
        let ok: AnyWidget = {
            let ok_handle = handle.clone();
            let on_confirm = self.on_confirm.clone();
            component(
                Button::new(
                    id + 9,
                    self.confirm_text
                        .clone()
                        .unwrap_or_else(|| OK_BUTTON_LABEL.into()),
                )
                .with_style(ButtonVariant::Text)
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    let on_confirm = on_confirm.clone();
                    ok_handle.set_state(move |state| match state.entry_mode {
                        TimePickerEntryMode::Dial | TimePickerEntryMode::DialOnly => {
                            if let Some(confirm) = &on_confirm {
                                confirm(state.selected_time);
                            }
                        }
                        TimePickerEntryMode::Input | TimePickerEntryMode::InputOnly => {
                            // `_handleOk` in input mode: parse both fields.
                            // An untouched field stands for the selected
                            // time's component -- the pre-filled controller's
                            // stand-in, as in the date dialog.
                            let hour = if state.hour_text.trim().is_empty() {
                                Some(state.selected_time.hour)
                            } else {
                                parse_hour_text(
                                    &state.hour_text,
                                    use_24h,
                                    state.selected_time.period(),
                                )
                            };
                            let minute = if state.minute_text.trim().is_empty() {
                                Some(state.selected_time.minute)
                            } else {
                                parse_minute_text(&state.minute_text)
                            };
                            match (hour, minute) {
                                (Some(hour), Some(minute)) => {
                                    state.selected_time = TimeOfDay::new(hour, minute);
                                    state.input_error = false;
                                    if let Some(confirm) = &on_confirm {
                                        confirm(state.selected_time);
                                    }
                                }
                                // The failed validate flips autovalidate on.
                                _ => state.input_error = true,
                            }
                        }
                    });
                })),
            )
        };
        let cancel: AnyWidget = {
            let on_cancel = self.on_cancel.clone();
            component(
                Button::new(
                    id + 10,
                    self.cancel_text
                        .clone()
                        .unwrap_or_else(|| CANCEL_BUTTON_LABEL.into()),
                )
                .with_style(ButtonVariant::Text)
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    if let Some(cancel) = &on_cancel {
                        cancel();
                    }
                })),
            )
        };

        // -- Assembly, `_TimePickerDialogState.build`'s layout.
        let help_text = self.help_text.clone().unwrap_or_else(|| {
            if is_dial {
                TIME_PICKER_DIAL_HELP_TEXT.into()
            } else {
                TIME_PICKER_INPUT_HELP_TEXT.into()
            }
        });
        let error_invalid_text = self
            .error_invalid_text
            .clone()
            .unwrap_or(INVALID_TIME_LABEL.into());
        let input_error = state.input_error;

        many(
            vec![
                hour_widget,
                minute_widget,
                day_period,
                content,
                toggle,
                ok,
                cancel,
            ],
            move |mut rendered| {
                let hour_widget = rendered.remove(0);
                let minute_widget = rendered.remove(0);
                let day_period = rendered.remove(0);
                let content = rendered.remove(0);
                let toggle = rendered.remove(0);
                let ok = rendered.remove(0);
                let cancel = rendered.remove(0);

                let header_label = Text::new(help_text.clone())
                    .with_size(12.0)
                    .with_color(muted);

                let actions = Container::new()
                    .with_height(52.0)
                    .with_padding(EdgeInsets::symmetric(12.0, 0.0))
                    .with_child(
                        Row::new()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(8.0)
                            .push(toggle)
                            .push_flex(FlexChild::expanded(Container::new(), 1))
                            .push(cancel)
                            .push(ok),
                    );

                let dialog_body: Box<dyn RenderBox> =
                    if is_dial && orientation == Orientation::Landscape {
                        // Upstream's landscape header: the help text over the
                        // hour box over the minute box, in a 216-wide band.
                        let mut header_column = Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_spacing(12.0)
                            .push(header_label)
                            .push(hour_widget)
                            .push(minute_widget);
                        if !use_24h {
                            header_column = header_column.push(day_period);
                        }
                        Box::new(
                            Row::new()
                                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                                .push(
                                    Container::new()
                                        .with_width(TIME_PICKER_HEADER_LANDSCAPE_WIDTH)
                                        .with_padding(EdgeInsets::all(16.0))
                                        .with_child(header_column),
                                )
                                .push_flex(FlexChild::expanded(
                                    Column::new()
                                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                                        .push_flex(FlexChild::expanded(Center::new(content), 1))
                                        .push(actions),
                                    1,
                                )),
                        )
                    } else {
                        // The time display row: hour box, ":", minute box, then
                        // AM/PM on a 12-hour clock.
                        let separator_size = if is_dial { TIME_HEADER_TEXT_SIZE } else { 24.0 };
                        let mut time_row = Row::new()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(12.0)
                            .push(hour_widget)
                            .push(
                                Text::new(":")
                                    .with_size(separator_size)
                                    .with_weight(if is_dial { 400 } else { 700 })
                                    .with_color(text),
                            )
                            .push(minute_widget);
                        if !use_24h {
                            time_row = time_row.push(day_period);
                        }

                        if is_dial {
                            Box::new(
                                Column::new()
                                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                                    .push(
                                        Container::new()
                                            .with_padding(EdgeInsets::only(24.0, 16.0, 24.0, 0.0))
                                            .with_child(header_label),
                                    )
                                    .push(
                                        Container::new()
                                            .with_padding(EdgeInsets::only(24.0, 20.0, 24.0, 0.0))
                                            .with_child(time_row),
                                    )
                                    .push_flex(FlexChild::expanded(Center::new(content), 1))
                                    .push(actions),
                            )
                        } else {
                            let mut column = Column::new()
                                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                                .push(
                                    Container::new()
                                        .with_padding(EdgeInsets::only(24.0, 16.0, 24.0, 0.0))
                                        .with_child(header_label),
                                )
                                .push(
                                    Container::new()
                                        .with_padding(EdgeInsets::only(24.0, 16.0, 24.0, 0.0))
                                        .with_child(time_row),
                                );
                            if input_error {
                                column = column.push(
                                    Container::new()
                                        .with_padding(EdgeInsets::symmetric(24.0, 0.0))
                                        .with_child(
                                            Text::new(error_invalid_text.clone())
                                                .with_size(body_size - 2.0)
                                                .with_color(danger),
                                        ),
                                );
                            }
                            Box::new(
                                column
                                    .push_flex(FlexChild::expanded(Container::new(), 1))
                                    .push(actions),
                            )
                        }
                    };

                Box::new(
                    Container::new()
                        .with_size(width, height)
                        .with_color(surface)
                        // Material 3's dialog shape and elevation.
                        .with_corner_radius(28.0)
                        .with_elevation(6)
                        .with_child(dialog_body),
                )
            },
        )
    }
}

/// The time picker dialog over a scrim, ready to stack over the page.
///
/// Anchor: `showTimePicker`; the same returned-overlay stand-in for the
/// `Navigator` push as [`show_date_picker`]. The scrim's hit-test id is
/// `dialog.id * 100`, the dialog's own ids starting at `+ 1`.
pub fn show_time_picker(dialog: TimePickerDialog) -> AnyWidget {
    let on_cancel = dialog.on_cancel.clone();
    let scrim_id = dialog.id * 100;
    let scrim: AnyWidget = leaf(move || {
        let on_cancel = on_cancel.clone();
        Pointer::new(
            scrim_id,
            Container::new().with_color(Color::argb(0x8A, 0, 0, 0)),
        )
        .with_handlers(PointerHandlers::new().with_tap(move |_| {
            if let Some(cancel) = &on_cancel {
                cancel();
            }
        }))
    });
    many(vec![scrim, stateful(dialog)], |mut rendered| {
        let scrim = rendered.remove(0);
        let dialog = rendered.remove(0);
        RenderStack::new()
            .push_boxed(scrim)
            .push_positioned(Center::new(dialog), Positioned::fill())
    })
}

// ---------------------------------------------------------------------------
// DateRangePickerDialog (anchor: date_picker.dart DateRangePickerDialog,
// _DateRangePickerDialogState, _CalendarRangePickerDialog,
// _CalendarDateRangePicker, _MonthItem, _DayItem, _HighlightPainter and
// _InputDateRangePickerDialog).
// ---------------------------------------------------------------------------

/// `_monthItemHeaderHeight`.
const MONTH_ITEM_HEADER_HEIGHT: f32 = 58.0;
/// `_monthItemFooterHeight`.
const MONTH_ITEM_FOOTER_HEIGHT: f32 = 12.0;
/// `_monthItemRowHeight`.
const MONTH_ITEM_ROW_HEIGHT: f32 = 42.0;
/// `_monthItemSpaceBetweenRows`.
const MONTH_ITEM_SPACE_BETWEEN_ROWS: f32 = 8.0;
/// `_maxCalendarWidthPortrait`.
const RANGE_MAX_CALENDAR_WIDTH_PORTRAIT: f32 = 480.0;
/// `_maxCalendarWidthLandscape`.
const RANGE_MAX_CALENDAR_WIDTH_LANDSCAPE: f32 = 384.0;
/// `_inputRangeLandscapeDialogSize`; the portrait input dialog reuses the
/// single-date picker's `_inputPortraitDialogSizeM3`.
const RANGE_INPUT_LANDSCAPE_DIALOG_SIZE: (f32, f32) = (496.0, 164.0);
const RANGE_INPUT_PORTRAIT_DIALOG_SIZE: (f32, f32) = (328.0, 270.0);

/// One month item's total height: `_MonthItem.build`'s header, grid and
/// footer.
fn month_item_height(year: i32, month: u32) -> f32 {
    let day_offset = first_day_offset(year, month, FIRST_DAY_OF_WEEK_INDEX);
    let weeks = (days_in_month(year, month) + day_offset).div_ceil(7);
    MONTH_ITEM_HEADER_HEIGHT
        + weeks as f32 * MONTH_ITEM_ROW_HEIGHT
        + (weeks.saturating_sub(1)) as f32 * MONTH_ITEM_SPACE_BETWEEN_ROWS
        + MONTH_ITEM_FOOTER_HEIGHT
}

/// The selection update of `_CalendarDateRangePicker._updateSelection`:
/// picking with a start but no end and on/after the start sets the end;
/// anything else resets the range to that start.
fn range_update_selection(state: &mut DateRangePickerDialogState, date: Date) {
    if let Some(start) = state.selected_start {
        if state.selected_end.is_none() && date >= start {
            state.selected_end = Some(date);
            return;
        }
    }
    state.selected_start = Some(date);
    state.selected_end = None;
}

/// One day cell of the range grid.
///
/// Anchor: `_DayItem.build`: the range's start and end are filled primary
/// circles, a day strictly inside the range sits on the highlight bar the
/// row paints behind it, today is ringed, and a disabled day is greyed and
/// untappable.
#[allow(clippy::too_many_arguments)]
fn range_day_cell(
    id: u64,
    date: Date,
    disabled: bool,
    is_start: bool,
    is_end: bool,
    today: bool,
    handle: StateHandle<DateRangePickerDialogState>,
    palette: &DayCellPalette,
) -> crate::render::RenderPointerRegion {
    let selected = is_start || is_end;
    let (fill, border, text_color) = if selected {
        (Some(palette.primary), None, palette.on_primary)
    } else if today {
        (None, Some(palette.primary), palette.primary)
    } else if disabled {
        (None, None, palette.text.with_alpha(0x61))
    } else {
        (None, None, palette.text)
    };
    let mut circle = Container::new()
        .with_size(40.0, 40.0)
        .with_corner_radius(20.0)
        .with_alignment(Alignment::CENTER)
        .with_child(
            Text::new(format!("{}", date.day))
                .with_size(palette.body_size)
                .with_color(text_color),
        );
    if let Some(fill) = fill {
        circle = circle.with_color(fill);
    }
    if let Some(border) = border {
        circle = circle.with_border(1.0, border);
    }
    let cell = Container::new()
        .with_height(MONTH_ITEM_ROW_HEIGHT)
        .with_alignment(Alignment::CENTER)
        .with_child(circle);
    let mut region = Pointer::new(id, cell);
    if !disabled {
        region = region.with_handlers(PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| range_update_selection(state, date));
        }));
    }
    region
}

/// One week row of the range grid, with the range highlight bar.
///
/// Anchor: `_MonthItem`'s week rows and `_HighlightPainter`. Upstream paints
/// the highlight per cell (half of the start and end cells, all of the ones
/// between, and edge boxes that carry it into the horizontal padding); one
/// rounded bar per row is the same picture: it runs from the middle of the
/// start cell to the middle of the end cell, or off the row's edge when the
/// range continues past it.
struct RangeWeekRow {
    flex: RenderFlex,
    week_first: Date,
    start: Option<Date>,
    end: Option<Date>,
    highlight: Color,
    size: Size,
}

impl RenderBox for RangeWeekRow {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.flex.layout(constraints);
        self.size
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.flex.compute_dry_layout(constraints)
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if let (Some(start), Some(end)) = (self.start, self.end) {
            // A one-day range is two circles and no bar, upstream's
            // `isOneDayRange`.
            if start != end {
                let week_last = add_days_to_date(self.week_first, 6);
                if end >= self.week_first && start <= week_last {
                    let cell_width = self.size.width / 7.0;
                    let x0 = if start >= self.week_first {
                        ((start.to_days() - self.week_first.to_days()) as f32 + 0.5) * cell_width
                    } else {
                        0.0
                    };
                    let x1 = if end <= week_last {
                        ((end.to_days() - self.week_first.to_days()) as f32 + 0.5) * cell_width
                    } else {
                        self.size.width
                    };
                    // The radius is half the row: the bar's ends are stadium
                    // caps tucked under the start and end circles.
                    context.canvas().draw_rounded_rect(
                        Rect::ltrb(
                            offset.dx + x0,
                            offset.dy,
                            offset.dx + x1,
                            offset.dy + self.size.height,
                        ),
                        self.size.height / 2.0,
                        &Paint::new(self.highlight),
                    );
                }
            }
        }
        self.flex.paint(context, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        self.flex.visit_children(visit);
    }
}

/// One week row of a month item.
#[allow(clippy::too_many_arguments)]
fn range_week_row(
    id_base: u64,
    displayed: Date,
    week: u32,
    day_offset: u32,
    days: u32,
    start: Option<Date>,
    end: Option<Date>,
    first_date: Date,
    last_date: Date,
    current_date: Date,
    predicate: Option<SelectableDayForRangePredicate>,
    handle: &StateHandle<DateRangePickerDialogState>,
    palette: &DayCellPalette,
    highlight: Color,
) -> RangeWeekRow {
    let month_first = displayed.first_of_month();
    let week_first = add_days_to_date(month_first, week as i32 * 7 - day_offset as i32);
    let mut flex = Row::new();
    for column in 0..7u32 {
        let index = week * 7 + column;
        let child: RenderRef = if index < day_offset || index >= day_offset + days {
            // The padding cells of `_MonthItem.build`'s `day < 1` branch.
            RenderRef::new(Container::new().with_height(MONTH_ITEM_ROW_HEIGHT))
        } else {
            let date = add_days_to_date(month_first, index as i32 - day_offset as i32);
            let disabled = date > last_date
                || date < first_date
                || predicate.is_some_and(|p| !p(date, start, end));
            RenderRef::new(range_day_cell(
                id_base + date.day as u64,
                date,
                disabled,
                is_same_day(start, Some(date)),
                is_same_day(end, Some(date)),
                is_same_day(Some(current_date), Some(date)),
                handle.clone(),
                palette,
            ))
        };
        flex = flex.push_flex(FlexChild::expanded(child, 1));
    }
    RangeWeekRow {
        flex,
        week_first,
        start,
        end,
        highlight,
        size: Size::ZERO,
    }
}

/// One month item of the range calendar.
///
/// Anchor: `_MonthItem.build`: a 58-tall month-year header, the weeks with 8
/// between rows, and a 12-tall footer.
#[allow(clippy::too_many_arguments)]
fn range_month_item(
    id_base: u64,
    displayed: Date,
    start: Option<Date>,
    end: Option<Date>,
    first_date: Date,
    last_date: Date,
    current_date: Date,
    predicate: Option<SelectableDayForRangePredicate>,
    handle: &StateHandle<DateRangePickerDialogState>,
    palette: &DayCellPalette,
    highlight: Color,
) -> RenderFlex {
    let days = days_in_month(displayed.year, displayed.month);
    let day_offset = first_day_offset(displayed.year, displayed.month, FIRST_DAY_OF_WEEK_INDEX);
    let weeks = (days + day_offset).div_ceil(7);

    let mut grid = Column::new()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(MONTH_ITEM_SPACE_BETWEEN_ROWS);
    for week in 0..weeks {
        grid = grid.push(range_week_row(
            id_base + week as u64 * 100,
            displayed,
            week,
            day_offset,
            days,
            start,
            end,
            first_date,
            last_date,
            current_date,
            predicate,
            handle,
            palette,
            highlight,
        ));
    }
    Column::new()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .push(
            Container::new()
                .with_height(MONTH_ITEM_HEADER_HEIGHT)
                .with_padding(EdgeInsets::symmetric(16.0, 0.0))
                .with_alignment(Alignment::CENTER_LEFT)
                .with_child(
                    Text::new(format_month_year(displayed))
                        .with_size(palette.body_size)
                        .with_color(palette.text),
                ),
        )
        .push(grid)
        .push(Container::new().with_height(MONTH_ITEM_FOOTER_HEIGHT))
}

/// The range a text pair describes, or the error to show.
///
/// Anchor: `_InputDateRangePicker.validate` + `_DateRangePickerDialogState._handleOk`'s
/// input branch. Empty texts stand for the current selection, the pre-filled
/// controllers' stand-in (see the input field's docs); both empty and nothing
/// selected is upstream's null pop.
fn validate_range_texts(
    start_text: &str,
    end_text: &str,
    selected_start: Option<Date>,
    selected_end: Option<Date>,
    first_date: Date,
    last_date: Date,
    predicate: Option<SelectableDayForRangePredicate>,
    error_format_text: Option<&str>,
    error_invalid_text: Option<&str>,
    error_invalid_range_text: Option<&str>,
) -> Result<Option<DateTimeRange>, String> {
    let parse = |text: &str, selected: Option<Date>| -> Result<Option<Date>, String> {
        if text.trim().is_empty() {
            return Ok(selected);
        }
        // `_parseDate` + `_validateDate` + `_isValidAcceptableDate`.
        match parse_compact_date(text) {
            None => Err(error_format_text
                .unwrap_or(INVALID_DATE_FORMAT_LABEL)
                .to_string()),
            Some(date)
                if date < first_date
                    || date > last_date
                    || predicate.is_some_and(|p| !p(date, selected_start, selected_end)) =>
            {
                Err(error_invalid_text
                    .unwrap_or(DATE_OUT_OF_RANGE_LABEL)
                    .to_string())
            }
            Some(date) => Ok(Some(date)),
        }
    };
    let start = parse(start_text, selected_start)?;
    let end = parse(end_text, selected_end)?;
    match (start, end) {
        (Some(start), Some(end)) => {
            if start <= end {
                Ok(Some(DateTimeRange::new(start, end)))
            } else {
                Err(error_invalid_range_text
                    .unwrap_or(INVALID_DATE_RANGE_LABEL)
                    .to_string())
            }
        }
        (None, None) => Ok(None),
        // Half a range is upstream's validate() failing.
        _ => Err(error_invalid_range_text
            .unwrap_or(INVALID_DATE_RANGE_LABEL)
            .to_string()),
    }
}

/// A Material-style date range picker dialog.
///
/// Anchor: `DateRangePickerDialog` in date_picker.dart. The calendar mode is
/// fullscreen in both orientations, as upstream's is (`size` is the
/// `MediaQuery` size with zero inset padding); the input mode is a centered
/// dialog of `_inputPortraitDialogSizeM3` / `_inputRangeLandscapeDialogSize`.
///
/// Upstream's month list is a bidirectional sliver list centered on the
/// initial month; here it is a forward list from `first_date`'s month with
/// the scroll offset of the initial selection's month computed up front --
/// the same first frame, without the negative half of the list.
///
/// Hit-test ids derive from `id`: the month list is `id * 10 + 1`, the input
/// fields `+ 2` and `+ 3`, the mode toggle `+ 4`, OK/Save `+ 5`, Cancel
/// `+ 6`, the close button `+ 7`; day cells are
/// `id * 1_000_000 + month_index * 100 + day`.
pub struct DateRangePickerDialog {
    id: u64,
    initial_date_range: Option<DateTimeRange>,
    first_date: Date,
    last_date: Date,
    current_date: Option<Date>,
    initial_entry_mode: DatePickerEntryMode,
    selectable_day_for_range_predicate: Option<SelectableDayForRangePredicate>,
    help_text: Option<String>,
    cancel_text: Option<String>,
    confirm_text: Option<String>,
    save_text: Option<String>,
    error_format_text: Option<String>,
    error_invalid_text: Option<String>,
    error_invalid_range_text: Option<String>,
    field_start_hint_text: Option<String>,
    field_end_hint_text: Option<String>,
    field_start_label_text: Option<String>,
    field_end_label_text: Option<String>,
    on_confirm: Option<Rc<dyn Fn(DateTimeRange)>>,
    on_cancel: Option<Rc<dyn Fn()>>,
}

/// What a [`DateRangePickerDialog`] remembers between frames.
///
/// Anchor: `_DateRangePickerDialogState`.
#[derive(Default)]
pub struct DateRangePickerDialogState {
    /// Upstream's `_entryMode`.
    pub entry_mode: DatePickerEntryMode,
    /// Upstream's `_selectedStart`.
    pub selected_start: Option<Date>,
    /// Upstream's `_selectedEnd`.
    pub selected_end: Option<Date>,
    /// The input fields' texts, mirrored as they change.
    pub start_text: String,
    pub end_text: String,
    /// The validation error showing in input mode: upstream's autovalidate
    /// flipped on after a failed Save.
    pub input_error: Option<String>,
    /// Upstream's `_scrollController` for the month list.
    pub month_scroll: Scroll,
}

impl DateRangePickerDialog {
    pub fn new(id: u64, first_date: Date, last_date: Date) -> DateRangePickerDialog {
        debug_assert!(first_date <= last_date);
        DateRangePickerDialog {
            id,
            initial_date_range: None,
            first_date,
            last_date,
            current_date: None,
            initial_entry_mode: DatePickerEntryMode::Calendar,
            selectable_day_for_range_predicate: None,
            help_text: None,
            cancel_text: None,
            confirm_text: None,
            save_text: None,
            error_format_text: None,
            error_invalid_text: None,
            error_invalid_range_text: None,
            field_start_hint_text: None,
            field_end_hint_text: None,
            field_start_label_text: None,
            field_end_label_text: None,
            on_confirm: None,
            on_cancel: None,
        }
    }

    pub fn with_initial_date_range(mut self, range: Option<DateTimeRange>) -> Self {
        self.initial_date_range = range;
        self
    }

    /// The day ringed as "today"; [`Date::today`] when unset.
    pub fn with_current_date(mut self, date: Date) -> Self {
        self.current_date = Some(date);
        self
    }

    pub fn with_initial_entry_mode(mut self, mode: DatePickerEntryMode) -> Self {
        self.initial_entry_mode = mode;
        self
    }

    /// Upstream's `selectableDayForRangePredicate`.
    pub fn with_selectable_day_for_range_predicate(
        mut self,
        predicate: SelectableDayForRangePredicate,
    ) -> Self {
        self.selectable_day_for_range_predicate = Some(predicate);
        self
    }

    pub fn with_help_text(mut self, text: impl Into<String>) -> Self {
        self.help_text = Some(text.into());
        self
    }
    pub fn with_cancel_text(mut self, text: impl Into<String>) -> Self {
        self.cancel_text = Some(text.into());
        self
    }
    pub fn with_confirm_text(mut self, text: impl Into<String>) -> Self {
        self.confirm_text = Some(text.into());
        self
    }
    pub fn with_save_text(mut self, text: impl Into<String>) -> Self {
        self.save_text = Some(text.into());
        self
    }
    pub fn with_error_format_text(mut self, text: impl Into<String>) -> Self {
        self.error_format_text = Some(text.into());
        self
    }
    pub fn with_error_invalid_text(mut self, text: impl Into<String>) -> Self {
        self.error_invalid_text = Some(text.into());
        self
    }
    pub fn with_error_invalid_range_text(mut self, text: impl Into<String>) -> Self {
        self.error_invalid_range_text = Some(text.into());
        self
    }
    pub fn with_field_start_hint_text(mut self, text: impl Into<String>) -> Self {
        self.field_start_hint_text = Some(text.into());
        self
    }
    pub fn with_field_end_hint_text(mut self, text: impl Into<String>) -> Self {
        self.field_end_hint_text = Some(text.into());
        self
    }
    pub fn with_field_start_label_text(mut self, text: impl Into<String>) -> Self {
        self.field_start_label_text = Some(text.into());
        self
    }
    pub fn with_field_end_label_text(mut self, text: impl Into<String>) -> Self {
        self.field_end_label_text = Some(text.into());
        self
    }

    /// Called with the picked range when Save/OK is pressed with a complete
    /// range. Replaces the value upstream's `Navigator.pop` delivers.
    pub fn with_on_confirm(mut self, confirm: impl Fn(DateTimeRange) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(confirm));
        self
    }
    /// Called when the dialog is dismissed without a range.
    pub fn with_on_cancel(mut self, cancel: impl Fn() + 'static) -> Self {
        self.on_cancel = Some(Rc::new(cancel));
        self
    }
}

impl StatefulComponent for DateRangePickerDialog {
    type State = DateRangePickerDialogState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    /// `_DateRangePickerDialogState`'s field initializers, plus the initial
    /// scroll: `_CalendarDateRangePickerState`'s `_initialMonthIndex` turned
    /// into an offset by summing the month item heights before it.
    fn initial_state(&self) -> DateRangePickerDialogState {
        let selected_start = self.initial_date_range.map(|range| range.start);
        let selected_end = self.initial_date_range.map(|range| range.end);
        let initial_month = selected_start
            .unwrap_or_else(|| self.current_date.unwrap_or_else(Date::today))
            .first_of_month();
        let mut offset = 0.0;
        let mut month = self.first_date.first_of_month();
        while month < initial_month {
            offset += month_item_height(month.year, month.month);
            month = add_months_to_month_date(month, 1);
        }
        let mut month_scroll = Scroll::default();
        month_scroll.jump_to(offset);
        DateRangePickerDialogState {
            entry_mode: self.initial_entry_mode,
            selected_start,
            selected_end,
            start_text: String::new(),
            end_text: String::new(),
            input_error: None,
            month_scroll,
        }
    }

    fn advance(&self, state: &mut DateRangePickerDialogState, frame_time_micros: i64) -> bool {
        state.month_scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &DateRangePickerDialogState,
        handle: StateHandle<DateRangePickerDialogState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let orientation = orientation_of(context);
        let entry_mode = state.entry_mode;
        let is_calendar = matches!(
            entry_mode,
            DatePickerEntryMode::Calendar | DatePickerEntryMode::CalendarOnly
        );
        let id = self.id;
        let (first_date, last_date) = (self.first_date, self.last_date);
        let current_date = self.current_date.unwrap_or_else(Date::today);
        let predicate = self.selectable_day_for_range_predicate;
        let (start, end) = (state.selected_start, state.selected_end);
        let body_size = theme.body_size;
        let (text, muted, primary, on_primary, surface, outline, danger) = (
            theme.text,
            theme.text_muted,
            theme.primary,
            theme.on_primary,
            theme.surface,
            theme.outline,
            theme.danger,
        );
        // The M3 range highlight is primaryContainer; the crate's theme has
        // no such token, so it is the primary at the emphasis alpha the spec
        // gives it over a surface.
        let highlight = primary.with_alpha(0x3D);

        // -- The entry-mode toggle: upstream's edit/calendar icons, as text
        // here (the Material icons font is not loaded). `calendarOnly` and
        // `inputOnly` have no toggle.
        let can_toggle = matches!(
            entry_mode,
            DatePickerEntryMode::Calendar | DatePickerEntryMode::Input
        );
        let toggle: AnyWidget = if can_toggle {
            let toggle_handle = handle.clone();
            let label = if is_calendar { "Edit" } else { "Calendar" };
            leaf(move || {
                let tap_handle = toggle_handle.clone();
                Pointer::new(
                    id * 10 + 4,
                    Container::new()
                        .with_padding(EdgeInsets::symmetric(8.0, 4.0))
                        .with_child(
                            Text::new(label)
                                .with_size(13.0)
                                .with_weight(500)
                                .with_color(muted),
                        ),
                )
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    tap_handle.set_state(move |state| {
                        // `_handleEntryModeToggle`, including the cleanup of a
                        // range that no longer holds together.
                        state.entry_mode = match state.entry_mode {
                            DatePickerEntryMode::Calendar => {
                                state.input_error = None;
                                DatePickerEntryMode::Input
                            }
                            DatePickerEntryMode::Input => {
                                if let (Some(s), Some(e)) =
                                    (state.selected_start, state.selected_end)
                                {
                                    if s > e {
                                        state.selected_end = None;
                                    }
                                }
                                let selectable = |date: Date| {
                                    date >= first_date
                                        && date <= last_date
                                        && predicate.is_none_or(|p| {
                                            p(date, state.selected_start, state.selected_end)
                                        })
                                };
                                match (state.selected_start, state.selected_end) {
                                    (Some(s), _) if !selectable(s) => {
                                        state.selected_start = None;
                                        state.selected_end = None;
                                    }
                                    (_, Some(e)) if !selectable(e) => {
                                        state.selected_end = None;
                                    }
                                    _ => {}
                                }
                                DatePickerEntryMode::Calendar
                            }
                            mode => mode,
                        };
                    });
                }))
            })
        } else {
            leaf(|| Empty)
        };

        // -- The content: the scrollable month list, or the two input fields.
        let content: AnyWidget = if is_calendar {
            let palette = DayCellPalette {
                body_size,
                text,
                primary,
                on_primary,
            };
            let scroll_offset = state.month_scroll.offset;
            let extent_sink = state.month_scroll.extent.clone();
            let list_handle = handle.clone();
            let down_handle = handle.clone();
            let drag_handle = handle.clone();
            let end_handle = handle.clone();
            let wheel_handle = handle.clone();
            // The same wiring as the year picker: drag, throw, wheel.
            let scroll_handlers = PointerHandlers::new()
                .with_pointer_down(move |_| {
                    down_handle.set_state(|state| state.month_scroll.stop());
                })
                .with_drag_update(move |drag| {
                    drag_handle
                        .set_state(move |state| state.month_scroll.scroll_by(-drag.delta.dy));
                })
                .with_drag_end(move |end| {
                    end_handle.set_state(move |state| state.month_scroll.fling(-end.velocity.dy));
                })
                .with_scroll(move |scroll| {
                    wheel_handle
                        .set_state(move |state| state.month_scroll.scroll_by(scroll.delta.dy));
                });
            leaf(move || {
                let mut list = ListView::new()
                    .with_offset(scroll_offset)
                    .with_extent_sink(extent_sink.clone());
                let months = month_delta(first_date, last_date) + 1;
                let mut month = first_date.first_of_month();
                for month_index in 0..months {
                    list = list.push(range_month_item(
                        id * 1_000_000 + month_index as u64 * 100,
                        month,
                        start,
                        end,
                        first_date,
                        last_date,
                        current_date,
                        predicate,
                        &list_handle,
                        &palette,
                        highlight,
                    ));
                    month = add_months_to_month_date(month, 1);
                }
                Pointer::new(id * 10 + 1, list).with_handlers(scroll_handlers.clone())
            })
        } else {
            let start_handle = handle.clone();
            let end_handle = handle.clone();
            let mut start_field = InputDatePickerFormField::new(id * 10 + 2, first_date, last_date)
                .with_on_text_changed(move |text| {
                    start_handle.set_state(move |state| state.start_text = text);
                });
            let mut end_field = InputDatePickerFormField::new(id * 10 + 3, first_date, last_date)
                .with_on_text_changed(move |text| {
                    end_handle.set_state(move |state| state.end_text = text);
                });
            if let Some(date) = start {
                start_field = start_field.with_initial_date(Some(date));
            }
            if let Some(date) = end {
                end_field = end_field.with_initial_date(Some(date));
            }
            if let Some(hint) = &self.field_start_hint_text {
                start_field = start_field.with_field_hint_text(hint.clone());
            }
            if let Some(hint) = &self.field_end_hint_text {
                end_field = end_field.with_field_hint_text(hint.clone());
            }
            let start_label = self
                .field_start_label_text
                .clone()
                .unwrap_or(DATE_RANGE_START_LABEL.into());
            let end_label = self
                .field_end_label_text
                .clone()
                .unwrap_or(DATE_RANGE_END_LABEL.into());
            start_field = start_field.with_field_label_text(start_label);
            end_field = end_field.with_field_label_text(end_label);
            if let Some(text) = &self.error_format_text {
                start_field = start_field.with_error_format_text(text.clone());
                end_field = end_field.with_error_format_text(text.clone());
            }
            if let Some(text) = &self.error_invalid_text {
                start_field = start_field.with_error_invalid_text(text.clone());
                end_field = end_field.with_error_invalid_text(text.clone());
            }
            // `_InputDateRangePicker`: the two fields side by side.
            many(
                vec![stateful(start_field), stateful(end_field)],
                |mut rendered| {
                    let start_field = rendered.remove(0);
                    let end_field = rendered.remove(0);
                    Row::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(8.0)
                        .push_flex(FlexChild::expanded(start_field, 1))
                        .push_flex(FlexChild::expanded(end_field, 1))
                },
            )
        };

        // -- The confirm action: SAVE in calendar mode (enabled once the
        // range is complete, upstream's `onConfirm: _hasSelectedDateRange ?
        // _handleOk : null`), OK in input mode.
        let ok: AnyWidget = {
            let ok_handle = handle.clone();
            let on_confirm = self.on_confirm.clone();
            let on_cancel = self.on_cancel.clone();
            let error_format_text = self.error_format_text.clone();
            let error_invalid_text = self.error_invalid_text.clone();
            let error_invalid_range_text = self.error_invalid_range_text.clone();
            let label = if is_calendar {
                self.save_text.clone().unwrap_or(SAVE_BUTTON_LABEL.into())
            } else {
                self.confirm_text.clone().unwrap_or(OK_BUTTON_LABEL.into())
            };
            component(
                Button::new(id * 10 + 5, label)
                    .with_style(ButtonVariant::Text)
                    .with_handlers(PointerHandlers::new().with_tap(move |_| {
                        let on_confirm = on_confirm.clone();
                        let on_cancel = on_cancel.clone();
                        let error_format_text = error_format_text.clone();
                        let error_invalid_text = error_invalid_text.clone();
                        let error_invalid_range_text = error_invalid_range_text.clone();
                        ok_handle.set_state(move |state| {
                            // `_handleOk`.
                            if matches!(
                                state.entry_mode,
                                DatePickerEntryMode::Input | DatePickerEntryMode::InputOnly
                            ) {
                                match validate_range_texts(
                                    &state.start_text,
                                    &state.end_text,
                                    state.selected_start,
                                    state.selected_end,
                                    first_date,
                                    last_date,
                                    predicate,
                                    error_format_text.as_deref(),
                                    error_invalid_text.as_deref(),
                                    error_invalid_range_text.as_deref(),
                                ) {
                                    Ok(Some(range)) => {
                                        state.selected_start = Some(range.start);
                                        state.selected_end = Some(range.end);
                                        state.input_error = None;
                                        if let Some(confirm) = &on_confirm {
                                            confirm(range);
                                        }
                                    }
                                    // The null pop.
                                    Ok(None) => {
                                        if let Some(cancel) = &on_cancel {
                                            cancel();
                                        }
                                    }
                                    Err(message) => state.input_error = Some(message),
                                }
                                return;
                            }
                            match (state.selected_start, state.selected_end) {
                                (Some(start), Some(end)) => {
                                    if let Some(confirm) = &on_confirm {
                                        confirm(DateTimeRange::new(start, end));
                                    }
                                }
                                // The disabled Save: nothing to confirm yet.
                                _ => {}
                            }
                        });
                    })),
            )
        };
        let cancel: AnyWidget = {
            let on_cancel = self.on_cancel.clone();
            component(
                Button::new(
                    id * 10 + 6,
                    self.cancel_text
                        .clone()
                        .unwrap_or_else(|| CANCEL_BUTTON_LABEL.into()),
                )
                .with_style(ButtonVariant::Text)
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    if let Some(cancel) = &on_cancel {
                        cancel();
                    }
                })),
            )
        };
        let close: AnyWidget = {
            let on_cancel = self.on_cancel.clone();
            leaf(move || {
                let on_cancel = on_cancel.clone();
                // The AppBar's CloseButton, as text.
                Pointer::new(
                    id * 10 + 7,
                    Container::new()
                        .with_padding(EdgeInsets::all(12.0))
                        .with_child(
                            Text::new("X")
                                .with_size(body_size + 2.0)
                                .with_color(on_primary),
                        ),
                )
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    if let Some(cancel) = &on_cancel {
                        cancel();
                    }
                }))
            })
        };

        // -- Assembly.
        let help_text = self.help_text.clone().unwrap_or_else(|| {
            if is_calendar {
                DATE_RANGE_PICKER_HELP_TEXT.into()
            } else {
                DATE_RANGE_PICKER_HELP_TEXT.into()
            }
        });
        let start_text = start.map(format_medium_date).unwrap_or_default();
        let end_text = end.map(format_medium_date).unwrap_or_else(|| {
            if start.is_some() {
                DATE_RANGE_END_LABEL.into()
            } else {
                String::new()
            }
        });
        let input_error = state.input_error.clone();
        let max_calendar_width = match orientation {
            Orientation::Portrait => RANGE_MAX_CALENDAR_WIDTH_PORTRAIT,
            Orientation::Landscape => RANGE_MAX_CALENDAR_WIDTH_LANDSCAPE,
        };
        let weekday_style = theme.muted();

        many(
            vec![toggle, content, ok, cancel, close],
            move |mut rendered| {
                let toggle = rendered.remove(0);
                let content = rendered.remove(0);
                let ok = rendered.remove(0);
                let cancel = rendered.remove(0);
                let close = rendered.remove(0);

                let dialog: Box<dyn RenderBox> = if is_calendar {
                    // `_CalendarRangePickerDialog`: a fullscreen scaffold whose
                    // app bar carries the close button and Save, with the help
                    // text and the "start – end" line below it.
                    let top_bar = Container::new()
                        .with_height(56.0)
                        .with_padding(EdgeInsets::symmetric(4.0, 0.0))
                        .with_child(
                            Row::new()
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .push(close)
                                .push_flex(FlexChild::expanded(Container::new(), 1))
                                .push(toggle)
                                .push(ok),
                        );
                    let range_line = Row::new()
                        .with_spacing(4.0)
                        .push(
                            Text::new(start_text.clone())
                                .with_size(28.0)
                                .with_color(on_primary),
                        )
                        .push(
                            Text::new(if start.is_some() { " – " } else { "" })
                                .with_size(28.0)
                                .with_color(on_primary),
                        )
                        .push(
                            Text::new(end_text.clone())
                                .with_size(28.0)
                                .with_color(on_primary),
                        );
                    let header = Container::new().with_color(primary).with_child(
                        Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .push(top_bar)
                            .push(
                                Container::new()
                                    .with_height(64.0)
                                    .with_padding(EdgeInsets::symmetric(24.0, 0.0))
                                    .with_child(
                                        Column::new()
                                            .with_main_axis_size(MainAxisSize::Min)
                                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                                            .with_spacing(8.0)
                                            .push(
                                                Text::new(help_text.clone())
                                                    .with_size(12.0)
                                                    .with_color(on_primary),
                                            )
                                            .push(range_line),
                                    ),
                            ),
                    );
                    // `_DayHeaders`: the narrow weekday letters over the list.
                    let mut day_headers = Row::new();
                    for i in 0..7u32 {
                        let weekday = NARROW_WEEKDAYS[((FIRST_DAY_OF_WEEK_INDEX + i) % 7) as usize];
                        day_headers = day_headers.push_flex(FlexChild::expanded(
                            Container::new()
                                .with_height(MONTH_ITEM_ROW_HEIGHT)
                                .with_alignment(Alignment::CENTER)
                                .with_child(Text::new(weekday).with_style(weekday_style.clone())),
                            1,
                        ));
                    }
                    let calendar = Container::new().with_width(max_calendar_width).with_child(
                        Column::new()
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .push(
                                Container::new()
                                    .with_padding(EdgeInsets::symmetric(8.0, 0.0))
                                    .with_child(day_headers),
                            )
                            .push(Container::new().with_height(1.0).with_color(outline))
                            .push_flex(FlexChild::expanded(content, 1)),
                    );
                    Box::new(
                        Container::new().with_color(surface).with_child(
                            Column::new()
                                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                                .push(header)
                                .push_flex(FlexChild::expanded(Center::new(calendar), 1)),
                        ),
                    )
                } else {
                    // `_InputDateRangePickerDialog`: a centered dialog with the
                    // help text and toggle on top, the fields, and the actions.
                    let (width, height) = match orientation {
                        Orientation::Portrait => RANGE_INPUT_PORTRAIT_DIALOG_SIZE,
                        Orientation::Landscape => RANGE_INPUT_LANDSCAPE_DIALOG_SIZE,
                    };
                    let mut column = Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(
                            Container::new()
                                .with_padding(EdgeInsets::only(24.0, 16.0, 12.0, 0.0))
                                .with_child(
                                    Row::new()
                                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                        .push_flex(FlexChild::expanded(
                                            Text::new(help_text.clone())
                                                .with_size(12.0)
                                                .with_color(muted),
                                            1,
                                        ))
                                        .push(toggle),
                                ),
                        )
                        .push_flex(FlexChild::expanded(
                            Container::new()
                                .with_padding(EdgeInsets::symmetric(24.0, 0.0))
                                .with_alignment(Alignment::CENTER)
                                .with_child(content),
                            1,
                        ));
                    if let Some(message) = &input_error {
                        column = column.push(
                            Container::new()
                                .with_padding(EdgeInsets::symmetric(24.0, 0.0))
                                .with_child(
                                    Text::new(message.clone())
                                        .with_size(body_size - 2.0)
                                        .with_color(danger),
                                ),
                        );
                    }
                    column = column.push(
                        Container::new()
                            .with_height(52.0)
                            .with_padding(EdgeInsets::symmetric(8.0, 0.0))
                            .with_alignment(Alignment::CENTER_RIGHT)
                            .with_child(
                                Row::new()
                                    .with_main_axis_alignment(MainAxisAlignment::End)
                                    .with_spacing(8.0)
                                    .push(cancel)
                                    .push(ok),
                            ),
                    );
                    Box::new(Center::new(
                        Container::new()
                            .with_size(width, height)
                            .with_color(surface)
                            .with_corner_radius(28.0)
                            .with_elevation(6)
                            .with_child(column),
                    ))
                };
                dialog
            },
        )
    }
}

/// The date range picker dialog over a scrim, ready to stack over the page.
///
/// Anchor: `showDateRangePicker`; the same returned-overlay stand-in as
/// [`show_date_picker`]. The dialog fills the overlay in calendar mode and
/// centers itself in input mode, so it is positioned to fill here. The
/// scrim's hit-test id is `dialog.id * 10 + 9`.
pub fn show_date_range_picker(dialog: DateRangePickerDialog) -> AnyWidget {
    let on_cancel = dialog.on_cancel.clone();
    let scrim_id = dialog.id * 10 + 9;
    let scrim: AnyWidget = leaf(move || {
        let on_cancel = on_cancel.clone();
        Pointer::new(
            scrim_id,
            Container::new().with_color(Color::argb(0x8A, 0, 0, 0)),
        )
        .with_handlers(PointerHandlers::new().with_tap(move |_| {
            if let Some(cancel) = &on_cancel {
                cancel();
            }
        }))
    });
    many(vec![scrim, stateful(dialog)], |mut rendered| {
        let scrim = rendered.remove(0);
        let dialog = rendered.remove(0);
        RenderStack::new()
            .push_boxed(scrim)
            .push_positioned(dialog, Positioned::fill())
    })
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

// -- The calendar behind a picker ---------------------------------------------

/// Upstream `CalendarDelegate` (`material/date.dart`): the seam that lets a
/// date picker show a calendar that is not the Gregorian one.
///
/// Every question a picker asks about dates goes through here -- how many days
/// are in this month, how many blanks lead the grid, what comes a month later
/// -- so that a Hijri or Japanese-era calendar can answer differently without
/// the picker knowing there is more than one answer. [`GregorianCalendarDelegate`]
/// is the one upstream ships and the one the crate defaults to.
///
/// # What is here and what is waiting
///
/// The arithmetic half is here, which is the whole of what a picker's *layout*
/// needs. Upstream's other twelve methods are formatting and parsing --
/// `formatMonthYear`, `formatYear`, `formatMediumDate`, `formatShortMonthDay`,
/// `formatShortDate`, `formatFullDate`, `formatCompactDate`,
/// `parseCompactDate`, `dateHelpText` -- and every one of them takes a
/// `MaterialLocalizations`, which this crate does not have yet (the
/// localization wave). They are named here rather than stubbed, so that adding
/// them later is an addition and not a correction.
///
/// # `date_only` is the identity here
///
/// Upstream's `dateOnly` strips the time from a `DateTime`, because upstream's
/// dates carry one and two dates that differ only by a few hours are the same
/// square on a calendar. This crate's [`Date`] has no time at all -- it *is*
/// the date-only type -- so the method is an identity. It stays in the trait
/// because a delegate for a calendar whose dates do carry time would need it,
/// and because a caller writing `delegate.date_only(d)` should not have to
/// know which kind it has.
pub trait CalendarDelegate {
    /// Upstream's `now()`.
    fn now(&self) -> Date;

    /// Upstream's `dateOnly`. See the trait docs: an identity for [`Date`].
    fn date_only(&self, date: Date) -> Date {
        date
    }

    /// Upstream's `datesOnly`, which is `dateOnly` on both ends.
    fn dates_only(&self, range: DateTimeRange) -> DateTimeRange {
        DateTimeRange::new(self.date_only(range.start), self.date_only(range.end))
    }

    /// Upstream's `isSameDay`, which answers **true for two nulls** -- the
    /// comparison is field by field on optionals, so "no date" equals "no
    /// date". A picker asking whether the selection changed relies on that.
    fn is_same_day(&self, a: Option<Date>, b: Option<Date>) -> bool {
        is_same_day(a, b)
    }

    /// Upstream's `isSameMonth`, with the same rule about two nulls.
    fn is_same_month(&self, a: Option<Date>, b: Option<Date>) -> bool {
        is_same_month(a, b)
    }

    /// Upstream's `monthDelta`: how many months apart two dates are, which is
    /// how a picker turns a scroll offset into a page.
    fn month_delta(&self, start: Date, end: Date) -> i32;

    /// Upstream's `addMonthsToMonthDate`.
    fn add_months_to_month_date(&self, month_date: Date, months_to_add: i32) -> Date;

    /// Upstream's `addDaysToDate`.
    fn add_days_to_date(&self, date: Date, days: i32) -> Date;

    /// Upstream's `firstDayOffset`: how many blanks lead the calendar grid.
    ///
    /// Upstream takes a `MaterialLocalizations` and reads
    /// `firstDayOfWeekIndex` off it; here the index is passed directly, since
    /// that one number is all the method ever wanted from it.
    fn first_day_offset(&self, year: i32, month: u32, first_day_of_week_index: u32) -> u32;

    /// Upstream's `getDaysInMonth`.
    fn days_in_month(&self, year: i32, month: u32) -> u32;

    /// Upstream's `getMonth`: the first of that month.
    fn get_month(&self, year: i32, month: u32) -> Date;

    /// Upstream's `getDay`.
    fn get_day(&self, year: i32, month: u32, day: u32) -> Date;
}

/// Upstream `GregorianCalendarDelegate`: the calendar everything defaults to.
///
/// Upstream's body is delegation -- every method forwards to the matching
/// `DateUtils` static -- and so is this one, to the free functions in this
/// module. That is the shape worth keeping: the arithmetic is usable without
/// a delegate at all, and the delegate exists to make it *replaceable*, not to
/// own it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GregorianCalendarDelegate;

impl CalendarDelegate for GregorianCalendarDelegate {
    fn now(&self) -> Date {
        Date::today()
    }

    fn month_delta(&self, start: Date, end: Date) -> i32 {
        month_delta(start, end)
    }

    fn add_months_to_month_date(&self, month_date: Date, months_to_add: i32) -> Date {
        add_months_to_month_date(month_date, months_to_add)
    }

    fn add_days_to_date(&self, date: Date, days: i32) -> Date {
        add_days_to_date(date, days)
    }

    fn first_day_offset(&self, year: i32, month: u32, first_day_of_week_index: u32) -> u32 {
        first_day_offset(year, month, first_day_of_week_index)
    }

    fn days_in_month(&self, year: i32, month: u32) -> u32 {
        days_in_month(year, month)
    }

    fn get_month(&self, year: i32, month: u32) -> Date {
        Date::new(year, month as i32, 1)
    }

    fn get_day(&self, year: i32, month: u32, day: u32) -> Date {
        Date::new(year, month as i32, day as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Theme;
    use crate::framework::{ElementTree, provide};
    use crate::render::BoxConstraints;

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height))
    }

    fn date(year: i32, month: i32, day: i32) -> Date {
        Date::new(year, month, day)
    }

    // -- Date math (anchors: DateUtils in date.dart) --------------------------

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
    }

    #[test]
    fn construction_rolls_over_like_datetimes() {
        assert_eq!(date(2024, 13, 1), date(2025, 1, 1));
        assert_eq!(date(2024, 1, 32), date(2024, 2, 1));
        assert_eq!(date(2024, 3, 0), date(2024, 2, 29));
        assert_eq!(date(2024, 0, 1), date(2023, 12, 1));
    }

    #[test]
    fn the_epoch_was_a_thursday() {
        assert_eq!(date(1970, 1, 1).weekday(), 4);
        assert_eq!(date(1970, 1, 4).weekday(), 7);
        assert_eq!(date(1970, 1, 5).weekday(), 1);
    }

    #[test]
    fn first_day_offset_counts_leading_blanks() {
        // September 2017 started on a Friday: five blanks in a Sunday-first
        // grid, upstream `DateUtils.firstDayOffset(2017, 9, ...)`.
        assert_eq!(first_day_offset(2017, 9, FIRST_DAY_OF_WEEK_INDEX), 5);
        // January 2024 started on a Monday: one blank.
        assert_eq!(first_day_offset(2024, 1, FIRST_DAY_OF_WEEK_INDEX), 1);
    }

    #[test]
    fn month_delta_and_add_months_round_trip() {
        assert_eq!(month_delta(date(2020, 1, 31), date(2020, 3, 15)), 2);
        assert_eq!(month_delta(date(2020, 3, 15), date(2020, 1, 31)), -2);
        assert_eq!(
            add_months_to_month_date(date(2020, 1, 15), 14),
            date(2021, 3, 1)
        );
        assert_eq!(
            add_months_to_month_date(date(2021, 3, 1), -14),
            date(2020, 1, 1)
        );
    }

    #[test]
    fn add_days_crosses_month_and_year_bounds() {
        assert_eq!(add_days_to_date(date(2023, 12, 31), 1), date(2024, 1, 1));
        assert_eq!(add_days_to_date(date(2024, 3, 1), -1), date(2024, 2, 29));
        assert_eq!(add_days_to_date(date(2023, 3, 1), -1), date(2023, 2, 28));
    }

    // -- Formatting and parsing ------------------------------------------------

    #[test]
    fn compact_dates_format_and_parse() {
        let day = date(2026, 8, 17);
        assert_eq!(format_compact_date(day), "08/17/2026");
        assert_eq!(parse_compact_date("08/17/2026"), Some(day));
        assert_eq!(parse_compact_date("2/3/2024"), Some(date(2024, 2, 3)));
        assert_eq!(parse_compact_date("17/08/2026"), None);
        assert_eq!(parse_compact_date("not a date"), None);
        assert_eq!(parse_compact_date("02/30/2024"), None);
    }

    #[test]
    fn medium_and_full_dates_name_the_weekday() {
        assert_eq!(format_medium_date(date(2026, 8, 17)), "Mon, Aug 17");
        assert_eq!(
            format_full_date(date(2026, 8, 17)),
            "Monday, August 17, 2026"
        );
        assert_eq!(format_month_year(date(2026, 8, 17)), "August 2026");
    }

    // -- TimeOfDay (anchor: time.dart) ------------------------------------------

    #[test]
    fn hours_map_to_periods() {
        assert_eq!(TimeOfDay::new(13, 5).period(), DayPeriod::Pm);
        assert_eq!(TimeOfDay::new(13, 5).hour_of_period(), 1);
        assert_eq!(TimeOfDay::new(0, 0).hour_of_period(), 12);
        assert_eq!(TimeOfDay::new(12, 0).hour_of_period(), 12);
        assert_eq!(TimeOfDay::new(12, 0).period_offset(), 12);
        assert_eq!(
            TimeOfDay::new(9, 30).replacing(Some(10), None),
            TimeOfDay::new(10, 30)
        );
    }

    #[test]
    fn times_format_both_ways() {
        assert_eq!(TimeOfDay::new(13, 5).format(false), "1:05 PM");
        assert_eq!(TimeOfDay::new(13, 5).format(true), "13:05");
        assert_eq!(TimeOfDay::new(0, 30).format(false), "12:30 AM");
    }

    // -- The dial math (anchor: _DialState in time_picker.dart) ------------------

    #[test]
    fn dart_mod_takes_the_divisors_sign() {
        assert!((dart_mod(-0.5, TWO_PI) - (TWO_PI - 0.5)).abs() < 1e-6);
        assert!((dart_mod(0.5, -1.0) - -0.5).abs() < 1e-6);
        assert!((dart_mod(0.5, 1.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn theta_and_time_round_trip() {
        let time = TimeOfDay::new(3, 45);
        let theta = theta_for_time(time, HourMinuteMode::Hour);
        assert_eq!(
            time_for_theta(theta, false, 1.0, HourMinuteMode::Hour, false, time),
            time
        );
        let theta = theta_for_time(time, HourMinuteMode::Minute);
        assert_eq!(
            time_for_theta(theta, false, 1.0, HourMinuteMode::Minute, false, time),
            time
        );
        // The 24-hour dial's inner ring is the afternoon.
        let time = TimeOfDay::new(15, 0);
        let theta = theta_for_time(time, HourMinuteMode::Hour);
        assert_eq!(
            time_for_theta(theta, false, 0.0, HourMinuteMode::Hour, true, time),
            time
        );
        // ...and the outer ring the morning.
        let time = TimeOfDay::new(3, 0);
        let theta = theta_for_time(time, HourMinuteMode::Hour);
        assert_eq!(
            time_for_theta(theta, false, 1.0, HourMinuteMode::Hour, true, time),
            time
        );
    }

    #[test]
    fn taps_round_minutes_to_five() {
        let selected = TimeOfDay::new(9, 0);
        // 47 minutes round to 45; 48 round to 50.
        let theta = theta_for_time(TimeOfDay::new(9, 47), HourMinuteMode::Minute);
        assert_eq!(
            time_for_theta(theta, true, 1.0, HourMinuteMode::Minute, false, selected),
            TimeOfDay::new(9, 45)
        );
        let theta = theta_for_time(TimeOfDay::new(9, 48), HourMinuteMode::Minute);
        assert_eq!(
            time_for_theta(theta, true, 1.0, HourMinuteMode::Minute, false, selected),
            TimeOfDay::new(9, 50)
        );
    }

    #[test]
    fn hour_and_minute_texts_parse_the_upstream_way() {
        assert_eq!(parse_hour_text("13", false, DayPeriod::Pm), None);
        assert_eq!(parse_hour_text("1", false, DayPeriod::Pm), Some(13));
        assert_eq!(parse_hour_text("12", false, DayPeriod::Am), Some(0));
        assert_eq!(parse_hour_text("12", false, DayPeriod::Pm), Some(12));
        assert_eq!(parse_hour_text("23", true, DayPeriod::Am), Some(23));
        assert_eq!(parse_hour_text("24", true, DayPeriod::Am), None);
        assert_eq!(parse_hour_text("x", true, DayPeriod::Am), None);
        assert_eq!(parse_minute_text("59"), Some(59));
        assert_eq!(parse_minute_text("60"), None);
    }

    // -- Range selection ---------------------------------------------------------

    #[test]
    fn range_selection_follows_the_documented_rules() {
        let mut state = DateRangePickerDialogState::default();
        // From the unselected state, one pick creates the start.
        range_update_selection(&mut state, date(2024, 5, 10));
        assert_eq!(state.selected_start, Some(date(2024, 5, 10)));
        assert_eq!(state.selected_end, None);
        // On or after the start sets the end.
        range_update_selection(&mut state, date(2024, 5, 12));
        assert_eq!(state.selected_end, Some(date(2024, 5, 12)));
        // A complete range restarts on the next pick.
        range_update_selection(&mut state, date(2024, 5, 20));
        assert_eq!(state.selected_start, Some(date(2024, 5, 20)));
        assert_eq!(state.selected_end, None);
        // Before the start resets the start.
        range_update_selection(&mut state, date(2024, 5, 18));
        assert_eq!(state.selected_start, Some(date(2024, 5, 18)));
        assert_eq!(state.selected_end, None);
    }

    #[test]
    fn range_texts_validate_to_a_range_or_an_error() {
        let first = date(2024, 1, 1);
        let last = date(2024, 12, 31);
        let valid = validate_range_texts(
            "05/10/2024",
            "05/12/2024",
            None,
            None,
            first,
            last,
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            valid,
            Ok(Some(DateTimeRange::new(
                date(2024, 5, 10),
                date(2024, 5, 12)
            )))
        );
        // Start after end is the invalid-range error.
        let backwards = validate_range_texts(
            "05/12/2024",
            "05/10/2024",
            None,
            None,
            first,
            last,
            None,
            None,
            None,
            None,
        );
        assert_eq!(backwards, Err(INVALID_DATE_RANGE_LABEL.to_string()));
        // Both empty and nothing selected is the null pop.
        let empty = validate_range_texts("", "", None, None, first, last, None, None, None, None);
        assert_eq!(empty, Ok(None));
        // An empty field falls back to the selection.
        let partial = validate_range_texts(
            "",
            "05/12/2024",
            Some(date(2024, 5, 10)),
            None,
            first,
            last,
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            partial,
            Ok(Some(DateTimeRange::new(
                date(2024, 5, 10),
                date(2024, 5, 12)
            )))
        );
    }

    #[test]
    fn month_item_height_matches_the_upstream_formula() {
        // January 2024: one leading blank, 31 days, five weeks.
        assert_eq!(
            month_item_height(2024, 1),
            58.0 + 5.0 * 42.0 + 4.0 * 8.0 + 12.0
        );
    }

    // -- Layout -------------------------------------------------------------------

    #[test]
    fn a_calendar_date_picker_stacks_subheader_over_grid() {
        // January 2024 has five week rows; the height is the 52-tall
        // subheader over the 48-tall weekday header and five 48-tall weeks.
        let size = lay_out(
            stateful(
                CalendarDatePicker::new(1, date(2024, 1, 1), date(2024, 12, 31))
                    .with_initial_date(Some(date(2024, 1, 15)))
                    .with_current_date(date(2024, 1, 15)),
            ),
            400.0,
            800.0,
        );
        assert_eq!(size.height, 52.0 + 6.0 * 48.0);
    }

    #[test]
    fn a_date_picker_dialog_takes_the_m3_sizes() {
        let size = lay_out(
            stateful(
                DatePickerDialog::new(1, date(2024, 1, 1), date(2030, 12, 31))
                    .with_current_date(date(2024, 1, 15)),
            ),
            800.0,
            700.0,
        );
        assert_eq!(size, Size::new(360.0, 568.0));
        let size = lay_out(
            stateful(
                DatePickerDialog::new(1, date(2024, 1, 1), date(2030, 12, 31))
                    .with_current_date(date(2024, 1, 15))
                    .with_initial_entry_mode(DatePickerEntryMode::Input),
            ),
            800.0,
            700.0,
        );
        assert_eq!(size, Size::new(328.0, 270.0));
    }

    #[test]
    fn a_time_picker_dialog_takes_the_m3_sizes() {
        let size = lay_out(
            stateful(TimePickerDialog::new(1, TimeOfDay::new(9, 30))),
            800.0,
            700.0,
        );
        assert_eq!(size, Size::new(310.0, 468.0));
        let size = lay_out(
            stateful(
                TimePickerDialog::new(1, TimeOfDay::new(9, 30))
                    .with_initial_entry_mode(TimePickerEntryMode::Input),
            ),
            800.0,
            700.0,
        );
        assert_eq!(size, Size::new(312.0, 252.0));
    }

    #[test]
    fn a_24_hour_time_picker_lays_out() {
        let size = lay_out(
            stateful(
                TimePickerDialog::new(1, TimeOfDay::new(15, 30))
                    .with_always_use_24_hour_format(true),
            ),
            800.0,
            700.0,
        );
        assert_eq!(size, Size::new(310.0, 468.0));
    }

    #[test]
    fn a_date_range_picker_fills_the_overlay_in_calendar_mode() {
        let size = lay_out(
            stateful(
                DateRangePickerDialog::new(1, date(2024, 1, 1), date(2024, 12, 31))
                    .with_current_date(date(2024, 1, 15)),
            ),
            800.0,
            700.0,
        );
        assert_eq!(size, Size::new(800.0, 700.0));
    }

    #[test]
    fn a_date_range_picker_centers_the_input_dialog() {
        let size = lay_out(
            stateful(
                DateRangePickerDialog::new(1, date(2024, 1, 1), date(2024, 12, 31))
                    .with_current_date(date(2024, 1, 15))
                    .with_initial_entry_mode(DatePickerEntryMode::Input),
            ),
            800.0,
            700.0,
        );
        // The Center wrapper fills; the dialog inside is 328x270.
        assert_eq!(size, Size::new(800.0, 700.0));
    }

    #[test]
    fn the_overlays_fill_their_stack() {
        let size = lay_out(
            show_date_picker(
                DatePickerDialog::new(7, date(2024, 1, 1), date(2030, 12, 31))
                    .with_current_date(date(2024, 1, 15)),
            ),
            800.0,
            700.0,
        );
        assert_eq!(size, Size::new(800.0, 700.0));
        let size = lay_out(
            show_time_picker(TimePickerDialog::new(7, TimeOfDay::new(9, 30))),
            800.0,
            700.0,
        );
        assert_eq!(size, Size::new(800.0, 700.0));
        let size = lay_out(
            show_date_range_picker(
                DateRangePickerDialog::new(7, date(2024, 1, 1), date(2024, 12, 31))
                    .with_current_date(date(2024, 1, 15)),
            ),
            800.0,
            700.0,
        );
        assert_eq!(size, Size::new(800.0, 700.0));
    }

    #[test]
    fn the_gregorian_delegate_answers_what_the_free_functions_do() {
        // Upstream's body is delegation to `DateUtils`, and so is this one.
        // The point of the class is that the arithmetic is *replaceable*, not
        // that it owns it -- so the two must not be able to drift.
        let calendar = GregorianCalendarDelegate;
        assert_eq!(calendar.days_in_month(2024, 2), days_in_month(2024, 2));
        assert_eq!(calendar.days_in_month(1900, 2), 28, "not a leap year");
        assert_eq!(calendar.days_in_month(2000, 2), 29, "but this one is");
        let start = Date::new(2024, 1, 31);
        assert_eq!(
            calendar.month_delta(start, Date::new(2025, 3, 1)),
            month_delta(start, Date::new(2025, 3, 1))
        );
        assert_eq!(
            calendar.add_days_to_date(start, 1),
            Date::new(2024, 2, 1),
            "the end of January is followed by February"
        );
    }

    #[test]
    fn date_only_is_the_identity_because_a_date_here_carries_no_time() {
        // Upstream strips the time, because two `DateTime`s a few hours apart
        // are the same square on a calendar. This crate's `Date` *is* the
        // date-only type, so the method has nothing to strip -- and it stays
        // on the trait so a caller never has to know which kind it has.
        let calendar = GregorianCalendarDelegate;
        let date = Date::new(2024, 6, 15);
        assert_eq!(calendar.date_only(date), date);
        let range = DateTimeRange::new(Date::new(2024, 1, 1), Date::new(2024, 12, 31));
        assert_eq!(calendar.dates_only(range), range);
    }

    #[test]
    fn two_missing_dates_are_the_same_day() {
        // Upstream compares field by field on optionals, so "no date" equals
        // "no date" -- which is what a picker asking whether its selection
        // changed relies on.
        let calendar = GregorianCalendarDelegate;
        assert!(calendar.is_same_day(None, None));
        assert!(calendar.is_same_month(None, None));
        assert!(!calendar.is_same_day(Some(Date::new(2024, 6, 15)), None));
        assert!(calendar.is_same_month(Some(Date::new(2024, 6, 1)), Some(Date::new(2024, 6, 30))));
        assert!(!calendar.is_same_day(Some(Date::new(2024, 6, 1)), Some(Date::new(2024, 6, 30))));
    }

    #[test]
    fn adding_months_to_a_month_date_lands_on_the_first() {
        // Upstream's `DateTime(monthDate.year, monthDate.month + monthsToAdd)`
        // with no day at all, which Dart reads as the first. Without that, a
        // picker paging from the 31st of January would land on the 3rd of
        // March.
        let calendar = GregorianCalendarDelegate;
        let january_31 = Date::new(2024, 1, 31);
        assert_eq!(
            calendar.add_months_to_month_date(january_31, 1),
            Date::new(2024, 2, 1)
        );
        assert_eq!(
            calendar.add_months_to_month_date(january_31, 13),
            Date::new(2025, 2, 1),
            "and rolls the year over"
        );
        // Backwards too, which is how a picker pages up.
        assert_eq!(
            calendar.add_months_to_month_date(january_31, -1),
            Date::new(2023, 12, 1)
        );
    }

    #[test]
    fn the_grids_leading_blanks_follow_the_locales_first_day_of_the_week() {
        // The one number upstream reads off `MaterialLocalizations`, passed
        // directly here. The 1st of June 2024 was a Saturday.
        let calendar = GregorianCalendarDelegate;
        assert_eq!(Date::new(2024, 6, 1).weekday(), 6, "Saturday");
        // Sunday-first (en_US): Saturday is the seventh column, so six blanks.
        assert_eq!(calendar.first_day_offset(2024, 6, 0), 6);
        // Monday-first: Saturday is the sixth, so five.
        assert_eq!(calendar.first_day_offset(2024, 6, 1), 5);
        // Saturday-first: no blanks at all.
        assert_eq!(calendar.first_day_offset(2024, 6, 6), 0);
    }

    #[test]
    fn get_month_and_get_day_build_the_dates_a_grid_is_made_of() {
        let calendar = GregorianCalendarDelegate;
        assert_eq!(calendar.get_month(2024, 6), Date::new(2024, 6, 1));
        assert_eq!(calendar.get_day(2024, 6, 15), Date::new(2024, 6, 15));
    }

    #[test]
    fn a_delegate_can_answer_differently_which_is_the_point_of_the_seam() {
        // The whole reason the picker asks a delegate rather than the free
        // functions: a calendar that is not the Gregorian one plugs in here.
        // A thirteen-month calendar of twenty-eight days each stands in.
        struct ThirteenMonths;
        impl CalendarDelegate for ThirteenMonths {
            fn now(&self) -> Date {
                Date::new(2024, 1, 1)
            }
            fn month_delta(&self, start: Date, end: Date) -> i32 {
                (end.year - start.year) * 13 + end.month as i32 - start.month as i32
            }
            fn add_months_to_month_date(&self, month_date: Date, months_to_add: i32) -> Date {
                Date::new(month_date.year, month_date.month as i32 + months_to_add, 1)
            }
            fn add_days_to_date(&self, date: Date, days: i32) -> Date {
                add_days_to_date(date, days)
            }
            fn first_day_offset(&self, _year: i32, _month: u32, _index: u32) -> u32 {
                0
            }
            fn days_in_month(&self, _year: i32, _month: u32) -> u32 {
                28
            }
            fn get_month(&self, year: i32, month: u32) -> Date {
                Date::new(year, month as i32, 1)
            }
            fn get_day(&self, year: i32, month: u32, day: u32) -> Date {
                Date::new(year, month as i32, day as i32)
            }
        }
        let calendar = ThirteenMonths;
        assert_eq!(calendar.days_in_month(2024, 2), 28);
        assert_eq!(
            calendar.month_delta(Date::new(2024, 1, 1), Date::new(2025, 1, 1)),
            13
        );
        // And it still gets the provided methods for free, which is what
        // makes the trait worth having rather than nine separate callbacks.
        assert!(calendar.is_same_day(None, None));
        assert_eq!(
            calendar.date_only(Date::new(2024, 6, 15)),
            Date::new(2024, 6, 15)
        );
    }
}
