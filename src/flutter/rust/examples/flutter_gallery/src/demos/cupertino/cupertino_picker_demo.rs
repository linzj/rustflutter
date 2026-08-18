// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_picker_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoPickerDemo` is five `_Menu` rows -- Date, Time, Date
//! and Time, Timer, Picker -- each opening a `showCupertinoModalPopup` bottom
//! sheet (`_BottomPicker`, 216 high) with a wheel picker in it, and showing
//! the picked value in the row. The wheels here are the framework's
//! `CupertinoPicker`; the values land in [`PickerDemoState`] and the rows
//! reformat on every change, as upstream's `setState` does.
//!
//! Divergences, each marked at its site:
//!
//! * `CupertinoDatePicker` and `CupertinoTimerPicker` are not part of the
//!   framework's Cupertino tier (rustflutter/src/cupertino.rs ports only
//!   `CupertinoPicker`), so the date/time/timer sheets are rows of
//!   `CupertinoPicker` wheels -- one per field of the value -- rather than
//!   upstream's single multi-column widget. The date-and-time sheet's first
//!   column is a month wheel and a day wheel where upstream's
//!   `CupertinoDatePickerMode.dateAndTime` shows formatted date strings.
//! * The modal is a stage-local sheet over a scrim rather than a popup route:
//!   there is no `showCupertinoModalPopup` machinery (cupertino.rs's module
//!   docs: "overlays are the app's"), and the slide-up entrance animation is
//!   not carried -- the sheet appears with the state change.
//! * `DateTime.now()` stands in as UTC: the engine bridge has no local-time
//!   query, the same substitution `pickers::Date::today` documents. The
//!   initial date and time are therefore UTC's.
//! * The sheet's `DefaultTextStyle` (22pt) is not carried: the framework's
//!   `CupertinoPicker::labels` styles its items at text_theme.dart's
//!   `_kDefaultPickerTextStyle` (21pt).
//! * The stage is height-bounded ([`DEMO_HEIGHT`]); upstream's `ListView`
//!   body is a fixed column, five rows never needing to scroll here.

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisAlignment, MainAxisSize, RenderFlex,
    StackPosition,
};
use rustflutter::widgets::{Align, Pointer};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The stage's fixed height, standing in for the demo screen (see the header).
const DEMO_HEIGHT: f32 = 420.0;

/// `_BottomPicker`'s height.
const BOTTOM_PICKER_HEIGHT: f32 = 216.0;

/// `_Menu`'s height.
const MENU_HEIGHT: f32 = 44.0;

/// The wheels' item extent: upstream's `CupertinoPicker(itemExtent: 32.0)`,
/// also `CupertinoDatePicker`'s `_kPickerItemExtent`.
const ITEM_EXTENT: f32 = 32.0;

/// `getDaysOfWeek`: `DateFormat.WEEKDAY` over the seven days from the current
/// week's Monday, which in English is always this list, whatever today is.
const DAYS_OF_WEEK: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Full month names, for `DateFormat.yMMMMd` and the date wheels.
const MONTHS: [&str; 12] = [
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

/// Abbreviated month names, for `DateFormat.yMMMd`.
const MONTHS_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The year wheel's absolute span. Upstream's date-mode year column runs the
/// calendar's full range; two centuries is the window the wheel is given.
const YEAR_MIN: i32 = 1900;
const YEAR_MAX: i32 = 2100;

/// Which row's sheet is open. Upstream's route stack holds at most one popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SheetKind {
    Date,
    Time,
    DateTime,
    Timer,
    Weekday,
}

/// Upstream's `_CupertinoPickerDemoState`: the five picked values, plus which
/// sheet is open (upstream's `Navigator` stack).
struct PickerDemoState {
    /// `timer`, the countdown picker's value, in seconds.
    timer: u64,
    /// `date`, the date-mode picker's value.
    date: Date,
    /// `time`, as (hour of day, minute).
    time: (u32, u32),
    /// `dateTime`, as (date, hour of day, minute).
    date_time: (Date, u32, u32),
    /// `_selectedWeekday`.
    weekday: usize,
    /// The open sheet, if any.
    open: Option<SheetKind>,
}

