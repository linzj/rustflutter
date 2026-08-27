//! Ports of `cupertino/date_picker.dart`'s `CupertinoDatePicker` and
//! `CupertinoTimerPicker`, and `cupertino/menu_anchor.dart`'s
//! `CupertinoMenuEntry`, `CupertinoMenuAnchor`, `CupertinoMenuDivider` and
//! `CupertinoMenuItem`.
//!
//! A wheel has no ends, and a menu item's business is mostly with its
//! neighbours.
//!
//! # What the two pickers do and do not carry
//!
//! Both are whole widgets now: the columns, the widths measured from the
//! longest string each column can show, the one selection band that spans them
//! all, and the axis every column appears to turn on. What is not carried:
//!
//! * **The looping columns loop a very long way rather than for ever.**
//!   Upstream's `ListWheelChildLoopingListDelegate` reports no child count at
//!   all and answers for every index there is, negative ones included; this
//!   crate's wheel is a finite list with a scroll extent, so
//!   [`crate::cupertino::CupertinoPicker::with_looping`] lays out some
//!   thousands of turns and starts in the middle of them. The index a looping
//!   column reports is taken modulo its length for the same reason -- upstream
//!   passes the raw index straight to `onSelectedItemChanged`, which is only
//!   in range because its wheel starts at turn zero.
//! * **`minimumDate`/`maximumDate` grey out but do not push back.** Upstream
//!   also animates the offending column back into range when scrolling stops
//!   (`_pickerDidStopScrolling`); the day column's clamp for a short month is
//!   carried, the rest is not.
//! * **No text scaling and no right-to-left.** Upstream wraps both pickers in
//!   `MediaQuery.withNoTextScaling` and threads a `textDirectionFactor`
//!   through every layout; this tier compiles one locale, left to right.

use std::rc::Rc;

use crate::animation::Curve;
use crate::cupertino::{
    CupertinoColors, CupertinoPicker, CupertinoPickerDefaultSelectionOverlay,
    FixedExtentScrollController, cupertino_theme_of,
};
use crate::cupertino_app::CupertinoLocalizationEn as L10n;
use crate::cupertino_app::{DatePickerDateOrder, DatePickerDateTimeOrder};
use crate::engine::{Color, TextStyle};
use crate::framework::{
    AnyWidget, BuildContext, Key, StateHandle, StatefulComponent, leaf, many, stateful,
};
use crate::pickers::{Date, add_days_to_date, days_in_month};
use crate::render::{
    Alignment, BoxConstraints, BoxedRender, EdgeInsets, MultiChildLayoutContext,
    MultiChildLayoutDelegate, Offset, RenderConstrainedBox, RenderCustomMultiChildLayoutBox,
    RenderRef, Size,
};
use crate::widgets::{Align, Container, Text};

// -- The measurements both pickers are built from ------------------------------
//
// Anchor: the constants at the head of `cupertino/date_picker.dart`, each
// "derived from https://developer.apple.com/design/resources/ and on iOS
// simulators with Debug View Hierarchy".

/// `_kItemExtent`.
pub const DATE_PICKER_ITEM_EXTENT: f32 = 32.0;
/// `_kPickerWidth`, from the picker's intrinsic content size constraint.
pub const DATE_PICKER_WIDTH: f32 = 320.0;
/// `_kPickerHeight`.
pub const DATE_PICKER_HEIGHT: f32 = 216.0;
/// `_kUseMagnifier`.
pub const DATE_PICKER_USE_MAGNIFIER: bool = true;
/// `_kMagnification`, written as upstream writes it.
pub const DATE_PICKER_MAGNIFICATION: f32 = 2.35 / 2.1;
/// `_kDatePickerPadSize`: the padding on both sides of every column.
pub const DATE_PICKER_PAD_SIZE: f32 = 12.0;
/// `_kSqueeze`. "The density of a date picker is different from a generic
/// picker. Eyeballed from iOS."
pub const DATE_PICKER_SQUEEZE: f32 = 1.25;
/// `_CupertinoDatePickerDateTimeState._kMaximumOffAxisFraction`: how far the
/// farthest column's vanishing point is pushed, as a fraction of its width.
const MAXIMUM_OFF_AXIS_FRACTION: f32 = 0.45;
/// `_animateColumnControllerToItem`'s duration and curve.
const COLUMN_ANIMATION_MICROS: i64 = 200_000;
/// The AM/PM column's own, from `_buildHourPicker`: "Animation values obtained
/// by comparing with iOS version."
const MERIDIEM_ANIMATION_MICROS: i64 = 300_000;

/// `_kTimerPickerMagnification`, `34 / 32`: the item is 32 high and the
/// magnifier 34.
pub const TIMER_PICKER_MAGNIFICATION: f32 = 34.0 / 32.0;
/// `_kTimerPickerMinHorizontalPadding`.
pub const TIMER_PICKER_MIN_HORIZONTAL_PADDING: f32 = 30.0;
/// `_kTimerPickerHalfColumnPadding`.
pub const TIMER_PICKER_HALF_COLUMN_PADDING: f32 = 4.0;
/// `_kTimerPickerLabelPadSize`: between a number and its unit.
pub const TIMER_PICKER_LABEL_PAD_SIZE: f32 = 6.0;
/// `_kTimerPickerLabelFontSize`.
pub const TIMER_PICKER_LABEL_FONT_SIZE: f32 = 17.0;
/// `_kTimerPickerColumnIntrinsicWidth`.
pub const TIMER_PICKER_COLUMN_INTRINSIC_WIDTH: f32 = 106.0;

/// `CupertinoTextThemeData.dateTimePickerTextStyle`, which is what
/// `_themeTextStyle` hands every item.
///
/// It is declared `inherit: false` upstream, and that is the whole reason
/// `date_picker.dart`'s own `_kDefaultPickerTextStyle` (letter spacing -0.83)
/// does not reach the items: a `Text` whose style does not inherit takes no
/// part in the ambient `DefaultTextStyle.merge` above it. The -0.83 style is
/// for what the picker draws *without* a style of its own.
pub fn date_time_picker_text_style(color: Color) -> TextStyle {
    TextStyle {
        font_size: 21.0,
        letter_spacing: Some(0.4),
        color,
        ..TextStyle::default()
    }
}

/// `CupertinoTextThemeData.pickerTextStyle`, the timer picker's.
pub fn picker_text_style(color: Color, magnification: f32) -> TextStyle {
    TextStyle {
        font_size: 21.0 * magnification,
        letter_spacing: Some(-0.6),
        color,
        ..TextStyle::default()
    }
}

/// The widest of `texts` in `style`. Upstream's
/// `CupertinoDatePicker.getColumnWidth`, which is `TextPainter.
/// computeMaxIntrinsicWidth` over each and the maximum of those.
pub fn column_width(texts: &[String], style: &TextStyle) -> f32 {
    texts
        .iter()
        .map(|text| {
            crate::painting::shape(text, style, None, false, f32::MAX / 4.0, 1.0)
                .max_intrinsic_width()
        })
        .fold(0.0_f32, f32::max)
}

// -- Where the columns go ------------------------------------------------------

/// Upstream `_DatePickerLayoutDelegate`: every column padded by
/// [`DATE_PICKER_PAD_SIZE`] on both sides, the group centred, and whatever
/// width is left over split between the first and last column so the wheels
/// reach the edges rather than bending away from them.
///
/// Left to right only; upstream's `textDirectionFactor` reverses the walk.
pub struct DatePickerLayout {
    pub column_widths: Vec<f32>,
    /// Upstream's `maxWidth`: "the max width the children should reach to
    /// avoid bending outwards".
    pub max_width: f32,
}

impl MultiChildLayoutDelegate for DatePickerLayout {
    fn perform_layout(&self, size: Size, context: &mut MultiChildLayoutContext) {
        let mut remaining_width = self.max_width.min(size.width);
        let mut current_horizontal_offset = (size.width - remaining_width) / 2.0;
        for width in &self.column_widths {
            remaining_width -= width + DATE_PICKER_PAD_SIZE * 2.0;
        }
        let last = self.column_widths.len().saturating_sub(1);
        for (index, width) in self.column_widths.iter().enumerate() {
            let mut child_width = width + DATE_PICKER_PAD_SIZE * 2.0;
            if index == 0 || index == last {
                child_width += remaining_width / 2.0;
            }
            context.layout_child(
                index as u64,
                BoxConstraints::tight(child_width.max(0.0), size.height),
            );
            context.position_child(index as u64, Offset::new(current_horizontal_offset, 0.0));
            current_horizontal_offset += child_width;
        }
    }

    fn should_relayout(&self, old: &dyn MultiChildLayoutDelegate) -> bool {
        match old.as_any().downcast_ref::<DatePickerLayout>() {
            Some(old) => old.column_widths != self.column_widths || old.max_width != self.max_width,
            None => true,
        }
    }

