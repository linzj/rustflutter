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
//! - **The pickers go up through the framework, over the window.** Upstream
//!   presents each as a `DialogRoute` over the whole demo screen
//!   (`_datePickerRoute`, `_timePickerRoute`, `_dateRangePickerRoute`), and so
//!   does this: [`rustflutter::show_date_picker`] and its two siblings are the
//!   imperative calls, and they put the dialog in the application's overlay
//!   behind a real barrier.
//!
//!   This paragraph used to say the opposite. The stage stacked the picker over
//!   itself, so the scrim covered the demo card rather than the window, and the
//!   stage had to grow to a hard-coded 600 while a picker was open -- tall
//!   enough for the calendar -- because a dialog inside the card could not
//!   exceed it. All of that is gone, along with the constant.
//! - **No restoration.** Upstream's `RestorationMixin`/`RestorableRouteFuture`
//!   machinery has no counterpart here; the state is plain component state.
//! - **UTC, not local.** `Date::today`/`TimeOfDay::now` read the UTC clock
//!   (the engine bridge has no local-time query), and the time label always
//!   uses the 12-hour clock, where upstream's `TimeOfDay.format(context)`
//!   reads the ambient `alwaysUse24HourFormat`.

use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent};
use rustflutter::prelude::*;
use rustflutter::OverlayHandle;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::Center;

use crate::app::ids;

use super::{caption, column};

/// The demo body for the `pickers` slug.
pub(super) fn stage() -> AnyWidget {
    stateful(PickerDemo)
}

/// Upstream's `PickerDemo`, state in `_PickerDemoState`.
struct PickerDemo;

/// Which picker a button opens. Upstream this is which restorable route future
/// it presents.
///
/// It used to also be *which one is open*, kept in `PickerDemoState`, because
/// the demo had to build the open dialog itself every frame. The overlay holds
/// the open dialog now, so this is only the choice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OpenPicker {
    Date,
    Time,
    Range,
}

/// Upstream's `_PickerDemoState`: `_fromDate`, `_fromTime`, `_startDate` and
/// `_endDate`, all starting at "now".
struct PickerDemoState {
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
        context: &mut BuildContext,
    ) -> AnyWidget {
        let overlay = OverlayHandle::of(context);

        // Each button opens its own picker, which means capturing the overlay
        // and the state handle -- so the handlers are built rather than going
        // through `Button::wired`, whose action is a bare `fn`.
        // What each picker opens on, read from the state this build saw. A
        // press handler runs between builds, and a state change is what causes
        // the next one -- so these are current at the moment of the press.
        let opens_on = OpensOn {
            date: state.from_date,
            time: state.from_time,
        };
        let show_picker = |id: u64, which: OpenPicker| {
            let overlay = overlay.clone();
            let opener = handle.clone();
            let presser = handle.clone();
            component(
                Button::new(id, "SHOW PICKER")
                    .with_style(ButtonVariant::Filled)
                    .with_pressed(state.pressed == Some(id))
                    .with_handlers(
                        rustflutter::gestures::PointerHandlers::new()
                            .with_tap(move |_| {
                                if let Some(overlay) = overlay.clone() {
                                    open_picker(overlay, which, opens_on, opener.clone());
                                }
                            })
                            .with_press_change(move |down| {
                                presser.set_state(move |s| {
                                    s.pressed = if down { Some(id) } else { None };
                                });
                            }),
                    ),
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
                    show_picker(DATE_BUTTON, OpenPicker::Date),
                ),
                caption("Time Picker"),
                picker_section(
                    state.from_time.format(false),
                    show_picker(TIME_BUTTON, OpenPicker::Time),
                ),
                caption("Date Range Picker"),
                picker_section(
                    range_label,
                    show_picker(RANGE_BUTTON, OpenPicker::Range),
                ),
            ],
            16.0,
        );

        body
    }
}

/// Puts one picker up. Upstream's three `DialogRoute`s, as three calls.
///
/// Each of the framework's `show_*_picker` functions closes its own dialog when
/// it answers, so there is nothing here to take it down -- which is the whole
/// difference from `pickers::*_surface`, and the part every caller of those had
/// to write.
fn open_picker(
    overlay: std::rc::Rc<OverlayHandle>,
    which: OpenPicker,
    opens_on: OpensOn,
    handle: StateHandle<PickerDemoState>,
) {
    let opened = match which {
        OpenPicker::Date => {
            // `_datePickerRoute`: initialDate is the current value, and the
            // bounds are 2015 to 2100.
            rustflutter::show_date_picker(
                overlay,
                DatePickerDialog::new(DATE_DIALOG, Date::new(2015, 1, 1), Date::new(2100, 12, 31))
                    .with_initial_date(Some(opens_on.date)),
                move |date| {
                    handle.set_state(move |s| s.from_date = select_date(s.from_date, date));
                },
            )
        }
        OpenPicker::Time => {
            // `_timePickerRoute`: the initial time is the current value.
            rustflutter::show_time_picker(
                overlay,
                TimePickerDialog::new(TIME_DIALOG, opens_on.time),
                move |time| {
                    handle.set_state(move |s| s.from_time = select_time(s.from_time, time));
                },
            )
        }
        OpenPicker::Range => {
            // `_dateRangePickerRoute`: the bounds are five years either side of
            // this one, and no initial range is passed.
            let this_year = Date::today().year;
            rustflutter::show_date_range_picker(
                overlay,
                DateRangePickerDialog::new(
                    RANGE_DIALOG,
                    Date::new(this_year - 5, 1, 1),
                    Date::new(this_year + 5, 1, 1),
                ),
                move |range| {
                    // `_selectDateRange`, which takes both ends or neither.
                    let Some(range) = range else {
                        return;
                    };
                    handle.set_state(move |s| {
                        s.start_date = range.start;
                        s.end_date = range.end;
                    });
                },
            )
        }
    };
    // A picker that could not find an overlay shows nothing rather than
    // pretending; `OverlayHandle::of` already returned one, so this is only
    // true if the overlay went away between the press and the call.
    let _ = opened;
}

/// What a picker opens on: upstream's `initialDate` and `initialTime`, which
/// are the demo's current values.
///
/// Carried from `build` rather than read at the press, because `StateHandle`
/// writes and does not read -- and it does not need to. A state change is what
/// causes the next build, so the values a handler captured are the ones on
/// screen when it runs.
#[derive(Clone, Copy)]
struct OpensOn {
    date: Date,
    time: TimeOfDay,
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
    fn the_stage_does_not_grow_to_hold_a_picker() {
        // What the rewiring bought, and the reason the old shape needed a
        // 600-tall host: a picker used to be a child of this stage, so the
        // stage had to be at least as tall as the tallest dialog while one was
        // open, and its scrim covered the demo card rather than the window.
        // The picker is in the application's overlay now, so the stage is its
        // own content and nothing else.
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), stage()));
        let mut root = tree.build_render_tree().expect("a root");
        let closed = root.layout(BoxConstraints::loose(460.0, 820.0));

        // Rebuilding does not change it -- there is no open-picker state left
        // on the stage for it to grow around.
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("a root");
        assert_eq!(root.layout(BoxConstraints::loose(460.0, 820.0)), closed);
    }

    #[test]
    fn the_stage_lays_out() {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), stage()));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(460.0, 820.0));
        assert!(size.width > 0.0 && size.height > 0.0);
        // The stage is its content, open or closed. It used to have to be 600
        // tall while a picker was up, because the picker was inside it.
        assert!(size.height < 600.0, "the stage is its content");
    }
}