impl Default for PickerDemoState {
    fn default() -> PickerDemoState {
        // `DateTime.now()`, in UTC (see the header).
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let now = (seconds / 3600 % 24) as u32;
        let minute = (seconds / 60 % 60) as u32;
        PickerDemoState {
            timer: 0,
            date: Date::today(),
            time: (now, minute),
            date_time: (Date::today(), now, minute),
            weekday: 0,
            open: None,
        }
    }
}

/// `DateFormat.yMMMMd().format`: "August 17, 2026".
fn format_date(date: Date) -> String {
    format!(
        "{} {}, {}",
        MONTHS[(date.month - 1) as usize],
        date.day,
        date.year
    )
}

/// `DateFormat.jm().format`: "9:44 AM".
fn format_time(hour: u32, minute: u32) -> String {
    let (hour12, period) = hour12_and_period(hour);
    format!("{hour12}:{minute:02} {period}")
}

/// `DateFormat.yMMMd().add_jm().format`: "Aug 17, 2026 9:44 AM".
fn format_date_time(date: Date, hour: u32, minute: u32) -> String {
    format!(
        "{} {}, {} {}",
        MONTHS_ABBR[(date.month - 1) as usize],
        date.day,
        date.year,
        format_time(hour, minute),
    )
}

/// Upstream's timer row: hours unpadded, minutes and seconds two digits
/// (`timer.inHours` / `padLeft(2, '0')`).
fn format_timer(seconds: u64) -> String {
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// The 12-hour clock reading of an hour of day: (1-12, "AM"/"PM").
fn hour12_and_period(hour: u32) -> (u32, &'static str) {
    let period = if hour >= 12 { "PM" } else { "AM" };
    let hour12 = match hour % 12 {
        0 => 12,
        other => other,
    };
    (hour12, period)
}

/// The day wheel's length for a month, clamping a day that no longer fits --
/// the same clamp upstream's date picker applies when February follows a
/// 31-day month.
fn clamp_day(date: Date) -> Date {
    let days = rustflutter::pickers::days_in_month(date.year, date.month);
    if date.day > days {
        Date { day: days, ..date }
    } else {
        date
    }
}

/// The demo body for the `cupertino-picker` slug. The Cupertino theme the
/// demo page provides upstream (`DemoWrapper`'s `CupertinoTheme(brightness:
/// light)`) is provided here; see the sibling demos' headers.
pub(super) fn stage() -> AnyWidget {
    provide(
        CupertinoTheme::light(),
        single(stateful(PickerDemo), move |inner| {
            Box::new(Container::new().with_height(DEMO_HEIGHT).with_child(inner))
        }),
    )
}

/// Upstream's `CupertinoPickerDemo`.
struct PickerDemo;

impl StatefulComponent for PickerDemo {
    type State = PickerDemoState;

    fn build(
        &self,
        state: &PickerDemoState,
        handle: StateHandle<PickerDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let l10n = GalleryLocalizations::en();
        let label_color = theme.resolve(CupertinoColors::LABEL);
        let value_color = theme.resolve(CupertinoColors::INACTIVE_GRAY);

        // The five rows, upstream's build order: date, time, dateAndTime,
        // countdown timer, picker.
        let rows = vec![
            menu_row(
                ids::DEMO_LOCAL + 40,
                l10n.demo_cupertino_picker_date().to_string(),
                format_date(state.date),
                label_color,
                value_color,
                handle.clone(),
                SheetKind::Date,
            ),
            menu_row(
                ids::DEMO_LOCAL + 41,
                l10n.demo_cupertino_picker_time().to_string(),
                format_time(state.time.0, state.time.1),
                label_color,
                value_color,
                handle.clone(),
                SheetKind::Time,
            ),
            menu_row(
                ids::DEMO_LOCAL + 42,
                l10n.demo_cupertino_picker_date_time().to_string(),
                format_date_time(state.date_time.0, state.date_time.1, state.date_time.2),
                label_color,
                value_color,
                handle.clone(),
                SheetKind::DateTime,
            ),
            menu_row(
                ids::DEMO_LOCAL + 43,
                l10n.demo_cupertino_picker_timer().to_string(),
                format_timer(state.timer),
                label_color,
                value_color,
                handle.clone(),
                SheetKind::Timer,
            ),
            menu_row(
                ids::DEMO_LOCAL + 44,
                l10n.demo_cupertino_picker().to_string(),
                DAYS_OF_WEEK[state.weekday].to_string(),
                label_color,
                value_color,
                handle.clone(),
                SheetKind::Weekday,
            ),
        ];

        // `ListView(children: [SizedBox(height: 32), ...rows])`, a fixed
        // column here (see the header).
        let body = many(rows, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push(Container::new().with_size(1.0, 32.0));
            for row in rendered {
                column = column.push(row);
            }
            Box::new(column)
        });

        let page = component(
            CupertinoPageScaffold::new(body).with_navigation_bar(component(
                // `automaticallyImplyLeading: false`: no back button.
                CupertinoNavigationBar::new().with_middle(l10n.demo_cupertino_picker_title()),
            )),
        );

        // The open sheet, if any, over the page -- upstream's
        // `showCupertinoModalPopup`, stage-local here (see the header).
        match state.open {
            None => page,
            Some(kind) => {
                let barrier: AnyWidget = {
                    // cupertino/dialog.dart's `_kModalBarrierColor`, the
                    // default barrier of `showCupertinoModalPopup`; a tap
                    // dismisses, as the popup's does.
                    let dismiss = handle.clone();
                    leaf(move || {
                        let tap = PointerHandlers::new().with_tap({
                            let dismiss = dismiss.clone();
                            move |_| {
                                dismiss.set_state(|state| state.open = None);
                            }
                        });
                        Pointer::new(
                            ids::DEMO_LOCAL + 45,
                            Container::new().with_color(Color(0x6604_040F)),
                        )
                        .with_handlers(tap)
                    })
                };
                let sheet = bottom_picker(state, handle, &theme, kind);
                many(vec![page, barrier, sheet], move |rendered| {
                    let mut rendered = rendered.into_iter();
                    let page = rendered.next().expect("three layers");
                    let barrier = rendered.next().expect("three layers");
                    let sheet = rendered.next().expect("three layers");
                    Box::new(
                        // `StackFit::Expand` so the sheet's bottom alignment
                        // has the stage's size to align in.
                        rustflutter::render::RenderStack::new()
                            .with_fit(rustflutter::render::StackFit::Expand)
                            .push_positioned(page, StackPosition::fill())
                            .push_positioned(barrier, StackPosition::fill())
                            .push(sheet),
                    )
                })
            }
        }
    }
}

