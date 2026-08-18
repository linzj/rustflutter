// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/picker_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's one `PickerDemo` is keyed by `PickerDemoType` (date, time,
//! range) and the gallery shows each as its own configuration; the catalogue
//! here flattens every demo to one configuration (PORTING.md), so the three
//! sections stack on the one `pickers` page.
//!
//! Divergences, each also noted at its site:
//!
//! - **The overlay is the stage's, not a route's.** Upstream presents each
//!   picker as a `DialogRoute` over the whole demo screen
//!   (`_datePickerRoute`, `_timePickerRoute`, `_dateRangePickerRoute`). The
//!   framework's pickers return the overlay widget for the caller to stack
//!   (see `rustflutter::pickers`), and the page-level `overlay()` dispatch is
//!   shared code, so the overlay is stacked over this stage: the scrim covers
//!   the demo card rather than the window, and the stage grows to
//!   [`OVERLAY_HOST_HEIGHT`] while a picker is open so the dialog fits.
//! - **No restoration.** Upstream's `RestorationMixin`/`RestorableRouteFuture`
//!   machinery has no counterpart here; the state is plain component state.
//! - **UTC, not local.** `Date::today`/`TimeOfDay::now` read the UTC clock
//!   (the engine bridge has no local-time query), and the time label always
//!   uses the 12-hour clock, where upstream's `TimeOfDay.format(context)`
//!   reads the ambient `alwaysUse24HourFormat`.

use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent};
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex, RenderRef};
use rustflutter::widgets::{Center, Empty, Positioned, Stack};

use crate::app::ids;

use super::{caption, column};

/// The demo body for the `pickers` slug.
pub(super) fn stage() -> AnyWidget {
    stateful(PickerDemo)
}

/// Upstream's `PickerDemo`, state in `_PickerDemoState`.
struct PickerDemo;

/// Which picker's dialog is open, if any. Upstream this is which restorable
/// route future has been presented.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OpenPicker {
    Date,
    Time,
    Range,
}

/// Upstream's `_PickerDemoState`: `_fromDate`, `_fromTime`, `_startDate` and
/// `_endDate`, all starting at "now".
struct PickerDemoState {
    open: Option<OpenPicker>,
    from_date: Date,
    from_time: TimeOfDay,
    start_date: Date,
    end_date: Date,
    /// Which button is held; `Button::wired` wants the field.
    pressed: Option<u64>,
}

impl Default for PickerDemoState {
    fn default() -> PickerDemoState {
        let today = Date::today();
        PickerDemoState {
            open: None,
            from_date: today,
            from_time: TimeOfDay::now(),
            start_date: today,
            end_date: today,
            pressed: None,
        }
    }
}

/// Upstream's `_selectDate`: a null or unchanged selection is ignored.
fn select_date(current: Date, selected: Option<Date>) -> Date {
    match selected {
        Some(date) if date != current => date,
        _ => current,
    }
}

/// Upstream's `_selectTime`, with the same guard as [`select_date`].
fn select_time(current: TimeOfDay, selected: Option<TimeOfDay>) -> TimeOfDay {
    match selected {
        Some(time) if time != current => time,
        _ => current,
    }
}

/// `DateFormat.yMMMd()` for en_US: "Aug 17, 2026". The framework's
/// `format_medium_date` prefixes the weekday, which `yMMMd` does not.
fn format_abbrev_date(date: Date) -> String {
    const SHORT_MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {}, {}",
        SHORT_MONTHS[(date.month - 1) as usize],
        date.day,
        date.year
    )
}

/// The height the stage takes while a picker is open: the tallest dialog (the
/// calendar date picker's 568) plus air. See the module header for why the
/// overlay lives inside the stage at all.
const OVERLAY_HOST_HEIGHT: f32 = 600.0;

/// Hit-test ids local to this demo. Only one demo is on stage at a time, so
/// they share the `DEMO_LOCAL` block; the picker dialogs derive their child
/// ids from the base they are given (`id * 10 + n`, day cells `id * 10000 +
/// day`), so the three bases must not overlap each other's derived ranges.
const DATE_BUTTON: u64 = ids::DEMO_LOCAL;
const TIME_BUTTON: u64 = ids::DEMO_LOCAL + 1;
const RANGE_BUTTON: u64 = ids::DEMO_LOCAL + 2;
const DATE_DIALOG: u64 = ids::DEMO_LOCAL + 100;
const TIME_DIALOG: u64 = ids::DEMO_LOCAL + 101;
const RANGE_DIALOG: u64 = ids::DEMO_LOCAL + 102;

/// A label over a "SHOW PICKER" button, centred: upstream's
/// `Center(Column(mainAxisSize: min, [Text, SizedBox(16), ElevatedButton]))`.
fn picker_section(label: String, button: AnyWidget) -> AnyWidget {
    many(
        vec![component(Label::new(label)), button],
        move |rendered| {
            let mut flex = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                // Upstream's `SizedBox(height: 16)` between the text and the button.
                .with_spacing(16.0);
            for child in rendered {
                flex = flex.push(child);
            }
            Box::new(Center::new(flex))
        },
    )
}

impl StatefulComponent for PickerDemo {
    type State = PickerDemoState;