    fn kind_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<DatePickerLayout>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Upstream `CupertinoDatePickerMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CupertinoDatePickerMode {
    Time,
    Date,
    #[default]
    DateAndTime,
    MonthYear,
}

/// Why a date picker's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatePickerError {
    NonPositiveItemExtent,
    MinuteIntervalNotAFactorOfSixty,
    InitialMinuteNotOnAStop,
    InitialBeforeMinimum,
    InitialAfterMaximum,
    DayOfWeekOutsideDateMode,
    TimeSeparatorOutsideTimeModes,
    ModeChangedAfterBuild,
}

/// A moment, as a picker passes one around: upstream's `DateTime`, reduced to
/// the fields a date picker can show.
///
/// [`Date`] alone is not enough -- `dateAndTime` and `time` mode both carry a
/// time of day -- and the crate has no `DateTime`, so this is it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PickerDateTime {
    /// Ordered first so that the derived comparison is chronological.
    pub date: Date,
    pub hour: u32,
    pub minute: u32,
}

impl PickerDateTime {
    pub fn new(date: Date, hour: u32, minute: u32) -> PickerDateTime {
        PickerDateTime { date, hour, minute }
    }

    /// Midnight on `date`.
    pub fn at_midnight(date: Date) -> PickerDateTime {
        PickerDateTime::new(date, 0, 0)
    }

    /// Minutes since 1970-01-01, for comparisons and for the ranges upstream
    /// states as `DateTime`s.
    pub fn to_minutes(self) -> i64 {
        self.date.to_days() * 24 * 60 + self.hour as i64 * 60 + self.minute as i64
    }
}

/// Upstream `CupertinoDatePicker`.
///
/// The rules -- [`CupertinoDatePicker::validate`] and its neighbours -- are
/// upstream's constructor asserts; the [`StatefulComponent`] implementation
/// below is the widget. See the module docs for what the widget does not
/// carry.
#[derive(Clone)]
pub struct CupertinoDatePicker {
    pub mode: CupertinoDatePickerMode,
    pub item_extent: f32,
    pub minute_interval: u32,
    pub show_day_of_week: bool,
    pub show_time_separator: bool,
    /// Upstream's `initialDateTime`.
    pub initial: PickerDateTime,
    pub minimum: Option<PickerDateTime>,
    pub maximum: Option<PickerDateTime>,
    /// Upstream's `use24hFormat`, which decides whether there is an AM/PM
    /// column at all.
    pub use_24h_format: bool,
    /// Upstream's `dateOrder`; `None` defers to the localisation's, which for
    /// this crate's one locale is `mdy`.
    pub date_order: Option<DatePickerDateOrder>,
    /// Upstream's `minimumYear`, whose default is 1.
    pub minimum_year: i32,
    /// Upstream's `maximumYear`, whose default is `null`.
    pub maximum_year: Option<i32>,
    /// The hit-test identity of the first column; the rest count up from it.
    id: u64,
    background_color: Option<Color>,
    on_changed: Option<Rc<dyn Fn(PickerDateTime)>>,
}

impl PartialEq for CupertinoDatePicker {
    /// Everything but the callback, which cannot be compared and is not part
    /// of what the picker *is*.
    fn eq(&self, other: &CupertinoDatePicker) -> bool {
        self.mode == other.mode
            && self.item_extent == other.item_extent
            && self.minute_interval == other.minute_interval
            && self.show_day_of_week == other.show_day_of_week
            && self.show_time_separator == other.show_time_separator
            && self.initial == other.initial
            && self.minimum == other.minimum
            && self.maximum == other.maximum
            && self.use_24h_format == other.use_24h_format
            && self.date_order == other.date_order
            && self.minimum_year == other.minimum_year
            && self.maximum_year == other.maximum_year
            && self.id == other.id
            && self.background_color == other.background_color
    }
}

impl std::fmt::Debug for CupertinoDatePicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoDatePicker")
            .field("mode", &self.mode)
            .field("initial", &self.initial)
            .finish_non_exhaustive()
    }
}

impl CupertinoDatePicker {
    pub fn new(mode: CupertinoDatePickerMode) -> CupertinoDatePicker {
        CupertinoDatePicker {
            mode,
            item_extent: DATE_PICKER_ITEM_EXTENT,
            minute_interval: 1,
            show_day_of_week: false,
            show_time_separator: false,
            initial: PickerDateTime::at_midnight(Date {
                year: 1970,
                month: 1,
                day: 1,
            }),
            minimum: None,
            maximum: None,
            use_24h_format: false,
            date_order: None,
            minimum_year: 1,
            maximum_year: None,
            id: 0,
            background_color: None,
            on_changed: None,
        }
    }

    /// The hit-test identity of the picker's first column. Each column takes
    /// the next id up, so a caller has to leave room for as many columns as
    /// the mode has (four is the most any mode builds).
    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    /// Upstream's `initialDateTime`.
    pub fn with_initial(mut self, initial: PickerDateTime) -> Self {
        self.initial = initial;
        self
    }

    /// Upstream's `backgroundColor`.
    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Upstream's `use24hFormat`.
    pub fn with_24h_format(mut self, on: bool) -> Self {
        self.use_24h_format = on;
        self
    }

    /// Upstream's `onDateTimeChanged`, which fires on every change of the
    /// selected row and not only when the wheels settle.
    pub fn with_on_changed(mut self, on_changed: impl Fn(PickerDateTime) + 'static) -> Self {
        self.on_changed = Some(Rc::new(on_changed));
        self
    }

    /// Upstream's `dateOrder`.
    pub fn with_date_order(mut self, order: DatePickerDateOrder) -> Self {
        self.date_order = Some(order);
        self
    }

    /// The years the year column offers. Upstream's `minimumYear` (1) and
    /// `maximumYear` (`null`, meaning no end); the open end is closed here at
    /// [`CupertinoDatePicker::LAST_YEAR`] because this crate's wheel is a
    /// finite list.
    pub fn with_year_range(mut self, minimum: i32, maximum: Option<i32>) -> Self {
        self.minimum_year = minimum;
        self.maximum_year = maximum;
        self
    }

    /// Where the year column stops when `maximumYear` is `null`. Upstream has
    /// no such limit; a wheel with an end has to have one somewhere, and the
    /// calendar this crate's [`Date`] arithmetic is proleptic Gregorian over
    /// runs well past it either way.
    pub const LAST_YEAR: i32 = 9999;

    fn last_year(&self) -> i32 {
        self.maximum_year.unwrap_or(CupertinoDatePicker::LAST_YEAR)
    }

    /// Upstream's twelve constructor asserts. Three things about them.
    ///
    /// **The minute interval must divide sixty**, not merely be positive:
    /// `assert(minuteInterval > 0 && 60 % minuteInterval == 0, 'minute interval
    /// is not a positive integer factor of 60')`. Because the wheel wraps. An
    /// interval of seven would run 0, 7, ... 56 and then meet 0 again four
    /// minutes later, so the one gap that is not an interval is the one you
    /// cannot see coming. **A wheel has no ends, so its step has to close the
    /// loop.**
    ///
    /// And the initial minute must land *on* a stop:
    /// `assert(initialDateTime.minute % minuteInterval == 0)`. The two together
    /// say you cannot start a wheel between two of its positions, because it has
    /// no in-between to start at.
    ///
    /// **Six of the twelve are mode-guarded, each written `mode != X || <check>`**
    /// -- an implication per mode, inline. Tick 88's `PaginatedDataTable` wrote
    /// the same idea as a nested assert inside an `assert(() { ... }())` closure.
    /// Two encodings of "this rule applies only in this situation", one per file.
    pub fn validate(&self) -> Result<(), DatePickerError> {
        if self.item_extent <= 0.0 {
            return Err(DatePickerError::NonPositiveItemExtent);
        }
        if self.minute_interval == 0 || 60 % self.minute_interval != 0 {
            return Err(DatePickerError::MinuteIntervalNotAFactorOfSixty);
        }
        if self.initial.minute % self.minute_interval != 0 {
            return Err(DatePickerError::InitialMinuteNotOnAStop);
        }
        if self.minimum.is_some_and(|min| self.initial < min) {
            return Err(DatePickerError::InitialBeforeMinimum);
        }
        if self.maximum.is_some_and(|max| self.initial > max) {
            return Err(DatePickerError::InitialAfterMaximum);
        }
        // `assert((mode == date) || !showDayOfWeek, 'showDayOfWeek is only
        // supported in date mode')`.
        if self.show_day_of_week && self.mode != CupertinoDatePickerMode::Date {
            return Err(DatePickerError::DayOfWeekOutsideDateMode);
        }
        // And the same shape for the separator, admitting two modes.
        if self.show_time_separator
            && !matches!(
                self.mode,
                CupertinoDatePickerMode::Time | CupertinoDatePickerMode::DateAndTime
            )
        {
            return Err(DatePickerError::TimeSeparatorOutsideTimeModes);
        }
        Ok(())
    }

    /// Upstream's `didUpdateWidget`:
    /// `assert(oldWidget.mode == widget.mode, "The $runtimeType's mode cannot
    /// change once it's built.")`.
    ///
    /// **A constructor argument that is part of the widget's identity** -- the
    /// second in this sweep, after the stepper's step list in tick 85, and for
    /// the same kind of reason: each mode builds a different set of wheels, and
    /// the scroll controllers behind them are made once.
    pub fn accepts_update(&self, updated: &CupertinoDatePicker) -> Result<(), DatePickerError> {
        if self.mode != updated.mode {
            return Err(DatePickerError::ModeChangedAfterBuild);
        }
        Ok(())
    }

    /// The stops a minute wheel offers, which is why the interval must divide
    /// sixty.
    pub fn minute_stops(&self) -> Vec<u32> {
        (0..60).step_by(self.minute_interval as usize).collect()
    }

    /// Whether every step between consecutive stops, **including the one that
    /// wraps past 60 back to 0**, is the same size.
    pub fn wheel_closes_evenly(&self) -> bool {
        let stops = self.minute_stops();
        stops
            .windows(2)
            .all(|pair| pair[1] - pair[0] == self.minute_interval)
            && 60 - stops.last().copied().unwrap_or(0) == self.minute_interval
    }

    /// The width of one column, from the longest string it can ever show.
    /// Upstream's `CupertinoDatePicker._getColumnWidth`.
    fn width_of(&self, column: PickerColumnType, style: &TextStyle) -> f32 {
        let texts: Vec<String> = match column {
            // Upstream measures `datePickerMediumDate(DateTime(2018, i, 25))`
            // for each month, so the weekday is **the real one for that
            // date** rather than a fixed one -- which is what makes the
            // measurement cover every weekday abbreviation the column can
            // show.
            PickerColumnType::Date => (1..=12)
                .map(|month| {
                    let date = Date {
                        year: 2018,
                        month: month as u32,
                        day: 25,
                    };
                    L10n::date_picker_medium_date(date.weekday(), month, date.day)
                })
                .collect(),
            PickerColumnType::Hour => (0..24).map(L10n::date_picker_hour).collect(),
            PickerColumnType::Minute => (0..60).map(L10n::date_picker_minute).collect(),
            PickerColumnType::DayPeriod => vec![
                L10n::ANTE_MERIDIEM_ABBREVIATION.to_string(),
                L10n::POST_MERIDIEM_ABBREVIATION.to_string(),
            ],
            PickerColumnType::DayOfMonth => {
                let mut texts: Vec<String> = (1..=31)
                    .map(|day| L10n::date_picker_day_of_month(day, None))
                    .collect();
                if self.show_day_of_week {
                    // The longest day with every weekday it could fall on --
                    // upstream walks `wd` from 1 to 6, not 7, which is a
                    // one-off in its own loop bound and is carried as written.
                    for week_day in 1..7 {
                        texts.push(L10n::date_picker_day_of_month(31, Some(week_day)));
                    }
                }
                texts
            }
            PickerColumnType::Month => (1..=12)
                .map(|month| L10n::date_picker_month(month).to_string())
                .collect(),
            PickerColumnType::Year => vec![L10n::date_picker_year(2018)],
            PickerColumnType::TimeSeparator => vec![":".to_string()],
        };
        column_width(&texts, style)
    }

    /// The columns of `date` mode, in the order the localisation asks for.
    fn date_columns(&self) -> Vec<PickerColumnType> {
        match self.date_order.unwrap_or_else(L10n::date_picker_date_order) {
            DatePickerDateOrder::Mdy => vec![
                PickerColumnType::Month,
                PickerColumnType::DayOfMonth,
                PickerColumnType::Year,
            ],
            DatePickerDateOrder::Dmy => vec![
                PickerColumnType::DayOfMonth,
                PickerColumnType::Month,
                PickerColumnType::Year,
            ],
            DatePickerDateOrder::Ymd => vec![
                PickerColumnType::Year,
                PickerColumnType::Month,
                PickerColumnType::DayOfMonth,
            ],
            DatePickerDateOrder::Ydm => vec![
                PickerColumnType::Year,
                PickerColumnType::DayOfMonth,
                PickerColumnType::Month,
            ],
        }
    }

    /// The columns of `time` and `dateAndTime` mode, in the order the
    /// localisation asks for. Upstream's `_CupertinoDatePickerDateTimeState.
    /// build`, whose inserts and appends this reproduces in the same order.
    fn time_columns(&self) -> Vec<PickerColumnType> {
        let mut columns = vec![PickerColumnType::Hour, PickerColumnType::Minute];
        if self.show_time_separator {
            columns.insert(1, PickerColumnType::TimeSeparator);
        }
        let order = L10n::date_picker_date_time_order();
        if !self.use_24h_format {
            match order {
                DatePickerDateTimeOrder::DateTimeDayPeriod
                | DatePickerDateTimeOrder::TimeDayPeriodDate => {
                    columns.push(PickerColumnType::DayPeriod)
                }
                DatePickerDateTimeOrder::DateDayPeriodTime
                | DatePickerDateTimeOrder::DayPeriodTimeDate => {
                    columns.insert(0, PickerColumnType::DayPeriod)
                }
            }
        }
        if self.mode == CupertinoDatePickerMode::DateAndTime {
            match order {
                DatePickerDateTimeOrder::TimeDayPeriodDate
                | DatePickerDateTimeOrder::DayPeriodTimeDate => {
                    columns.push(PickerColumnType::Date)
                }
                DatePickerDateTimeOrder::DateTimeDayPeriod
                | DatePickerDateTimeOrder::DateDayPeriodTime => {
                    columns.insert(0, PickerColumnType::Date)
                }
            }
        }
        columns
    }
}

/// Upstream `_PickerColumnType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerColumnType {
    DayOfMonth,
    Month,
    Year,
    /// The medium-date column of `dateAndTime` mode ("Fri Aug 31").
    Date,
    Hour,
    Minute,
    DayPeriod,
    TimeSeparator,
}