/// `_Menu`: a 44-high row, hairlines top and bottom, the label on the leading
/// side and the value in `inactiveGray` on the trailing side, the whole row
/// the tap target that opens the sheet (`GestureDetector(onTap: ...)`).
fn menu_row(
    id: u64,
    label: String,
    value: String,
    label_color: Color,
    value_color: Color,
    handle: StateHandle<PickerDemoState>,
    kind: SheetKind,
) -> AnyWidget {
    let handlers = PointerHandlers::new().with_tap(move |_| {
        handle.set_state(move |state| state.open = Some(kind));
    });
    leaf(move || {
        // Upstream's `Border(top:, bottom:)` hairlines (width 0) are one
        // logical pixel here, cupertino.rs's hairline convention.
        let hairline = || Container::new().with_height(1.0).with_color(value_color);
        let row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .push(
                Text::new(label.clone())
                    .with_size(17.0)
                    .with_color(label_color),
            )
            .push(
                Text::new(value.clone())
                    .with_size(17.0)
                    .with_color(value_color)
                    .with_soft_wrap(false)
                    .with_max_lines(1),
            );
        Pointer::new(
            id,
            Container::new().with_height(MENU_HEIGHT).with_child(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(hairline())
                    .push_flex(FlexChild::expanded(
                        Container::new()
                            .with_padding(EdgeInsets::symmetric(16.0, 0.0))
                            .with_child(row),
                        1,
                    ))
                    .push(hairline()),
            ),
        )
        .with_handlers(handlers.clone())
    })
}