    fn build(
        &self,
        state: &PickerDemoState,
        handle: StateHandle<PickerDemoState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        // `Button::wired` takes a bare `fn`, so each button's opener is its
        // own non-capturing closure.
        let show_picker = |id: u64, open: fn(&mut PickerDemoState)| {
            component(
                Button::new(id, "SHOW PICKER")
                    .with_style(ButtonStyle::Filled)
                    .with_pressed(state.pressed == Some(id))
                    .wired(handle.clone(), |s| &mut s.pressed, open),
            )
        };

        // The `_labelText` getter, once per section. The time label is always
        // the 12-hour clock; see the module header.
        let range_label = format!(
            "{} - {}",
            format_abbrev_date(state.start_date),
            format_abbrev_date(state.end_date),
        );
        let body = column(
            vec![
                caption("Date Picker"),
                picker_section(
                    format_abbrev_date(state.from_date),
                    show_picker(DATE_BUTTON, |s| s.open = Some(OpenPicker::Date)),
                ),
                caption("Time Picker"),
                picker_section(
                    state.from_time.format(false),
                    show_picker(TIME_BUTTON, |s| s.open = Some(OpenPicker::Time)),
                ),
                caption("Date Range Picker"),
                picker_section(
                    range_label,
                    show_picker(RANGE_BUTTON, |s| s.open = Some(OpenPicker::Range)),
                ),
            ],
            16.0,
        );

        let overlay: Option<AnyWidget> = match state.open {
            Some(OpenPicker::Date) => {
                // `_datePickerRoute`: initialDate is the current value, and
                // the bounds are 2015 to 2100.
                let confirm = handle.clone();
                let cancel = handle.clone();
                Some(show_date_picker(
                    DatePickerDialog::new(
                        DATE_DIALOG,
                        Date::new(2015, 1, 1),
                        Date::new(2100, 12, 31),
                    )
                    .with_initial_date(Some(state.from_date))
                    .with_on_confirm(move |date| {
                        confirm.set_state(move |s| {
                            s.from_date = select_date(s.from_date, Some(date));
                            s.open = None;
                        });
                    })
                    .with_on_cancel(move || {
                        cancel.set_state(|s| s.open = None);
                    }),
                ))
            }
            Some(OpenPicker::Time) => {
                // `_timePickerRoute`: the initial time is the current value.
                let confirm = handle.clone();
                let cancel = handle.clone();
                Some(show_time_picker(
                    TimePickerDialog::new(TIME_DIALOG, state.from_time)
                        .with_on_confirm(move |time| {
                            confirm.set_state(move |s| {
                                s.from_time = select_time(s.from_time, Some(time));
                                s.open = None;
                            });
                        })
                        .with_on_cancel(move || {
                            cancel.set_state(|s| s.open = None);
                        }),
                ))
            }
            Some(OpenPicker::Range) => {
                // `_dateRangePickerRoute`: the bounds are five years either
                // side of this one, and no initial range is passed.
                let this_year = Date::today().year;
                let confirm = handle.clone();
                let cancel = handle.clone();
                Some(show_date_range_picker(
                    DateRangePickerDialog::new(
                        RANGE_DIALOG,
                        Date::new(this_year - 5, 1, 1),
                        Date::new(this_year + 5, 1, 1),
                    )
                    .with_on_confirm(move |range| {
                        confirm.set_state(move |s| {
                            // `_selectDateRange`.
                            s.start_date = range.start;
                            s.end_date = range.end;
                            s.open = None;
                        });
                    })
                    .with_on_cancel(move || {
                        cancel.set_state(|s| s.open = None);
                    }),
                ))
            }
            None => None,
        };

        match overlay {
            None => body,
            Some(overlay) => many(vec![body, overlay], move |mut rendered| {
                let overlay = rendered.pop().unwrap_or_else(|| RenderRef::new(Empty));
                let body = rendered.pop().unwrap_or_else(|| RenderRef::new(Empty));
                Box::new(
                    Container::new()
                        .with_height(OVERLAY_HOST_HEIGHT)
                        .with_child(
                            Stack::new()
                                .push(Center::new(body))
                                .push_positioned(overlay, Positioned::fill()),
                        ),
                )
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox};

    #[test]
    fn the_date_label_is_yMMMd() {
        assert_eq!(format_abbrev_date(Date::new(2026, 8, 17)), "Aug 17, 2026");
        assert_eq!(format_abbrev_date(Date::new(2015, 1, 1)), "Jan 1, 2015");
    }

    #[test]
    fn a_null_or_unchanged_selection_is_ignored() {
        let date = Date::new(2026, 8, 17);
        assert_eq!(select_date(date, None), date);
        assert_eq!(select_date(date, Some(date)), date);
        assert_eq!(
            select_date(date, Some(Date::new(2026, 8, 18))),
            Date::new(2026, 8, 18)
        );

        let time = TimeOfDay::new(9, 30);
        assert_eq!(select_time(time, None), time);
        assert_eq!(select_time(time, Some(time)), time);
        assert_eq!(
            select_time(time, Some(TimeOfDay::new(10, 0))),
            TimeOfDay::new(10, 0)
        );
    }

    #[test]
    fn the_stage_lays_out() {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), stage()));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(460.0, 820.0));
        assert!(size.width > 0.0 && size.height > 0.0);
        assert!(
            size.height < OVERLAY_HOST_HEIGHT,
            "closed, the stage is its content"
        );
    }
}