/// How far up and down the `dateAndTime` date column runs.
///
/// **Upstream's has no ends at all**: its `dateController` starts at item 0
/// and the builder answers for every index, negative ones included, so the
/// column runs from the initial date to either end of the calendar. This
/// crate's wheel is a finite list, so the column is a window centred on the
/// initial date -- about twenty-seven years wide, which no reader will reach
/// by dragging.
const DATE_COLUMN_RADIUS_DAYS: i32 = 5000;

/// What a live [`CupertinoDatePicker`] remembers.
///
/// Upstream splits this across three `State` classes, one per mode. Only one
/// of them is ever live -- the mode cannot change once built (see
/// [`CupertinoDatePicker::accepts_update`]) -- so they are one struct here.
pub struct CupertinoDatePickerState {
    initial: PickerDateTime,
    /// `_CupertinoDatePickerDateState`'s three.
    selected_day: u32,
    selected_month: u32,
    selected_year: i32,
    day_controller: FixedExtentScrollController,
    month_controller: FixedExtentScrollController,
    year_controller: FixedExtentScrollController,
    /// `_CupertinoDatePickerDateTimeState`'s.
    selected_day_from_initial: i32,
    /// The hour column's own item index, 0..24 -- not the hour it shows. The
    /// two differ by twelve while the meridiem region is flipped.
    selected_hour_index: usize,
    selected_minute_index: usize,
    /// 0 is AM, 1 is PM.
    selected_am_pm: usize,
    /// Which twelve-hour half of the hour column the selection is physically
    /// in. Upstream keeps this apart from `selected_am_pm` because the AM/PM
    /// column can be scrolled by the hour column *animatedly*, and the meaning
    /// has to change the moment the hour does rather than when the animation
    /// lands.
    meridiem_region: usize,
    date_controller: FixedExtentScrollController,
    hour_controller: FixedExtentScrollController,
    minute_controller: FixedExtentScrollController,
    meridiem_controller: FixedExtentScrollController,
}

impl Default for CupertinoDatePickerState {
    fn default() -> CupertinoDatePickerState {
        CupertinoDatePickerState {
            initial: PickerDateTime::at_midnight(Date {
                year: 1970,
                month: 1,
                day: 1,
            }),
            selected_day: 1,
            selected_month: 1,
            selected_year: 1970,
            day_controller: FixedExtentScrollController::default(),
            month_controller: FixedExtentScrollController::default(),
            year_controller: FixedExtentScrollController::default(),
            selected_day_from_initial: 0,
            selected_hour_index: 0,
            selected_minute_index: 0,
            selected_am_pm: 0,
            meridiem_region: 0,
            date_controller: FixedExtentScrollController::default(),
            hour_controller: FixedExtentScrollController::default(),
            minute_controller: FixedExtentScrollController::default(),
            meridiem_controller: FixedExtentScrollController::default(),
        }
    }
}

impl CupertinoDatePickerState {
    /// Upstream's `_isHourRegionFlipped`.
    fn is_hour_region_flipped(&self) -> bool {
        self.selected_am_pm != self.meridiem_region
    }

    /// Upstream's `_selectedHour(selectedAmPm, selectedHour)`.
    fn hour_of(&self, hour_index: usize) -> u32 {
        if self.is_hour_region_flipped() {
            ((hour_index + 12) % 24) as u32
        } else {
            hour_index as u32
        }
    }

    /// Upstream's `selectedDateTime` in the two time modes.
    fn selected_date_time(&self, minute_interval: u32) -> PickerDateTime {
        PickerDateTime::new(
            add_days_to_date(self.initial.date, self.selected_day_from_initial),
            self.hour_of(self.selected_hour_index),
            self.selected_minute_index as u32 * minute_interval % 60,
        )
    }

    /// The date the three date-mode columns spell out.
    fn selected_date(&self) -> Date {
        Date {
            year: self.selected_year,
            month: self.selected_month,
            day: self.selected_day,
        }
    }
}

/// Upstream's `itemPositioningBuilder`, which is written out twice -- once in
/// each of the two `build`s -- and differs between them.
///
/// Date mode gives every item a `SizedBox` of the column's own width and aligns
/// twice, so the strings line up on the edge that faces the neighbouring
/// column. The two time modes constrain only the outer columns and align once.
fn position_item(
    child: BoxedRender,
    first: bool,
    last: bool,
    width: f32,
    date_mode: bool,
) -> BoxedRender {
    let outer = if last {
        Alignment::CENTER_LEFT
    } else {
        Alignment::CENTER_RIGHT
    };
    if date_mode {
        let inner = if first {
            Alignment::CENTER_LEFT
        } else {
            Alignment::CENTER_RIGHT
        };
        let padding = if first {
            EdgeInsets::ZERO
        } else {
            EdgeInsets::only(0.0, 0.0, DATE_PICKER_PAD_SIZE, 0.0)
        };
        return RenderRef::new(
            Container::new()
                .with_padding(padding)
                .with_child(Align::new(
                    outer,
                    RenderConstrainedBox::new(BoxConstraints::new(
                        width + DATE_PICKER_PAD_SIZE,
                        width + DATE_PICKER_PAD_SIZE,
                        0.0,
                        f32::INFINITY,
                    ))
                    .with_child(Align::new(inner, child)),
                )),
        );
    }
    let padding = if last {
        EdgeInsets::only(DATE_PICKER_PAD_SIZE, 0.0, 0.0, 0.0)
    } else {
        EdgeInsets::only(0.0, 0.0, DATE_PICKER_PAD_SIZE, 0.0)
    };
    let aligned: BoxedRender = if first || last {
        RenderRef::new(
            RenderConstrainedBox::new(BoxConstraints::new(
                0.0,
                width + DATE_PICKER_PAD_SIZE,
                0.0,
                f32::INFINITY,
            ))
            .with_child(child),
        )
    } else {
        child
    };
    RenderRef::new(
        Container::new()
            .with_padding(padding)
            .with_child(Align::new(outer, aligned)),
    )
}

impl StatefulComponent for CupertinoDatePicker {
    type State = CupertinoDatePickerState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    /// Upstream's two `initState`s. "Read this out when the state is initially
    /// created. Changes in initialDateTime in the widget after first build is
    /// ignored."
    fn initial_state(&self) -> CupertinoDatePickerState {
        let initial = self.initial;
        // "Initially each of the physical regions is mapped to the meridiem
        // region with the same number."
        let am_pm = (initial.hour / 12) as usize;
        let minute_item = (initial.minute / self.minute_interval.max(1)) as usize;
        CupertinoDatePickerState {
            initial,
            selected_day: initial.date.day,
            selected_month: initial.date.month,
            selected_year: initial.date.year,
            day_controller: FixedExtentScrollController::new(initial.date.day as usize - 1),
            month_controller: FixedExtentScrollController::new(initial.date.month as usize - 1),
            year_controller: FixedExtentScrollController::new(
                (initial.date.year - self.minimum_year).max(0) as usize,
            ),
            selected_day_from_initial: 0,
            selected_hour_index: initial.hour as usize,
            selected_minute_index: minute_item,
            selected_am_pm: am_pm,
            meridiem_region: am_pm,
            date_controller: FixedExtentScrollController::new(DATE_COLUMN_RADIUS_DAYS as usize),
            hour_controller: FixedExtentScrollController::new(initial.hour as usize),
            minute_controller: FixedExtentScrollController::new(minute_item),
            meridiem_controller: FixedExtentScrollController::new(am_pm),
        }
    }

    fn build(
        &self,
        state: &CupertinoDatePickerState,
        handle: StateHandle<CupertinoDatePickerState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let valid = date_time_picker_text_style(theme.resolve(CupertinoColors::LABEL));
        let invalid = date_time_picker_text_style(theme.resolve(CupertinoColors::INACTIVE_GRAY));

        let date_mode = matches!(
            self.mode,
            CupertinoDatePickerMode::Date | CupertinoDatePickerMode::MonthYear
        );
        let columns = if date_mode {
            self.date_columns()
        } else {
            self.time_columns()
        };
        let widths: Vec<f32> = columns
            .iter()
            .map(|column| self.width_of(*column, &valid))
            .collect();

        // Upstream's `totalColumnWidths`, which starts at four pad sizes --
        // two more than the columns account for, so the outermost wheels have
        // somewhere to bend into.
        let total: f32 = 4.0 * DATE_PICKER_PAD_SIZE
            + widths
                .iter()
                .map(|width| width + 2.0 * DATE_PICKER_PAD_SIZE)
                .sum::<f32>();
        let max_width = total.max(DATE_PICKER_WIDTH);

        let count = columns.len();
        let children: Vec<AnyWidget> = columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let first = index == 0;
                // The two modes push their axes apart differently: date mode
                // spreads them evenly about the middle column, the time modes
                // push only the outermost ones.
                let off_axis = if date_mode {
                    (index as f32 - 1.0) * 0.3
                } else if first {
                    -MAXIMUM_OFF_AXIS_FRACTION
                } else if index >= 2 || count == 2 {
                    MAXIMUM_OFF_AXIS_FRACTION
                } else {
                    0.0
                };
                self.build_column(
                    *column,
                    index,
                    count,
                    widths[index],
                    off_axis,
                    date_mode,
                    state,
                    handle.clone(),
                    &valid,
                    &invalid,
                )
            })
            .collect();

        let layout = Rc::new(DatePickerLayout {
            column_widths: widths,
            max_width,
        });
        many(children, move |rendered| {
            RenderCustomMultiChildLayoutBox::new(
                Rc::clone(&layout) as Rc<dyn MultiChildLayoutDelegate>,
                rendered
                    .into_iter()
                    .enumerate()
                    .map(|(index, child)| (index as u64, child))
                    .collect(),
            )
        })
    }
}

impl CupertinoDatePicker {
    /// One column: upstream's `_buildMonthPicker` and its seven siblings, whose
    /// bodies differ only in the list they generate, what they write on a
    /// change, and which controller they hold.
    #[allow(clippy::too_many_arguments)]
    fn build_column(
        &self,
        column: PickerColumnType,
        index: usize,
        columns: usize,
        width: f32,
        off_axis: f32,
        date_mode: bool,
        state: &CupertinoDatePickerState,
        handle: StateHandle<CupertinoDatePickerState>,
        valid: &TextStyle,
        invalid: &TextStyle,
    ) -> AnyWidget {
        let first = index == 0;
        let last = index + 1 == columns;
        let overlay = CupertinoPickerDefaultSelectionOverlay::for_column(index, columns);
        let id = self.id + 1 + index as u64;

        // The separator is not a wheel at all: upstream's
        // `_buildTimeSeparatorWidget` is a plain centred `Text`.
        if column == PickerColumnType::TimeSeparator {
            let style = valid.clone();
            return leaf(move || {
                position_item(
                    RenderRef::new(Text::new(":").with_style(style.clone())),
                    first,
                    last,
                    width,
                    date_mode,
                )
            });
        }

        let (count, item, on_selected) = self.column_wiring(column, state, handle);
        let valid = valid.clone();
        let invalid = invalid.clone();
        let build_item = move |i: usize| {
            let (text, is_valid) = item(i);
            let style = if is_valid {
                valid.clone()
            } else {
                invalid.clone()
            };
            leaf(move || {
                position_item(
                    RenderRef::new(
                        Text::new(text.clone())
                            .with_style(style.clone())
                            .with_soft_wrap(false)
                            .with_max_lines(1),
                    ),
                    first,
                    last,
                    width,
                    date_mode,
                )
            })
        };

        // Upstream's `looping: true`, which four of the eight columns carry:
        // the day, the month, the hour and the minute have no ends, and the
        // year, the medium date and AM/PM do.
        let looping = matches!(
            column,
            PickerColumnType::DayOfMonth
                | PickerColumnType::Month
                | PickerColumnType::Hour
                | PickerColumnType::Minute
        );
        let mut picker = CupertinoPicker::new(id, self.item_extent, count, build_item)
            .with_off_axis_fraction(off_axis)
            .with_magnifier(DATE_PICKER_USE_MAGNIFIER)
            .with_magnification(DATE_PICKER_MAGNIFICATION)
            .with_squeeze(DATE_PICKER_SQUEEZE)
            .with_looping(looping)
            .with_selection_overlay(Some(overlay))
            .with_scroll_controller(self.controller_for(column, state))
            .with_on_selected(on_selected);
        if let Some(background) = self.background_color {
            picker = picker.with_background_color(background);
        }
        stateful(picker)
    }