/// `_BottomPicker` and its child: the 216-high sheet on the theme's
/// `systemBackground`, bottom-aligned, holding the row's wheels.
fn bottom_picker(
    state: &PickerDemoState,
    handle: StateHandle<PickerDemoState>,
    theme: &CupertinoTheme,
    kind: SheetKind,
) -> AnyWidget {
    let background = theme.resolve(CupertinoColors::SYSTEM_BACKGROUND);
    let wheels = sheet_wheels(state, handle, background, kind);
    single(wheels, move |inner| {
        Box::new(Align::new(
            Alignment::BOTTOM_CENTER,
            Container::new()
                .with_height(BOTTOM_PICKER_HEIGHT)
                // `padding: const EdgeInsets.only(top: 6)`.
                .with_padding(EdgeInsets::only(0.0, 6.0, 0.0, 0.0))
                .with_color(background)
                .with_child(inner),
        ))
    })
}

/// The open sheet's wheels, one `CupertinoPicker` per field of the row's
/// value (see the header for the mapping from upstream's pickers).
fn sheet_wheels(
    state: &PickerDemoState,
    handle: StateHandle<PickerDemoState>,
    background: Color,
    kind: SheetKind,
) -> AnyWidget {
    // One wheel: `CupertinoPicker::labels` at the shared extent, on the
    // sheet's background, starting from the current value. `select` writes
    // the picked index back, upstream's `onSelectedItemChanged`.
    fn wheel(
        id: u64,
        labels: Vec<String>,
        selected: usize,
        background: Color,
        handle: StateHandle<PickerDemoState>,
        select: fn(&mut PickerDemoState, usize),
    ) -> AnyWidget {
        stateful(
            CupertinoPicker::labels(id, ITEM_EXTENT, labels)
                .with_background_color(background)
                .with_initial_item(selected)
                .wired(handle, select),
        )
    }

    fn numbers(first: u32, last: u32, pad: bool) -> Vec<String> {
        (first..=last)
            .map(|n| {
                if pad {
                    format!("{n:02}")
                } else {
                    format!("{n}")
                }
            })
            .collect()
    }

    let hour_labels = numbers(1, 12, false);
    let minute_labels = numbers(0, 59, true);
    let period_labels = vec!["AM".to_string(), "PM".to_string()];
    let month_labels: Vec<String> = MONTHS.iter().map(|m| m.to_string()).collect();

    let wheels: Vec<AnyWidget> = match kind {
        // `CupertinoDatePickerMode.date`: month, day, year columns.
        SheetKind::Date => {
            let days = rustflutter::pickers::days_in_month(state.date.year, state.date.month);
            vec![
                wheel(
                    ids::DEMO_LOCAL + 50,
                    month_labels,
                    (state.date.month - 1) as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        state.date = clamp_day(Date {
                            month: index as u32 + 1,
                            ..state.date
                        });
                    },
                ),
                wheel(
                    ids::DEMO_LOCAL + 51,
                    numbers(1, days, false),
                    (state.date.day - 1) as usize,
                    background,
                    handle.clone(),
                    |state, index| state.date.day = index as u32 + 1,
                ),
                wheel(
                    ids::DEMO_LOCAL + 52,
                    (YEAR_MIN..=YEAR_MAX)
                        .map(|year| format!("{year}"))
                        .collect(),
                    (state.date.year - YEAR_MIN).clamp(0, YEAR_MAX - YEAR_MIN) as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        state.date = clamp_day(Date {
                            year: YEAR_MIN + index as i32,
                            ..state.date
                        });
                    },
                ),
            ]
        }
        // `CupertinoDatePickerMode.time`: hour, minute, AM/PM columns.
        SheetKind::Time => {
            let (hour12, period) = hour12_and_period(state.time.0);
            vec![
                wheel(
                    ids::DEMO_LOCAL + 53,
                    hour_labels,
                    (hour12 - 1) as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        let pm = state.time.0 >= 12;
                        state.time.0 = (index as u32 + 1) % 12 + if pm { 12 } else { 0 };
                    },
                ),
                wheel(
                    ids::DEMO_LOCAL + 54,
                    minute_labels,
                    state.time.1 as usize,
                    background,
                    handle.clone(),
                    |state, index| state.time.1 = index as u32,
                ),
                wheel(
                    ids::DEMO_LOCAL + 55,
                    period_labels,
                    (period == "PM") as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        let (hour12, _) = hour12_and_period(state.time.0);
                        state.time.0 = hour12 % 12 + if index == 1 { 12 } else { 0 };
                    },
                ),
            ]
        }
        // `CupertinoDatePickerMode.dateAndTime`: upstream's first column is
        // formatted dates; here a month wheel and a day wheel (header).
        SheetKind::DateTime => {
            let (date, hour, minute) = state.date_time;
            let days = rustflutter::pickers::days_in_month(date.year, date.month);
            let (hour12, period) = hour12_and_period(hour);
            vec![
                wheel(
                    ids::DEMO_LOCAL + 56,
                    month_labels,
                    (date.month - 1) as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        let (date, h, m) = state.date_time;
                        state.date_time = (
                            clamp_day(Date {
                                month: index as u32 + 1,
                                ..date
                            }),
                            h,
                            m,
                        );
                    },
                ),
                wheel(
                    ids::DEMO_LOCAL + 57,
                    numbers(1, days, false),
                    (date.day - 1) as usize,
                    background,
                    handle.clone(),
                    |state, index| state.date_time.0.day = index as u32 + 1,
                ),
                wheel(
                    ids::DEMO_LOCAL + 58,
                    hour_labels,
                    (hour12 - 1) as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        let pm = state.date_time.1 >= 12;
                        state.date_time.1 = (index as u32 + 1) % 12 + if pm { 12 } else { 0 };
                    },
                ),
                wheel(
                    ids::DEMO_LOCAL + 59,
                    minute_labels,
                    minute as usize,
                    background,
                    handle.clone(),
                    |state, index| state.date_time.2 = index as u32,
                ),
                wheel(
                    ids::DEMO_LOCAL + 60,
                    period_labels,
                    (period == "PM") as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        let (hour12, _) = hour12_and_period(state.date_time.1);
                        state.date_time.1 = hour12 % 12 + if index == 1 { 12 } else { 0 };
                    },
                ),
            ]
        }
        // `CupertinoTimerPicker`: hour, minute, second columns; the minute
        // and second wheels read two digits, as upstream's do.
        SheetKind::Timer => {
            let (hours, minutes, seconds) =
                (state.timer / 3600, state.timer / 60 % 60, state.timer % 60);
            vec![
                wheel(
                    ids::DEMO_LOCAL + 61,
                    numbers(0, 23, false),
                    hours as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        state.timer = index as u64 * 3600 + state.timer % 3600;
                    },
                ),
                wheel(
                    ids::DEMO_LOCAL + 62,
                    numbers(0, 59, true),
                    minutes as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        state.timer =
                            state.timer / 3600 * 3600 + index as u64 * 60 + state.timer % 60;
                    },
                ),
                wheel(
                    ids::DEMO_LOCAL + 63,
                    numbers(0, 59, true),
                    seconds as usize,
                    background,
                    handle.clone(),
                    |state, index| {
                        state.timer = state.timer / 60 * 60 + index as u64;
                    },
                ),
            ]
        }
        // `_buildPicker`'s `CupertinoPicker`: the seven weekdays,
        // magnified and squeezed as upstream sets them.
        SheetKind::Weekday => vec![stateful(
            CupertinoPicker::labels(
                ids::DEMO_LOCAL + 64,
                ITEM_EXTENT,
                DAYS_OF_WEEK.iter().map(|d| d.to_string()).collect(),
            )
            .with_background_color(background)
            .with_magnification(1.22)
            .with_squeeze(1.2)
            .with_magnifier(true)
            .with_initial_item(state.weekday)
            .wired(handle.clone(), |state, index| state.weekday = index),
        )],
    };

    many(wheels, move |rendered| {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for wheel in rendered {
            row = row.push_flex(FlexChild::expanded(wheel, 1));
        }
        Box::new(row)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weekday_wheel_is_monday_through_sunday() {
        // `getDaysOfWeek` walks the current week from its Monday, so the
        // English names are always these seven in this order.
        assert_eq!(DAYS_OF_WEEK[0], "Monday");
        assert_eq!(DAYS_OF_WEEK[6], "Sunday");
        assert_eq!(DAYS_OF_WEEK.len(), 7);
    }

    #[test]
    fn the_date_format_is_yMMMMd() {
        assert_eq!(
            format_date(Date {
                year: 2026,
                month: 8,
                day: 17
            }),
            "August 17, 2026"
        );
        assert_eq!(
            format_date(Date {
                year: 2026,
                month: 1,
                day: 3
            }),
            "January 3, 2026"
        );
    }

    #[test]
    fn the_time_format_is_jm() {
        assert_eq!(format_time(9, 44), "9:44 AM");
        assert_eq!(format_time(0, 5), "12:05 AM");
        assert_eq!(format_time(12, 0), "12:00 PM");
        assert_eq!(format_time(23, 59), "11:59 PM");
    }

    #[test]
    fn the_date_time_format_is_yMMMd_add_jm() {
        assert_eq!(
            format_date_time(
                Date {
                    year: 2026,
                    month: 8,
                    day: 17
                },
                9,
                44
            ),
            "Aug 17, 2026 9:44 AM"
        );
    }

    #[test]
    fn the_timer_format_pads_minutes_and_seconds_only() {
        assert_eq!(format_timer(0), "0:00:00");
        assert_eq!(format_timer(3 * 3600 + 7 * 60 + 9), "3:07:09");
        assert_eq!(format_timer(25 * 3600), "25:00:00");
    }

    #[test]
    fn the_day_clamps_when_the_month_shortens() {
        // January 31, moved to February: upstream's day column shrinks and
        // the date follows.
        let date = clamp_day(Date {
            year: 2026,
            month: 2,
            day: 31,
        });
        assert_eq!(
            date,
            Date {
                year: 2026,
                month: 2,
                day: 28
            }
        );
        let date = clamp_day(Date {
            year: 2024,
            month: 2,
            day: 31,
        });
        assert_eq!(date.day, 29);
    }

    #[test]
    fn the_hour_wheels_round_trip_through_the_12_hour_clock() {
        for hour in 0..24 {
            let (hour12, period) = hour12_and_period(hour);
            let back = hour12 % 12 + if period == "PM" { 12 } else { 0 };
            assert_eq!(back, hour);
        }
    }

    #[test]
    fn only_one_sheet_is_open_at_a_time() {
        let mut state = PickerDemoState::default();
        assert_eq!(state.open, None);
        state.open = Some(SheetKind::Weekday);
        state.open = Some(SheetKind::Date);
        assert_eq!(state.open, Some(SheetKind::Date));
        state.open = None;
        assert_eq!(state.open, None);
    }
}
