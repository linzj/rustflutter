// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_picker_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoPickerDemo` is five `_Menu` rows -- Date, Time, Date
//! and Time, Timer, Picker -- each opening a `showCupertinoModalPopup` bottom
//! sheet (`_BottomPicker`, 216 high) with a picker in it, and showing the
//! picked value in the row. So is this one: the first four sheets hold a
//! [`rustflutter::CupertinoDatePicker`] or a
//! [`rustflutter::CupertinoTimerPicker`], the fifth a plain
//! [`rustflutter::CupertinoPicker`], and the sheet is put up by
//! [`rustflutter::show_cupertino_modal_popup`].
//!
//! This file used to build the date, time and timer sheets out of rows of
//! plain wheels -- one per field -- because the framework had no date picker;
//! it had its own scrim and its own open flag, because there was no modal
//! popup either. Both are gone.
//!
//! Divergences, each also marked at its site:
//!
//! * **`DateTime.now()` stands in as UTC.** The engine bridge has no
//!   local-time query -- nothing in `services/` or the platform channels
//!   reports a zone offset -- so the clock the demo opens on is UTC's, the
//!   same substitution [`rustflutter::pickers::Date::today`] documents.
//! * **The stage is height-bounded** ([`DEMO_HEIGHT`]); upstream's `ListView`
//!   body fills the screen, and five rows never need to scroll here.

use rustflutter::dialogs::show_cupertino_modal_popup;
use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::pickers::Date;
use rustflutter::prelude::*;
use rustflutter::render::{
    CrossAxisAlignment, FlexChild, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use rustflutter::theatre::OverlayHandle;
use rustflutter::widgets::Pointer;
use rustflutter::{
    CupertinoDatePicker, CupertinoDatePickerMode, CupertinoTimerPicker, PickerDateTime,
};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The stage's fixed height, standing in for the demo screen (see the header).
const DEMO_HEIGHT: f32 = 420.0;

/// `_BottomPicker`'s height.
const BOTTOM_PICKER_HEIGHT: f32 = 216.0;

/// `_Menu`'s height.
const MENU_HEIGHT: f32 = 44.0;

/// The weekday wheel's item extent: upstream's `CupertinoPicker(itemExtent:
/// 32.0)` in `_buildPicker`.
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

/// Full month names, for `DateFormat.yMMMMd`.
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

/// Upstream's `_CupertinoPickerDemoState`: the five picked values.
///
/// Upstream's `Navigator` stack held the sixth thing this used to keep -- which
/// sheet is open -- and the theatre holds it now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PickerDemoState {
    /// `timer`, the countdown picker's value, in seconds.
    timer: i64,
    /// `date`, the date-mode picker's value.
    date: PickerDateTime,
    /// `time`, the time-mode picker's value.
    time: PickerDateTime,
    /// `dateTime`, the dateAndTime-mode picker's value.
    date_time: PickerDateTime,
    /// `_selectedWeekday`.
    weekday: usize,
}

impl Default for PickerDemoState {
    fn default() -> PickerDemoState {
        // Upstream's three `DateTime.now()`s, in UTC (see the header).
        let now = now_utc();
        PickerDemoState {
            timer: 0,
            date: now,
            time: now,
            date_time: now,
            weekday: 0,
        }
    }
}

/// `DateTime.now()`, as far as the engine bridge can answer it.
fn now_utc() -> PickerDateTime {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    PickerDateTime::new(
        Date::today(),
        (seconds.div_euclid(3600).rem_euclid(24)) as u32,
        (seconds.div_euclid(60).rem_euclid(60)) as u32,
    )
}

/// `DateFormat.yMMMMd().format`: "August 27, 2026".
fn format_date(value: PickerDateTime) -> String {
    format!(
        "{} {}, {}",
        MONTHS[(value.date.month - 1) as usize],
        value.date.day,
        value.date.year
    )
}

/// `DateFormat.jm().format`: "1:40 PM".
///
/// The space before the day period is **U+202F**, a narrow no-break space, not
/// an ordinary one: that is what CLDR's `en_US` `jm` pattern has used since
/// release 42, and it is visibly narrower on the screen than the space in
/// "Aug 27, 2026" beside it.
fn format_time(value: PickerDateTime) -> String {
    let (hour12, period) = hour12_and_period(value.hour);
    format!("{hour12}:{:02}\u{202f}{period}", value.minute)
}

/// `DateFormat.yMMMd().add_jm().format`: "Aug 27, 2026 1:40 PM".
fn format_date_time(value: PickerDateTime) -> String {
    format!(
        "{} {}, {} {}",
        MONTHS_ABBR[(value.date.month - 1) as usize],
        value.date.day,
        value.date.year,
        format_time(value),
    )
}