    /// Which controller a column is driven by.
    fn controller_for(
        &self,
        column: PickerColumnType,
        state: &CupertinoDatePickerState,
    ) -> FixedExtentScrollController {
        match column {
            PickerColumnType::DayOfMonth => state.day_controller.clone(),
            PickerColumnType::Month => state.month_controller.clone(),
            PickerColumnType::Year => state.year_controller.clone(),
            PickerColumnType::Date => state.date_controller.clone(),
            PickerColumnType::Hour => state.hour_controller.clone(),
            PickerColumnType::Minute => state.minute_controller.clone(),
            PickerColumnType::DayPeriod => state.meridiem_controller.clone(),
            PickerColumnType::TimeSeparator => FixedExtentScrollController::default(),
        }
    }

    /// A column's length, its items, and what a change to it writes.
    #[allow(clippy::type_complexity)]
    fn column_wiring(
        &self,
        column: PickerColumnType,
        state: &CupertinoDatePickerState,
        handle: StateHandle<CupertinoDatePickerState>,
    ) -> (
        usize,
        Box<dyn Fn(usize) -> (String, bool)>,
        Box<dyn Fn(usize)>,
    ) {
        let minute_interval = self.minute_interval.max(1);
        let on_changed = self.on_changed.clone();
        let minimum = self.minimum;
        let maximum = self.maximum;
        let show_day_of_week = self.show_day_of_week;
        let use_24h = self.use_24h_format;
        let minimum_year = self.minimum_year;
        let last_year = self.last_year();

        // What a change reports, in each family of modes. Upstream's
        // `_onSelectedItemChange` (time modes) and the `_isCurrentDateValid`
        // guard the date-mode columns each repeat.
        let report_time = {
            let on_changed = on_changed.clone();
            move |state: &CupertinoDatePickerState| {
                let Some(on_changed) = &on_changed else {
                    return;
                };
                let now = state.selected_date_time(minute_interval);
                if minimum.is_some_and(|min| min > now) || maximum.is_some_and(|max| max < now) {
                    return;
                }
                on_changed(now);
            }
        };
        let report_date = {
            let on_changed = on_changed.clone();
            move |state: &CupertinoDatePickerState| {
                let Some(on_changed) = &on_changed else {
                    return;
                };
                let date = state.selected_date();
                // Upstream's `_isCurrentDateValid`, whose last clause --
                // `minSelectedDate.day == selectedDay` -- is how it asks
                // "does this day exist in this month", `DateTime` having
                // rolled February 30th over into March.
                if date.day > days_in_month(date.year, date.month) {
                    return;
                }
                let now = PickerDateTime::at_midnight(date);
                if minimum.is_some_and(|min| PickerDateTime::at_midnight(min.date) > now)
                    || maximum.is_some_and(|max| PickerDateTime::at_midnight(max.date) < now)
                {
                    return;
                }
                on_changed(now);
            }
        };

        match column {
            PickerColumnType::Month => {
                let year = state.selected_year;
                let day_controller = state.day_controller.clone();
                (
                    12,
                    Box::new(move |index| {
                        let month = index as u32 + 1;
                        let is_invalid = minimum
                            .is_some_and(|min| min.date.year == year && min.date.month > month)
                            || maximum
                                .is_some_and(|max| max.date.year == year && max.date.month < month);
                        (
                            L10n::date_picker_month(month as usize).to_string(),
                            !is_invalid,
                        )
                    }),
                    Box::new(move |index| {
                        let day_controller = day_controller.clone();
                        let report_date = report_date.clone();
                        handle.set_state(move |state| {
                            state.selected_month = index as u32 + 1;
                            clamp_day_column(state, &day_controller);
                            report_date(state);
                        });
                    }),
                )
            }
            PickerColumnType::DayOfMonth => {
                let year = state.selected_year;
                let month = state.selected_month;
                let days_in_current_month = days_in_month(year, month);
                (
                    31,
                    Box::new(move |index| {
                        let day = index as u32 + 1;
                        let week_day =
                            show_day_of_week.then(|| Date { year, month, day }.weekday());
                        let is_invalid = day > days_in_current_month
                            || minimum.is_some_and(|min| {
                                min.date.year == year
                                    && min.date.month == month
                                    && min.date.day > day
                            })
                            || maximum.is_some_and(|max| {
                                max.date.year == year
                                    && max.date.month == month
                                    && max.date.day < day
                            });
                        (L10n::date_picker_day_of_month(day, week_day), !is_invalid)
                    }),
                    Box::new(move |index| {
                        let report_date = report_date.clone();
                        handle.set_state(move |state| {
                            state.selected_day = index as u32 + 1;
                            report_date(state);
                        });
                    }),
                )
            }
            PickerColumnType::Year => {
                let day_controller = state.day_controller.clone();
                (
                    (last_year - minimum_year + 1).max(1) as usize,
                    Box::new(move |index| {
                        let year = minimum_year + index as i32;
                        let is_valid = minimum.is_none_or(|min| min.date.year <= year)
                            && maximum.is_none_or(|max| max.date.year >= year);
                        (L10n::date_picker_year(year), is_valid)
                    }),
                    Box::new(move |index| {
                        let day_controller = day_controller.clone();
                        let report_date = report_date.clone();
                        handle.set_state(move |state| {
                            state.selected_year = minimum_year + index as i32;
                            clamp_day_column(state, &day_controller);
                            report_date(state);
                        });
                    }),
                )
            }
            PickerColumnType::Date => {
                let initial = state.initial.date;
                let today = Date::today();
                (
                    DATE_COLUMN_RADIUS_DAYS as usize * 2 + 1,
                    Box::new(move |index| {
                        let date =
                            add_days_to_date(initial, index as i32 - DATE_COLUMN_RADIUS_DAYS);
                        let text = if date == today {
                            L10n::TODAY_LABEL.to_string()
                        } else {
                            L10n::date_picker_medium_date(
                                date.weekday(),
                                date.month as usize,
                                date.day,
                            )
                        };
                        (text, true)
                    }),
                    Box::new(move |index| {
                        let report_time = report_time.clone();
                        handle.set_state(move |state| {
                            state.selected_day_from_initial =
                                index as i32 - DATE_COLUMN_RADIUS_DAYS;
                            report_time(state);
                        });
                    }),
                )
            }
            PickerColumnType::Hour => {
                let flipped = state.is_hour_region_flipped();
                let meridiem_controller = state.meridiem_controller.clone();
                (
                    24,
                    Box::new(move |index| {
                        let hour = if flipped { (index + 12) % 24 } else { index } as u32;
                        let display = if use_24h { hour } else { (hour + 11) % 12 + 1 };
                        (L10n::date_picker_hour(display), true)
                    }),
                    Box::new(move |index| {
                        let meridiem_controller = meridiem_controller.clone();
                        let report_time = report_time.clone();
                        handle.set_state(move |state| {
                            // Upstream's `_buildHourPicker`'s callback: the
                            // *physical* region is what the wheel says, and
                            // crossing it flips which meridiem the two halves
                            // stand for.
                            let region_changed = state.meridiem_region != index / 12;
                            if region_changed {
                                state.meridiem_region = index / 12;
                                state.selected_am_pm = 1 - state.selected_am_pm;
                            }
                            state.selected_hour_index = index;
                            if !use_24h && region_changed {
                                // The AM/PM column follows, and its own
                                // callback is what reports the change.
                                meridiem_controller.animate_to_item(
                                    state.selected_am_pm,
                                    MERIDIEM_ANIMATION_MICROS,
                                    Curve::EASE_OUT,
                                );
                            } else {
                                report_time(state);
                            }
                        });
                    }),
                )
            }
            PickerColumnType::Minute => (
                (60 / minute_interval) as usize,
                Box::new(move |index| {
                    let minute = index as u32 * minute_interval;
                    (L10n::date_picker_minute(minute), true)
                }),
                Box::new(move |index| {
                    let report_time = report_time.clone();
                    handle.set_state(move |state| {
                        state.selected_minute_index = index;
                        report_time(state);
                    });
                }),
            ),
            PickerColumnType::DayPeriod => (
                2,
                Box::new(move |index| {
                    let text = if index == 0 {
                        L10n::ANTE_MERIDIEM_ABBREVIATION
                    } else {
                        L10n::POST_MERIDIEM_ABBREVIATION
                    };
                    (text.to_string(), true)
                }),
                Box::new(move |index| {
                    let report_time = report_time.clone();
                    handle.set_state(move |state| {
                        state.selected_am_pm = index;
                        report_time(state);
                    });
                }),
            ),
            PickerColumnType::TimeSeparator => {
                (1, Box::new(|_| (":".to_string(), true)), Box::new(|_| {}))
            }
        }
    }
}

/// Upstream's `_pickerDidStopScrolling`, the one clause of it this port
/// carries: "Some months have less days (e.g. February). Go to the last day of
/// that month if the selectedDay exceeds the maximum."
fn clamp_day_column(
    state: &mut CupertinoDatePickerState,
    day_controller: &FixedExtentScrollController,
) {
    let days = days_in_month(state.selected_year, state.selected_month);
    if state.selected_day > days {
        state.selected_day = days;
        day_controller.animate_to_item(
            days as usize - 1,
            COLUMN_ANIMATION_MICROS,
            Curve::EASE_IN_OUT,
        );
    }
}

/// One column of a [`CupertinoTimerPicker`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerPickerUnit {
    Hour,
    Minute,
    Second,
}

/// Upstream `CupertinoTimerPickerMode`: which units the timer shows.
///
/// **The minute is in all three.** Upstream's `initState` says so by leaving
/// it unconditional and guarding only the other two:
///
/// ```dart
/// selectedMinute = widget.initialTimerDuration.inMinutes % 60;
/// if (widget.mode != CupertinoTimerPickerMode.ms) { selectedHour = ...; }
/// if (widget.mode != CupertinoTimerPickerMode.hm) { selectedSecond = ...; }
/// ```
///
/// So the three modes are not three lists of units; they are **which of the
/// hour and the second keep the minute company.**
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CupertinoTimerPickerMode {
    /// Hours and minutes: "16 hours | 14 min".
    Hm,
    /// Minutes and seconds: "14 min | 43 sec".
    Ms,
    /// All three. Upstream's default.
    #[default]
    Hms,
}

impl CupertinoTimerPickerMode {
    pub const ALL: [CupertinoTimerPickerMode; 3] = [
        CupertinoTimerPickerMode::Hm,
        CupertinoTimerPickerMode::Ms,
        CupertinoTimerPickerMode::Hms,
    ];

    /// The columns, left to right. Everything else here is read off this.
    pub fn columns(self) -> &'static [TimerPickerUnit] {
        match self {
            CupertinoTimerPickerMode::Hm => &[TimerPickerUnit::Hour, TimerPickerUnit::Minute],
            CupertinoTimerPickerMode::Ms => &[TimerPickerUnit::Minute, TimerPickerUnit::Second],
            CupertinoTimerPickerMode::Hms => &[
                TimerPickerUnit::Hour,
                TimerPickerUnit::Minute,
                TimerPickerUnit::Second,
            ],
        }
    }

    /// Upstream divides the width by `mode == hms ? 3 : 2`.
    pub fn column_count(self) -> usize {
        self.columns().len()
    }

    pub fn shows(self, unit: TimerPickerUnit) -> bool {
        self.columns().contains(&unit)
    }

    /// Where a unit sits, counting from the left.
    ///
    /// Upstream writes this out per unit at each call site -- the minute's
    /// off-axis fraction is `mode == ms ? 0 : 1`, the second's is
    /// `mode == ms ? 1 : 2`. **Both are just this index**, and deriving it
    /// keeps the two from disagreeing when a mode is added.
    pub fn index_of(self, unit: TimerPickerUnit) -> Option<usize> {
        self.columns().iter().position(|held| *held == unit)
    }
}

/// Upstream `CupertinoTimerPicker`.
///
/// A countdown, not a clock: every column carries its own unit label beside
/// the number, and the columns are placed by measuring those labels rather
/// than by [`DatePickerLayout`].
#[derive(Clone)]
pub struct CupertinoTimerPicker {
    /// Upstream's `initialTimerDuration`, in seconds.
    pub initial_timer_duration_secs: i64,
    pub minute_interval: u32,
    pub second_interval: u32,
    pub mode: CupertinoTimerPickerMode,
    pub item_extent: f32,
    id: u64,
    background_color: Option<Color>,
    on_changed: Option<Rc<dyn Fn(i64)>>,
}

impl PartialEq for CupertinoTimerPicker {
    fn eq(&self, other: &CupertinoTimerPicker) -> bool {
        self.initial_timer_duration_secs == other.initial_timer_duration_secs
            && self.minute_interval == other.minute_interval
            && self.second_interval == other.second_interval
            && self.mode == other.mode
            && self.item_extent == other.item_extent
            && self.id == other.id
            && self.background_color == other.background_color
    }
}

impl std::fmt::Debug for CupertinoTimerPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoTimerPicker")
            .field("mode", &self.mode)
            .field(
                "initial_timer_duration_secs",
                &self.initial_timer_duration_secs,
            )
            .finish_non_exhaustive()
    }
}

impl CupertinoTimerPicker {
    /// A day, in seconds. Upstream: `assert(initialTimerDuration < const
    /// Duration(days: 1))`.
    pub const ONE_DAY_SECS: i64 = 24 * 60 * 60;

    pub fn new(initial_timer_duration_secs: i64) -> CupertinoTimerPicker {
        CupertinoTimerPicker {
            initial_timer_duration_secs,
            minute_interval: 1,
            second_interval: 1,
            mode: CupertinoTimerPickerMode::Hms,
            item_extent: DATE_PICKER_ITEM_EXTENT,
            id: 0,
            background_color: None,
            on_changed: None,
        }
    }

    /// The hit-test identity of the first column; the rest count up from it.
    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    pub fn with_mode(mut self, mode: CupertinoTimerPickerMode) -> Self {
        self.mode = mode;
        self
    }

    /// Upstream's `backgroundColor`.
    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Upstream's `onTimerDurationChanged`, in seconds.
    pub fn with_on_changed(mut self, on_changed: impl Fn(i64) + 'static) -> Self {
        self.on_changed = Some(Rc::new(on_changed));
        self
    }

    /// Upstream's two duration asserts: at least zero, and **strictly less than
    /// a day**. The picker shows hours, minutes and seconds, so twenty-four
    /// hours has nowhere to be displayed -- it would read as zero.
    pub fn validate(&self) -> bool {
        self.initial_timer_duration_secs >= 0
            && self.initial_timer_duration_secs < CupertinoTimerPicker::ONE_DAY_SECS
    }
}

/// What a live [`CupertinoTimerPicker`] remembers: upstream's
/// `selectedHour`/`selectedMinute`/`selectedSecond`, which are `null` for the
/// units the mode does not show.
#[derive(Default)]
pub struct CupertinoTimerPickerState {
    hour: Option<u32>,
    minute: u32,
    second: Option<u32>,
}

impl CupertinoTimerPickerState {
    fn seconds(&self) -> i64 {
        self.hour.unwrap_or(0) as i64 * 3600
            + self.minute as i64 * 60
            + self.second.unwrap_or(0) as i64
    }
}

/// What `_measureLabelMetrics` works out, which every padding below is
/// arithmetic on.
struct TimerPickerMetrics {
    /// The width of the widest two-digit number, on upstream's four stated
    /// assumptions -- among them "the widest 2-digit number is composed of 2
    /// same 1-digit numbers that has the biggest width".
    number_label_width: f32,
    number_label_height: f32,
    number_label_baseline: f32,
    hour_label_width: f32,
    minute_label_width: f32,
    second_label_width: f32,
}