/// Upstream's timer row: hours unpadded, minutes and seconds two digits
/// (`timer.inHours` / `padLeft(2, '0')`).
fn format_timer(seconds: i64) -> String {
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
        let background = theme.resolve(CupertinoColors::SYSTEM_BACKGROUND);
        // `showCupertinoModalPopup` reaches the root navigator upstream, which
        // is why its barrier covers the application rather than the page; the
        // theatre's root overlay is the same surface. Taken here because the
        // row's tap handler runs without a context.
        let overlay = OverlayHandle::of(context);

        // The five rows, upstream's build order: date, time, dateAndTime,
        // countdown timer, picker.
        let rows = vec![
            menu_row(
                ids::DEMO_LOCAL + 40,
                l10n.demo_cupertino_picker_date().to_string(),
                format_date(state.date),
                label_color,
                value_color,
                {
                    let (overlay, handle) = (overlay.clone(), handle.clone());
                    let initial = state.date;
                    move || {
                        let handle = handle.clone();
                        show_sheet(&overlay, background, move || {
                            let handle = handle.clone();
                            stateful(
                                CupertinoDatePicker::new(CupertinoDatePickerMode::Date)
                                    .with_id(ids::DEMO_LOCAL + 50)
                                    .with_initial(initial)
                                    .with_background_color(background)
                                    .with_on_changed(move |value| {
                                        handle.set_state(move |state| state.date = value);
                                    }),
                            )
                        });
                    }
                },
            ),
            menu_row(
                ids::DEMO_LOCAL + 41,
                l10n.demo_cupertino_picker_time().to_string(),
                format_time(state.time),
                label_color,
                value_color,
                {
                    let (overlay, handle) = (overlay.clone(), handle.clone());
                    let initial = state.time;
                    move || {
                        let handle = handle.clone();
                        show_sheet(&overlay, background, move || {
                            let handle = handle.clone();
                            stateful(
                                CupertinoDatePicker::new(CupertinoDatePickerMode::Time)
                                    .with_id(ids::DEMO_LOCAL + 60)
                                    .with_initial(initial)
                                    .with_background_color(background)
                                    .with_on_changed(move |value| {
                                        handle.set_state(move |state| state.time = value);
                                    }),
                            )
                        });
                    }
                },
            ),
            menu_row(
                ids::DEMO_LOCAL + 42,
                l10n.demo_cupertino_picker_date_time().to_string(),
                format_date_time(state.date_time),
                label_color,
                value_color,
                {
                    let (overlay, handle) = (overlay.clone(), handle.clone());
                    let initial = state.date_time;
                    move || {
                        let handle = handle.clone();
                        show_sheet(&overlay, background, move || {
                            let handle = handle.clone();
                            stateful(
                                CupertinoDatePicker::new(CupertinoDatePickerMode::DateAndTime)
                                    .with_id(ids::DEMO_LOCAL + 70)
                                    .with_initial(initial)
                                    .with_background_color(background)
                                    .with_on_changed(move |value| {
                                        handle.set_state(move |state| state.date_time = value);
                                    }),
                            )
                        });
                    }
                },
            ),
            menu_row(
                ids::DEMO_LOCAL + 43,
                l10n.demo_cupertino_picker_timer().to_string(),
                format_timer(state.timer),
                label_color,
                value_color,
                {
                    let (overlay, handle) = (overlay.clone(), handle.clone());
                    let initial = state.timer;
                    move || {
                        let handle = handle.clone();
                        show_sheet(&overlay, background, move || {
                            let handle = handle.clone();
                            stateful(
                                CupertinoTimerPicker::new(initial)
                                    .with_id(ids::DEMO_LOCAL + 80)
                                    .with_background_color(background)
                                    .with_on_changed(move |seconds| {
                                        handle.set_state(move |state| state.timer = seconds);
                                    }),
                            )
                        });
                    }
                },
            ),
            menu_row(
                ids::DEMO_LOCAL + 44,
                l10n.demo_cupertino_picker().to_string(),
                DAYS_OF_WEEK[state.weekday].to_string(),
                label_color,
                value_color,
                {
                    let (overlay, handle) = (overlay.clone(), handle.clone());
                    let initial = state.weekday;
                    move || {
                        let handle = handle.clone();
                        show_sheet(&overlay, background, move || {
                            // `_buildPicker`'s `CupertinoPicker`: magnified and
                            // squeezed as upstream sets it, the seven weekdays
                            // centred.
                            stateful(
                                CupertinoPicker::labels(
                                    ids::DEMO_LOCAL + 90,
                                    ITEM_EXTENT,
                                    DAYS_OF_WEEK.iter().map(|day| day.to_string()).collect(),
                                )
                                .with_background_color(background)
                                .with_magnification(1.22)
                                .with_squeeze(1.2)
                                .with_magnifier(true)
                                .with_initial_item(initial)
                                .wired(handle.clone(), |state, index| state.weekday = index),
                            )
                        });
                    }
                },
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

        component(
            CupertinoPageScaffold::new(body).with_navigation_bar(component(
                // `automaticallyImplyLeading: false`: no back button.
                CupertinoNavigationBar::new().with_middle(l10n.demo_cupertino_picker_title()),
            )),
        )
    }
}

/// Upstream's `_showDemoPicker`: the picker wrapped in `_BottomPicker` and
/// handed to `showCupertinoModalPopup`.
fn show_sheet(
    overlay: &Option<std::rc::Rc<OverlayHandle>>,
    background: Color,
    picker: impl Fn() -> AnyWidget + 'static,
) {
    let Some(overlay) = overlay.clone() else {
        return;
    };
    show_cupertino_modal_popup(overlay, move || bottom_picker(background, picker()));
}

/// `_BottomPicker`: the 216-high sheet on the theme's `systemBackground`,
/// padded 6 at the top, holding the picker.
///
/// The `Pointer` around it is upstream's `GestureDetector(onTap: () {})`, whose
/// comment is the whole of its purpose: "Blocks taps from propagating to the
/// modal sheet and popping." Without it a tap on the sheet's own background
/// reaches the barrier underneath and closes the sheet.
fn bottom_picker(background: Color, picker: AnyWidget) -> AnyWidget {
    single(picker, move |inner| {
        Box::new(
            Pointer::new(
                ids::DEMO_LOCAL + 45,
                Container::new()
                    .with_height(BOTTOM_PICKER_HEIGHT)
                    // `padding: const EdgeInsets.only(top: 6)`.
                    .with_padding(EdgeInsets::only(0.0, 6.0, 0.0, 0.0))
                    .with_color(background)
                    .with_child(inner),
            )
            .with_handlers(PointerHandlers::new().with_tap(|_| {})),
        )
    })
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
    open: impl Fn() + 'static,
) -> AnyWidget {
    let handlers = PointerHandlers::new().with_tap(move |_| open());
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

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::cupertino_pickers::{
        DATE_PICKER_HEIGHT, DATE_PICKER_ITEM_EXTENT, DATE_PICKER_MAGNIFICATION,
        DATE_PICKER_PAD_SIZE, DATE_PICKER_SQUEEZE,
    };

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> PickerDateTime {
        PickerDateTime::new(Date { year, month, day }, hour, minute)
    }

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
        assert_eq!(format_date(at(2026, 8, 17, 0, 0)), "August 17, 2026");
        assert_eq!(format_date(at(2026, 1, 3, 0, 0)), "January 3, 2026");
    }

    #[test]
    fn the_time_format_is_jm() {
        assert_eq!(format_time(at(2026, 8, 17, 9, 44)), "9:44\u{202f}AM");
        assert_eq!(format_time(at(2026, 8, 17, 0, 5)), "12:05\u{202f}AM");
        assert_eq!(format_time(at(2026, 8, 17, 12, 0)), "12:00\u{202f}PM");
        assert_eq!(format_time(at(2026, 8, 17, 23, 59)), "11:59\u{202f}PM");
    }

    #[test]
    fn the_date_time_format_is_yMMMd_add_jm() {
        assert_eq!(
            format_date_time(at(2026, 8, 17, 9, 44)),
            "Aug 17, 2026 9:44\u{202f}AM"
        );
    }

    #[test]
    fn the_timer_format_pads_minutes_and_seconds_only() {
        assert_eq!(format_timer(0), "0:00:00");
        assert_eq!(format_timer(3 * 3600 + 7 * 60 + 9), "3:07:09");
        assert_eq!(format_timer(25 * 3600), "25:00:00");
    }

    #[test]
    fn the_hour_wheels_round_trip_through_the_12_hour_clock() {
        for hour in 0..24 {
            let (hour12, period) = hour12_and_period(hour);
            let back = hour12 % 12 + if period == "PM" { 12 } else { 0 };
            assert_eq!(back, hour);
        }
    }

    /// The sheet is exactly as tall as the picker inside it, which is what
    /// makes `_BottomPicker`'s 216 and `_kPickerHeight`'s 216 the same number
    /// rather than a coincidence.
    #[test]
    fn the_sheet_is_the_pickers_own_height() {
        assert_eq!(BOTTOM_PICKER_HEIGHT, DATE_PICKER_HEIGHT);
    }

    /// The demo's own wheel is a generic picker and the other four are date
    /// pickers, which are denser: upstream's `_kSqueeze` is 1.25 against the
    /// 1.2 this demo asks for, and its magnification 2.35/2.1 against 1.22.
    #[test]
    fn the_weekday_wheel_is_not_a_date_pickers_wheel() {
        assert_eq!(ITEM_EXTENT, DATE_PICKER_ITEM_EXTENT);
        assert!(DATE_PICKER_SQUEEZE > 1.2);
        assert!(DATE_PICKER_MAGNIFICATION < 1.22);
        assert_eq!(DATE_PICKER_PAD_SIZE, 12.0);
    }
}