impl TimerPickerMetrics {
    fn measure(style: &TextStyle) -> TimerPickerMetrics {
        let widest = (0..=9)
            .map(|digit| digit.to_string())
            .max_by(|a, b| {
                column_width(std::slice::from_ref(a), style)
                    .partial_cmp(&column_width(std::slice::from_ref(b), style))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or_else(|| "0".to_string());
        let two_digits = format!("{widest}{widest}");
        let paragraph =
            crate::painting::shape(&two_digits, style, None, false, f32::MAX / 4.0, 1.0);
        TimerPickerMetrics {
            number_label_width: paragraph.max_intrinsic_width(),
            number_label_height: paragraph.height(),
            // The crate's paragraph has no `computeDistanceToActualBaseline`;
            // the alphabetic baseline of a single line sits at the ascent,
            // which for these one-line labels is the same measurement.
            number_label_baseline: paragraph.height() * 0.8,
            hour_label_width: column_width(&["hour".to_string(), "hours".to_string()], style),
            minute_label_width: column_width(&["min.".to_string()], style),
            second_label_width: column_width(&["sec.".to_string()], style),
        }
    }
}

impl StatefulComponent for CupertinoTimerPicker {
    type State = CupertinoTimerPickerState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn initial_state(&self) -> CupertinoTimerPickerState {
        let total = self.initial_timer_duration_secs.max(0);
        CupertinoTimerPickerState {
            hour: (self.mode != CupertinoTimerPickerMode::Ms).then_some((total / 3600) as u32),
            minute: (total / 60 % 60) as u32,
            second: (self.mode != CupertinoTimerPickerMode::Hm).then_some((total % 60) as u32),
        }
    }

    fn build(
        &self,
        state: &CupertinoTimerPickerState,
        handle: StateHandle<CupertinoTimerPickerState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let label_color = theme.resolve(CupertinoColors::LABEL);
        // The numbers are drawn at the magnified size: upstream overrides the
        // theme's `pickerTextStyle` with `_textStyleFrom(context,
        // _kTimerPickerMagnification)` around the whole picker, and
        // `CupertinoPicker` hands that style to its children.
        let number_style = picker_text_style(label_color, TIMER_PICKER_MAGNIFICATION);
        let metrics = TimerPickerMetrics::measure(&number_style);

        // Upstream measures against the incoming constraints and shrinks to
        // fit; there is no `LayoutBuilder` in this crate, so the picker takes
        // its natural width and a caller narrower than that clips instead.
        let (picker_column_width, total_width) = match self.mode {
            CupertinoTimerPickerMode::Hms => {
                let column =
                    TIMER_PICKER_COLUMN_INTRINSIC_WIDTH + TIMER_PICKER_HALF_COLUMN_PADDING * 2.0;
                (column, column * 3.0)
            }
            _ => (DATE_PICKER_WIDTH / 2.0, DATE_PICKER_WIDTH),
        };

        let base_label_content_width = metrics.number_label_width + TIMER_PICKER_LABEL_PAD_SIZE;
        let minute_label_content_width = base_label_content_width + metrics.minute_label_width;

        // `(unit, start padding, end padding)` per column, straight out of
        // upstream's three-armed switch.
        let columns: Vec<(TimerPickerUnit, f32, f32)> = match self.mode {
            CupertinoTimerPickerMode::Hm => {
                let hour_label_content_width = base_label_content_width + metrics.hour_label_width;
                let hour_start = (picker_column_width
                    - hour_label_content_width
                    - TIMER_PICKER_HALF_COLUMN_PADDING)
                    .max(TIMER_PICKER_MIN_HORIZONTAL_PADDING);
                let minute_end = (picker_column_width
                    - minute_label_content_width
                    - TIMER_PICKER_HALF_COLUMN_PADDING)
                    .max(TIMER_PICKER_MIN_HORIZONTAL_PADDING);
                vec![
                    (
                        TimerPickerUnit::Hour,
                        hour_start,
                        picker_column_width - hour_start - hour_label_content_width,
                    ),
                    (
                        TimerPickerUnit::Minute,
                        picker_column_width - minute_end - minute_label_content_width,
                        minute_end,
                    ),
                ]
            }
            CupertinoTimerPickerMode::Ms => {
                let second_label_content_width =
                    base_label_content_width + metrics.second_label_width;
                let second_end = (picker_column_width
                    - second_label_content_width
                    - TIMER_PICKER_HALF_COLUMN_PADDING)
                    .max(TIMER_PICKER_MIN_HORIZONTAL_PADDING);
                let minute_start = (picker_column_width
                    - minute_label_content_width
                    - TIMER_PICKER_HALF_COLUMN_PADDING)
                    .max(TIMER_PICKER_MIN_HORIZONTAL_PADDING);
                vec![
                    (
                        TimerPickerUnit::Minute,
                        minute_start,
                        picker_column_width - minute_start - minute_label_content_width,
                    ),
                    (
                        TimerPickerUnit::Second,
                        picker_column_width - second_end - minute_label_content_width,
                        second_end,
                    ),
                ]
            }
            CupertinoTimerPickerMode::Hms => {
                let hour_end = picker_column_width
                    - base_label_content_width
                    - metrics.hour_label_width
                    - TIMER_PICKER_MIN_HORIZONTAL_PADDING;
                let minute_padding = (picker_column_width - minute_label_content_width) / 2.0;
                let second_start = picker_column_width
                    - base_label_content_width
                    - metrics.second_label_width
                    - TIMER_PICKER_MIN_HORIZONTAL_PADDING;
                vec![
                    (
                        TimerPickerUnit::Hour,
                        TIMER_PICKER_MIN_HORIZONTAL_PADDING,
                        hour_end.max(0.0),
                    ),
                    (TimerPickerUnit::Minute, minute_padding, minute_padding),
                    (
                        TimerPickerUnit::Second,
                        second_start.max(0.0),
                        TIMER_PICKER_MIN_HORIZONTAL_PADDING,
                    ),
                ]
            }
        };

        let column_count = columns.len();
        let built: Vec<AnyWidget> = columns
            .iter()
            .enumerate()
            .map(|(index, (unit, start, end))| {
                self.build_column(
                    *unit,
                    index,
                    column_count,
                    (*start).max(0.0),
                    (*end).max(0.0),
                    &metrics,
                    &number_style,
                    label_color,
                    state,
                    handle.clone(),
                )
            })
            .collect();

        let background = self.background_color;
        many(built, move |rendered| {
            let mut row = crate::render::RenderFlex::row()
                .with_main_axis_size(crate::render::MainAxisSize::Max);
            for child in rendered {
                row = row.push_flex(crate::render::FlexChild::expanded(child, 1));
            }
            let mut container = Container::new()
                .with_size(total_width, DATE_PICKER_HEIGHT)
                .with_child(row);
            if let Some(background) = background {
                container = container.with_color(background);
            }
            // Upstream's `Align(alignment: widget.alignment)`, whose default
            // is `Alignment.center`.
            Align::new(Alignment::CENTER, container)
        })
    }
}

impl CupertinoTimerPicker {
    /// Upstream's `_buildHourColumn` and its two siblings: the wheel with the
    /// unit label stacked over it.
    #[allow(clippy::too_many_arguments)]
    fn build_column(
        &self,
        unit: TimerPickerUnit,
        index: usize,
        columns: usize,
        start: f32,
        end: f32,
        metrics: &TimerPickerMetrics,
        number_style: &TextStyle,
        label_color: Color,
        state: &CupertinoTimerPickerState,
        handle: StateHandle<CupertinoTimerPickerState>,
    ) -> AnyWidget {
        let overlay = CupertinoPickerDefaultSelectionOverlay::for_column(index, columns);
        let id = self.id + 1 + index as u64;
        let number_label_width = metrics.number_label_width;
        let interval = match unit {
            TimerPickerUnit::Hour => 1,
            TimerPickerUnit::Minute => self.minute_interval.max(1),
            TimerPickerUnit::Second => self.second_interval.max(1),
        };
        let (count, initial) = match unit {
            TimerPickerUnit::Hour => (24, state.hour.unwrap_or(0) as usize),
            TimerPickerUnit::Minute => {
                ((60 / interval) as usize, (state.minute / interval) as usize)
            }
            TimerPickerUnit::Second => (
                (60 / interval) as usize,
                (state.second.unwrap_or(0) / interval) as usize,
            ),
        };

        // The number, in the box upstream's `_buildPickerNumberLabel` gives it:
        // the column is wider than its content because the selection band's
        // separators are part of the column.
        let style = number_style.clone();
        let build_item = move |i: usize| {
            let value = i as u32 * interval;
            let text = match unit {
                TimerPickerUnit::Hour => L10n::timer_picker_hour(value),
                TimerPickerUnit::Minute => L10n::timer_picker_minute(value),
                TimerPickerUnit::Second => L10n::timer_picker_second(value),
            };
            let style = style.clone();
            leaf(move || {
                Container::new()
                    .with_width(TIMER_PICKER_COLUMN_INTRINSIC_WIDTH + start + end)
                    .with_padding(EdgeInsets::only(start, 0.0, end, 0.0))
                    .with_child(Align::new(
                        Alignment::CENTER_LEFT,
                        RenderConstrainedBox::new(BoxConstraints::new(
                            number_label_width,
                            number_label_width,
                            0.0,
                            f32::INFINITY,
                        ))
                        .with_child(Align::new(
                            Alignment::CENTER_RIGHT,
                            Text::new(text.clone())
                                .with_style(style.clone())
                                .with_soft_wrap(false)
                                .with_max_lines(1),
                        )),
                    ))
            })
        };

        let on_changed = self.on_changed.clone();
        let mut picker = CupertinoPicker::new(id, self.item_extent, count, build_item)
            .with_magnifier(DATE_PICKER_USE_MAGNIFIER)
            .with_magnification(DATE_PICKER_MAGNIFICATION)
            .with_squeeze(DATE_PICKER_SQUEEZE)
            // Upstream's `looping: true` on the minute and second columns and
            // not on the hour's: a countdown's hours have a first and a last.
            .with_looping(unit != TimerPickerUnit::Hour)
            .with_off_axis_fraction(self.off_axis_fraction(start, index, metrics))
            .with_selection_overlay(Some(overlay))
            .with_initial_item(initial)
            .with_on_selected(move |i| {
                let on_changed = on_changed.clone();
                handle.set_state(move |state| {
                    let value = i as u32 * interval;
                    match unit {
                        TimerPickerUnit::Hour => state.hour = Some(value),
                        TimerPickerUnit::Minute => state.minute = value,
                        TimerPickerUnit::Second => state.second = Some(value),
                    }
                    if let Some(on_changed) = &on_changed {
                        on_changed(state.seconds());
                    }
                });
            });
        if let Some(background) = self.background_color {
            picker = picker.with_background_color(background);
        }

        // The unit label, which does not scroll: upstream's `_buildLabel`,
        // an `IgnorePointer` over the wheel.
        let label = match unit {
            TimerPickerUnit::Hour => {
                L10n::timer_picker_hour_label(state.hour.unwrap_or(0)).to_string()
            }
            TimerPickerUnit::Minute => L10n::timer_picker_minute_label(state.minute).to_string(),
            TimerPickerUnit::Second => {
                L10n::timer_picker_second_label(state.second.unwrap_or(0)).to_string()
            }
        };
        let label_height = metrics.number_label_height;
        let baseline = metrics.number_label_baseline;
        let label = leaf(move || {
            crate::render::RenderIgnorePointer::new(
                Container::new()
                    .with_padding(EdgeInsets::only(
                        number_label_width + TIMER_PICKER_LABEL_PAD_SIZE + start,
                        0.0,
                        0.0,
                        0.0,
                    ))
                    .with_child(Align::new(
                        Alignment::CENTER_LEFT,
                        RenderConstrainedBox::new(BoxConstraints::new(
                            0.0,
                            f32::INFINITY,
                            label_height,
                            label_height,
                        ))
                        .with_child(crate::render::RenderBaseline::new(
                            baseline,
                            Text::new(label.clone())
                                .with_style(TextStyle {
                                    font_size: TIMER_PICKER_LABEL_FONT_SIZE,
                                    font_weight: 600,
                                    color: label_color,
                                    ..TextStyle::default()
                                })
                                .with_soft_wrap(false)
                                .with_max_lines(1),
                        )),
                    )),
            )
        });

        many(vec![stateful(picker), label], |rendered| {
            let mut stack =
                crate::render::RenderStack::new().with_fit(crate::render::StackFit::Expand);
            for child in rendered {
                stack = stack.push(child);
            }
            stack
        })
    }

    /// Upstream's `_calculateOffAxisFraction`: "Calculate the number label
    /// center point by padding start and position to get a reasonable
    /// offAxisFraction."
    fn off_axis_fraction(&self, start: f32, position: usize, metrics: &TimerPickerMetrics) -> f32 {
        let (column_width, total_width) = match self.mode {
            CupertinoTimerPickerMode::Hms => {
                let column =
                    TIMER_PICKER_COLUMN_INTRINSIC_WIDTH + TIMER_PICKER_HALF_COLUMN_PADDING * 2.0;
                (column, column * 3.0)
            }
            _ => (DATE_PICKER_WIDTH / 2.0, DATE_PICKER_WIDTH),
        };
        let center_point = start + metrics.number_label_width / 2.0;
        let in_column = 0.5 - center_point / column_width;
        let in_picker = 0.5 - (center_point + column_width * position as f32) / total_width;
        in_column - in_picker
    }
}

/// Upstream `CupertinoMenuEntry`, an `abstract interface class` with exactly two
/// members -- and **both of them are about the item's neighbours rather than
/// itself.**
pub trait CupertinoMenuEntry {
    /// Upstream: *"If `hasLeading` returns true, **siblings** of this menu item
    /// that are missing a leading widget will have leading space added to align
    /// the leading edges of all menu items."*
    ///
    /// **One item with an icon indents every other item.** The alignment belongs
    /// to the group, and the group works it out by asking each member -- so the
    /// answer to "does this item have a leading widget" is read by everybody
    /// except that item.
    fn has_leading(&self) -> bool;

    /// Upstream: *"When true, a divider will **not** be drawn above or below this
    /// menu item. Otherwise, adjacent menu items will be separated by a
    /// divider."*
    ///
    /// So the flag does not say "I am a line", it says **"do not put lines next
    /// to me"** -- which is what stops a divider you asked for from arriving
    /// between two the menu drew itself.
    fn is_divider(&self) -> bool;
}

/// What colour a menu item's label is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItemLabel {
    /// `CupertinoColors.systemGrey`, for an item that cannot be pressed.
    Disabled,
    /// `CupertinoColors.systemRed`, for one that will destroy something.
    Destructive,
    /// The ordinary label colour.
    Ordinary,
}

/// Upstream `CupertinoMenuItem`.
///
/// # Disabled beats destructive
///
/// `_resolveDefaultTextStyle` asks in this order:
///
/// ```text
/// if (onPressed == null)      color = systemGrey;
/// else if (isDestructiveAction) color = systemRed;
/// else                          color = _kDefaultTextColor;
/// ```
///
/// So a disabled destructive item is **grey and not red**. The warning colour
/// is withdrawn along with the ability to act: there is nothing to warn about
/// in a button that cannot be pressed, and a red one that does nothing would
/// be alarming for no reason.
///
/// # Pressing closes the menu before it runs the callback
///
/// `_handleSelect` closes first and calls `onPressed` after, so the callback
/// runs with the menu already going. A callback that pushes a route is not
/// then fighting the menu's own dismissal for the same frame.
///
/// And `requestCloseOnActivate: false` stops the closing, not the callback --
/// the two are separate steps and only the first is optional.
///
/// # The subtitle's colour is a blend, and an approximation of one
///
/// Upstream sets `foreground: Paint()..blendMode = isDark ? BlendMode.plus :
/// BlendMode.hardLight`, with its own note that iOS uses `linearDodge` in the
/// dark and `plusDarker` in the light, and that these are approximations of
/// those. The whole style is marked "approximated from the iOS and iPadOS 18.5
/// simulators".
///
/// Recorded because a port that reproduced these two modes exactly would be
/// reproducing an approximation, not the platform -- worth knowing before
/// anybody tunes them against a screenshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoMenuItem {
    pub leading: bool,
    /// Upstream's `onPressed == null`, which is what "disabled" means here.
    pub enabled: bool,
    /// Upstream's `isDestructiveAction`, **false** by default.
    pub is_destructive_action: bool,
    /// Upstream's `requestCloseOnActivate`, **true** by default.
    pub request_close_on_activate: bool,
    /// Upstream's `requestFocusOnHover`, **true** by default -- the same as
    /// Material's `MenuItemButton.requestFocusOnHover`.
    ///
    /// I wrote the opposite here first, on the strength of this port having it
    /// false on the Material side. Checking upstream showed the port was
    /// wrong, not the platforms: both default to true, and the Material one
    /// has been corrected.
    pub request_focus_on_hover: bool,
}

impl Default for CupertinoMenuItem {
    fn default() -> CupertinoMenuItem {
        CupertinoMenuItem::new()
    }
}

impl CupertinoMenuItem {
    pub fn new() -> CupertinoMenuItem {
        CupertinoMenuItem {
            leading: false,
            enabled: true,
            is_destructive_action: false,
            request_close_on_activate: true,
            request_focus_on_hover: true,
        }
    }

    pub fn destructive() -> CupertinoMenuItem {
        CupertinoMenuItem {
            is_destructive_action: true,
            ..CupertinoMenuItem::new()
        }
    }

    pub fn disabled(mut self) -> CupertinoMenuItem {
        self.enabled = false;
        self
    }

    /// Upstream's `_resolveDefaultTextStyle` colour ladder -- see the type's
    /// docs for why disabled is asked first.
    pub fn label(&self) -> MenuItemLabel {
        if !self.enabled {
            return MenuItemLabel::Disabled;
        }
        if self.is_destructive_action {
            return MenuItemLabel::Destructive;
        }
        MenuItemLabel::Ordinary
    }

    /// The colour that label resolves to.
    pub fn label_color(&self) -> crate::cupertino::CupertinoDynamicColor {
        match self.label() {
            MenuItemLabel::Disabled => crate::cupertino::CupertinoColors::SYSTEM_GREY,
            MenuItemLabel::Destructive => crate::cupertino::CupertinoColors::SYSTEM_RED,
            MenuItemLabel::Ordinary => crate::cupertino::CupertinoColors::LABEL,
        }
    }

    /// Upstream's `_handleSelect`, as the two things it does and their order.
    ///
    /// Closing is conditional and calling is not, so an item that keeps the
    /// menu open still does its work.
    pub fn activation(&self) -> (bool, bool) {
        (self.request_close_on_activate, self.enabled)
    }

    /// Upstream's subtitle blend: `plus` in the dark, `hardLight` in the
    /// light. Both are approximations -- see the type's docs.
    pub fn subtitle_blend(is_dark: bool) -> crate::painting::BlendMode {
        if is_dark {
            crate::painting::BlendMode::Plus
        } else {
            crate::painting::BlendMode::HardLight
        }
    }
}

impl CupertinoMenuEntry for CupertinoMenuItem {
    fn has_leading(&self) -> bool {
        self.leading
    }

    fn is_divider(&self) -> bool {
        false
    }
}

/// Upstream `CupertinoMenuDivider`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CupertinoMenuDivider;

impl CupertinoMenuEntry for CupertinoMenuDivider {
    fn has_leading(&self) -> bool {
        false
    }

    fn is_divider(&self) -> bool {
        true
    }
}

/// Upstream `CupertinoMenuAnchor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoMenuAnchor {
    pub enable_swipe: bool,
    pub enable_long_press_to_open: bool,
}

impl CupertinoMenuAnchor {
    pub fn new() -> CupertinoMenuAnchor {
        CupertinoMenuAnchor {
            enable_swipe: true,
            enable_long_press_to_open: true,
        }
    }

    /// Upstream's one constructor assert:
    /// `assert(enableSwipe || !enableLongPressToOpen, 'enableLongPressToOpen
    /// cannot be true if enableSwipe is false')`.
    ///
    /// Another implication, and a physical one. A long press that opens the menu
    /// leaves your finger already down on it; the gesture continues straight
    /// into a swipe to pick something. **Opening by long press with swiping
    /// turned off would strand the finger that opened it.**
    pub fn is_valid(&self) -> bool {
        self.enable_swipe || !self.enable_long_press_to_open
    }

    /// Where the separators fall between a run of entries, given each one's
    /// answer to `isDivider`. Returns the gaps that get a drawn rule.
    pub fn drawn_dividers(entries: &[&dyn CupertinoMenuEntry]) -> Vec<usize> {
        (0..entries.len().saturating_sub(1))
            .filter(|&gap| !entries[gap].is_divider() && !entries[gap + 1].is_divider())
            .collect()
    }

    /// Whether the menu indents its items, which one leading widget anywhere is
    /// enough to decide.
    pub fn aligns_leading_edges(entries: &[&dyn CupertinoMenuEntry]) -> bool {
        entries.iter().any(|entry| entry.has_leading())
    }
}

impl Default for CupertinoMenuAnchor {
    fn default() -> Self {
        CupertinoMenuAnchor::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time_picker() -> CupertinoDatePicker {
        CupertinoDatePicker::new(CupertinoDatePickerMode::Time)
    }

    // -- A wheel has no ends ---------------------------------------------------------

    #[test]
    fn a_minute_interval_has_to_divide_sixty_not_merely_be_positive() {
        let mut picker = time_picker();
        for good in [1, 2, 3, 5, 10, 15, 20, 30] {
            picker.minute_interval = good;
            assert_eq!(picker.validate(), Ok(()), "interval {good}");
        }
        for bad in [7, 8, 9, 11, 45] {
            picker.minute_interval = bad;
            assert_eq!(
                picker.validate(),
                Err(DatePickerError::MinuteIntervalNotAFactorOfSixty),
                "interval {bad}"
            );
        }
    }

    #[test]
    fn because_the_one_gap_that_is_not_an_interval_is_the_one_you_cannot_see() {
        // Seven would run 0, 7, ... 56 and then meet 0 four minutes later.
        let mut picker = time_picker();
        picker.minute_interval = 7;
        assert_eq!(picker.minute_stops().last(), Some(&56));
        assert!(!picker.wheel_closes_evenly());

        picker.minute_interval = 15;
        assert_eq!(picker.minute_stops(), vec![0, 15, 30, 45]);
        assert!(picker.wheel_closes_evenly());
    }

    #[test]
    fn and_you_cannot_start_between_two_positions_of_a_wheel_that_has_none() {
        let mut picker = time_picker();
        picker.minute_interval = 15;
        picker.initial.minute = 30;
        assert_eq!(picker.validate(), Ok(()));

        picker.initial.minute = 31;
        assert_eq!(
            picker.validate(),
            Err(DatePickerError::InitialMinuteNotOnAStop)
        );
    }

    // -- Six implications, one per mode ------------------------------------------------

    #[test]
    fn the_day_of_week_column_belongs_to_one_mode_only() {
        let mut picker = CupertinoDatePicker::new(CupertinoDatePickerMode::Date);
        picker.show_day_of_week = true;
        assert_eq!(picker.validate(), Ok(()));

        for other in [
            CupertinoDatePickerMode::Time,
            CupertinoDatePickerMode::DateAndTime,
            CupertinoDatePickerMode::MonthYear,
        ] {
            picker.mode = other;
            assert_eq!(
                picker.validate(),
                Err(DatePickerError::DayOfWeekOutsideDateMode),
                "{other:?}"
            );
        }
    }

    #[test]
    fn and_the_time_separator_to_the_two_that_show_a_time() {
        let mut picker = time_picker();
        picker.show_time_separator = true;
        assert_eq!(picker.validate(), Ok(()));

        picker.mode = CupertinoDatePickerMode::DateAndTime;
        assert_eq!(picker.validate(), Ok(()));

        picker.mode = CupertinoDatePickerMode::Date;
        assert_eq!(
            picker.validate(),
            Err(DatePickerError::TimeSeparatorOutsideTimeModes)
        );
    }

    #[test]
    fn a_mode_cannot_change_once_the_picker_is_built() {
        // The second widget in this sweep with an argument that is part of its
        // identity, after the stepper's step list.
        let picker = time_picker();
        assert_eq!(picker.accepts_update(&time_picker()), Ok(()));
        assert_eq!(
            picker.accepts_update(&CupertinoDatePicker::new(CupertinoDatePickerMode::Date)),
            Err(DatePickerError::ModeChangedAfterBuild)
        );
    }

    #[test]
    fn the_initial_date_has_to_be_inside_the_range_it_was_given() {
        let day = Date {
            year: 2026,
            month: 8,
            day: 27,
        };
        let at = |hour, minute| PickerDateTime::new(day, hour, minute);
        let mut picker = time_picker();
        picker.initial = at(1, 40);
        picker.minimum = Some(at(3, 20));
        assert_eq!(
            picker.validate(),
            Err(DatePickerError::InitialBeforeMinimum)
        );

        picker.minimum = Some(at(0, 50));
        picker.maximum = Some(at(1, 20));
        assert_eq!(picker.validate(), Err(DatePickerError::InitialAfterMaximum));

        picker.maximum = Some(at(2, 30));
        assert_eq!(picker.validate(), Ok(()));
    }

    #[test]
    fn a_row_needs_a_height() {
        let mut picker = time_picker();
        picker.item_extent = 0.0;
        assert_eq!(
            picker.validate(),
            Err(DatePickerError::NonPositiveItemExtent)
        );
    }

    // -- The columns, and where they go ------------------------------------------------

    #[test]
    fn a_date_picker_shows_month_day_year_and_a_time_picker_hour_minute_am_pm() {
        // The two orders `DefaultCupertinoLocalizations` reports for `en_US`,
        // as the two `build`s assemble them.
        let date = CupertinoDatePicker::new(CupertinoDatePickerMode::Date);
        assert_eq!(
            date.date_columns(),
            vec![
                PickerColumnType::Month,
                PickerColumnType::DayOfMonth,
                PickerColumnType::Year
            ]
        );
        let time = CupertinoDatePicker::new(CupertinoDatePickerMode::Time);
        assert_eq!(
            time.time_columns(),
            vec![
                PickerColumnType::Hour,
                PickerColumnType::Minute,
                PickerColumnType::DayPeriod
            ]
        );
        // `dateAndTime` puts the medium date *in front*, because the order is
        // `date_time_dayPeriod`.
        let both = CupertinoDatePicker::new(CupertinoDatePickerMode::DateAndTime);
        assert_eq!(
            both.time_columns(),
            vec![
                PickerColumnType::Date,
                PickerColumnType::Hour,
                PickerColumnType::Minute,
                PickerColumnType::DayPeriod
            ]
        );
        // And a 24-hour picker has no AM/PM column at all.
        let military =
            CupertinoDatePicker::new(CupertinoDatePickerMode::Time).with_24h_format(true);
        assert_eq!(
            military.time_columns(),
            vec![PickerColumnType::Hour, PickerColumnType::Minute]
        );
    }

    #[test]
    fn a_column_is_as_wide_as_the_longest_thing_it_can_show() {
        let style = date_time_picker_text_style(crate::engine::Color::BLACK);
        let picker = CupertinoDatePicker::new(CupertinoDatePickerMode::Date);
        let month = picker.width_of(PickerColumnType::Month, &style);
        let day = picker.width_of(PickerColumnType::DayOfMonth, &style);
        // "September" against "31": the month column is the wide one, and
        // neither is measured from the value that happens to be selected.
        assert!(month > day * 2.0, "month {month}, day {day}");
        assert!(
            month >= column_width(&["September".to_string()], &style),
            "the widest month is what sets it"
        );
    }

    #[test]
    fn the_group_of_columns_is_centred_and_the_outer_two_take_the_slack() {
        // Upstream's `_DatePickerLayoutDelegate`: each column is padded by 12
        // on both sides, and whatever is left of `maxWidth` is split between
        // the first and the last so the wheels reach the edges.
        use crate::render::{RenderConstrainedBox, RenderCustomMultiChildLayoutBox};
        let widths = vec![100.0, 40.0, 60.0];
        let delegate = DatePickerLayout {
            column_widths: widths.clone(),
            max_width: 320.0,
        };
        let mut laid_out = RenderCustomMultiChildLayoutBox::new(
            Rc::new(delegate) as Rc<dyn MultiChildLayoutDelegate>,
            (0..3)
                .map(|index| {
                    (
                        index as u64,
                        RenderRef::new(RenderConstrainedBox::tight(10.0, 10.0)) as BoxedRender,
                    )
                })
                .collect(),
        );
        crate::render::RenderBox::layout(&mut laid_out, BoxConstraints::tight(400.0, 216.0));
        let mut placed: Vec<(f32, f32)> = Vec::new();
        crate::render::RenderBox::visit_children(&laid_out, &mut |child, offset| {
            placed.push((offset.dx, child.size().width));
        });
        // 3 columns + 24 of padding each = 272; 48 of the 320 is left over.
        let slack = 320.0 - (100.0 + 40.0 + 60.0 + 3.0 * 24.0);
        assert!(
            (placed[0].1 - (100.0 + 24.0 + slack / 2.0)).abs() < 0.01,
            "{placed:?}"
        );
        assert!((placed[1].1 - (40.0 + 24.0)).abs() < 0.01, "{placed:?}");
        assert!(
            (placed[2].1 - (60.0 + 24.0 + slack / 2.0)).abs() < 0.01,
            "{placed:?}"
        );
        // Centred in the 400 it was given: (400 - 320) / 2.
        assert!((placed[0].0 - 40.0).abs() < 0.01, "{placed:?}");
        // And laid end to end.
        assert!(
            (placed[1].0 - (placed[0].0 + placed[0].1)).abs() < 0.01,
            "{placed:?}"
        );
        assert!(
            (placed[2].0 - (placed[1].0 + placed[1].1)).abs() < 0.01,
            "{placed:?}"
        );
    }

    #[test]
    fn a_flipped_hour_region_moves_the_hour_the_column_stands_for() {
        // Upstream's meridiem machinery: the hour column has twenty-four
        // items and twelve labels, and which twelve hours the physical half
        // stands for flips when the AM/PM column moves.
        let mut state = CupertinoDatePickerState {
            selected_am_pm: 0,
            meridiem_region: 0,
            ..Default::default()
        };
        assert!(!state.is_hour_region_flipped());
        assert_eq!(state.hour_of(9), 9);

        state.selected_am_pm = 1;
        assert!(state.is_hour_region_flipped());
        assert_eq!(state.hour_of(9), 21, "the same item now reads as 9 PM");
        assert_eq!(state.hour_of(0), 12, "and midnight as noon");
    }

    // -- A timer is shorter than a day ---------------------------------------------------

    #[test]
    fn a_timer_may_run_to_one_second_short_of_a_day_and_no_further() {
        assert!(CupertinoTimerPicker::new(0).validate());
        assert!(CupertinoTimerPicker::new(CupertinoTimerPicker::ONE_DAY_SECS - 1).validate());
        assert!(
            !CupertinoTimerPicker::new(CupertinoTimerPicker::ONE_DAY_SECS).validate(),
            "twenty-four hours has no column to be displayed in"
        );
        assert!(!CupertinoTimerPicker::new(-1).validate());
    }

    // -- An interface entirely about the neighbours ---------------------------------------

    #[test]
    fn one_item_with_an_icon_indents_all_the_others() {
        let plain = CupertinoMenuItem::new();
        let with_icon = CupertinoMenuItem {
            leading: true,
            ..CupertinoMenuItem::new()
        };

        let all_plain: [&dyn CupertinoMenuEntry; 2] = [&plain, &plain];
        assert!(!CupertinoMenuAnchor::aligns_leading_edges(&all_plain));

        let one_icon: [&dyn CupertinoMenuEntry; 3] = [&plain, &with_icon, &plain];
        assert!(
            CupertinoMenuAnchor::aligns_leading_edges(&one_icon),
            "and the two plain items move for it"
        );
    }

    #[test]
    fn a_divider_stops_the_menu_drawing_its_own_beside_it() {
        // isDivider does not say "I am a line", it says "no lines next to me".
        let item = CupertinoMenuItem::default();
        let rule = CupertinoMenuDivider;

        let plain: [&dyn CupertinoMenuEntry; 3] = [&item, &item, &item];
        assert_eq!(
            CupertinoMenuAnchor::drawn_dividers(&plain),
            vec![0, 1],
            "every gap between plain items gets one"
        );

        let with_rule: [&dyn CupertinoMenuEntry; 3] = [&item, &rule, &item];
        assert_eq!(
            CupertinoMenuAnchor::drawn_dividers(&with_rule),
            Vec::<usize>::new(),
            "and an explicit rule suppresses both gaps it touches"
        );
    }

    #[test]
    fn so_two_rules_never_end_up_side_by_side() {
        let item = CupertinoMenuItem::default();
        let rule = CupertinoMenuDivider;
        let entries: [&dyn CupertinoMenuEntry; 4] = [&item, &rule, &rule, &item];
        assert_eq!(
            CupertinoMenuAnchor::drawn_dividers(&entries),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn a_divider_never_claims_a_leading_widget() {
        assert!(!CupertinoMenuDivider.has_leading());
        assert!(CupertinoMenuDivider.is_divider());
        assert!(!CupertinoMenuItem::default().is_divider());
    }

    // -- The finger that opened it --------------------------------------------------------

    #[test]
    fn opening_by_long_press_needs_swiping_to_stay_on() {
        let mut anchor = CupertinoMenuAnchor::new();
        assert!(anchor.is_valid());

        anchor.enable_swipe = false;
        assert!(
            !anchor.is_valid(),
            "the long press would leave a finger down with nowhere to go"
        );

        anchor.enable_long_press_to_open = false;
        assert!(anchor.is_valid(), "and turning both off is fine");
    }
}

#[cfg(test)]
mod cupertino_menu_item_tests {
    use super::*;
    use crate::cupertino::CupertinoColors;
    use crate::painting::BlendMode;

    #[test]
    fn a_disabled_destructive_item_is_grey_and_not_red() {
        // The warning colour is withdrawn along with the ability to act:
        // there is nothing to warn about in a button that cannot be pressed.
        let off = CupertinoMenuItem::destructive().disabled();
        assert_eq!(off.label(), MenuItemLabel::Disabled);
        assert_eq!(off.label_color(), CupertinoColors::SYSTEM_GREY);

        // And it *is* red while it can be pressed, or the test above would
        // only show that nothing is ever red.
        assert_eq!(
            CupertinoMenuItem::destructive().label(),
            MenuItemLabel::Destructive
        );
        assert_eq!(
            CupertinoMenuItem::destructive().label_color(),
            CupertinoColors::SYSTEM_RED
        );
    }

    #[test]
    fn an_ordinary_disabled_item_is_the_same_grey() {
        // Disabled is one answer, not two: the destructive flag stops
        // mattering entirely rather than tinting the grey.
        assert_eq!(
            CupertinoMenuItem::new().disabled().label_color(),
            CupertinoMenuItem::destructive().disabled().label_color()
        );
    }

    #[test]
    fn and_an_enabled_ordinary_item_is_neither() {
        let plain = CupertinoMenuItem::new();
        assert_eq!(plain.label(), MenuItemLabel::Ordinary);
        assert_ne!(plain.label_color(), CupertinoColors::SYSTEM_GREY);
        assert_ne!(plain.label_color(), CupertinoColors::SYSTEM_RED);
    }

    #[test]
    fn closing_is_optional_and_calling_is_not() {
        // `_handleSelect` closes first and calls after, and only the closing
        // is behind a flag -- an item that keeps the menu open still works.
        let mut keeps_open = CupertinoMenuItem::new();
        keeps_open.request_close_on_activate = false;
        assert_eq!(keeps_open.activation(), (false, true));
        assert_eq!(CupertinoMenuItem::new().activation(), (true, true));
    }

    #[test]
    fn the_three_defaults_are_upstreams() {
        let item = CupertinoMenuItem::new();
        assert!(item.enabled);
        assert!(!item.is_destructive_action, "destructive is opt-in");
        assert!(item.request_close_on_activate, "pressing closes the menu");
        assert!(item.request_focus_on_hover, "the pointer carries the focus");
    }

    #[test]
    fn and_the_material_item_agrees_about_hovering() {
        // Both platforms default `requestFocusOnHover` to true. This port had
        // the Material one false, which is the bug this tick found: moving the
        // mouse and then pressing Enter would have acted on whatever the
        // keyboard had left behind.
        assert!(crate::menu_anchor::MenuItemButton::new().request_focus_on_hover);
        assert_eq!(
            crate::menu_anchor::MenuItemButton::new().request_focus_on_hover,
            CupertinoMenuItem::new().request_focus_on_hover
        );
    }

    #[test]
    fn the_subtitle_blends_differently_in_the_dark() {
        // Both are upstream's own approximations of iOS's `linearDodge` and
        // `plusDarker`, which is why they are worth pinning as *these two*
        // rather than tuned against a screenshot.
        assert_eq!(CupertinoMenuItem::subtitle_blend(true), BlendMode::Plus);
        assert_eq!(
            CupertinoMenuItem::subtitle_blend(false),
            BlendMode::HardLight
        );
        assert_ne!(
            CupertinoMenuItem::subtitle_blend(true),
            CupertinoMenuItem::subtitle_blend(false)
        );
    }

    #[test]
    fn a_menu_item_is_not_a_divider_whatever_else_it_is() {
        for item in [
            CupertinoMenuItem::new(),
            CupertinoMenuItem::destructive(),
            CupertinoMenuItem::new().disabled(),
        ] {
            assert!(!item.is_divider());
        }
    }
}

#[cfg(test)]
mod timer_picker_mode_tests {
    use super::{CupertinoTimerPicker, CupertinoTimerPickerMode, TimerPickerUnit};

    #[test]
    fn the_minute_is_in_every_mode() {
        // Upstream sets selectedMinute unconditionally and guards only the
        // other two, so the modes are which of the hour and the second keep
        // the minute company.
        for mode in CupertinoTimerPickerMode::ALL {
            assert!(mode.shows(TimerPickerUnit::Minute), "{mode:?}");
        }
        assert!(!CupertinoTimerPickerMode::Ms.shows(TimerPickerUnit::Hour));
        assert!(!CupertinoTimerPickerMode::Hm.shows(TimerPickerUnit::Second));
        assert!(CupertinoTimerPickerMode::Hms.shows(TimerPickerUnit::Hour));
        assert!(CupertinoTimerPickerMode::Hms.shows(TimerPickerUnit::Second));
    }

    #[test]
    fn and_the_minute_sits_in_a_different_column_depending_on_the_mode() {
        // Upstream computes this inline as `mode == ms ? 0 : 1` for the
        // minute's off-axis fraction, and `mode == ms ? 1 : 2` for the
        // second's. Both are the column index.
        for mode in CupertinoTimerPickerMode::ALL {
            let expected = if mode == CupertinoTimerPickerMode::Ms {
                0
            } else {
                1
            };
            assert_eq!(
                mode.index_of(TimerPickerUnit::Minute),
                Some(expected),
                "{mode:?}"
            );
        }
        assert_eq!(
            CupertinoTimerPickerMode::Ms.index_of(TimerPickerUnit::Second),
            Some(1)
        );
        assert_eq!(
            CupertinoTimerPickerMode::Hms.index_of(TimerPickerUnit::Second),
            Some(2)
        );
    }

    #[test]
    fn and_a_unit_that_is_not_shown_has_no_column() {
        assert_eq!(
            CupertinoTimerPickerMode::Ms.index_of(TimerPickerUnit::Hour),
            None
        );
        assert_eq!(
            CupertinoTimerPickerMode::Hm.index_of(TimerPickerUnit::Second),
            None
        );
        // Which agrees with `shows`, since both read the same list.
        for mode in CupertinoTimerPickerMode::ALL {
            for unit in [
                TimerPickerUnit::Hour,
                TimerPickerUnit::Minute,
                TimerPickerUnit::Second,
            ] {
                assert_eq!(
                    mode.index_of(unit).is_some(),
                    mode.shows(unit),
                    "{mode:?} {unit:?}"
                );
            }
        }
    }

    #[test]
    fn only_the_full_mode_has_three_columns() {
        // Upstream divides the width by `mode == hms ? 3 : 2`.
        assert_eq!(CupertinoTimerPickerMode::Hms.column_count(), 3);
        assert_eq!(CupertinoTimerPickerMode::Hm.column_count(), 2);
        assert_eq!(CupertinoTimerPickerMode::Ms.column_count(), 2);
    }

    #[test]
    fn the_units_never_change_places() {
        // Whatever the mode leaves out, what remains stays in hour, minute,
        // second order -- a timer reading "43 sec | 14 min" would be nonsense,
        // and nothing else here would catch it.
        for mode in CupertinoTimerPickerMode::ALL {
            let order: Vec<usize> = mode
                .columns()
                .iter()
                .map(|unit| match unit {
                    TimerPickerUnit::Hour => 0,
                    TimerPickerUnit::Minute => 1,
                    TimerPickerUnit::Second => 2,
                })
                .collect();
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(order, sorted, "{mode:?}");
        }
    }

    #[test]
    fn a_timer_picker_shows_everything_unless_told_otherwise() {
        assert_eq!(
            CupertinoTimerPicker::new(0).mode,
            CupertinoTimerPickerMode::Hms
        );
        assert_eq!(
            CupertinoTimerPickerMode::default(),
            CupertinoTimerPickerMode::Hms
        );
    }
}
