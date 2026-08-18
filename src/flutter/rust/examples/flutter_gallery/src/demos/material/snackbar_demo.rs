// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/snackbar_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `SnackbarsDemo.build` is a `Scaffold` whose app bar titles the
//! demo and whose body is a single centered button; the scaffold and bar are
//! the demo page's chrome here (`src/pages/demo.rs`), so what remains is the
//! centered button and the snackbar it shows. The button hides the current
//! snackbar and shows "This is a snackbar." with an ACTION; pressing the
//! action shows "You pressed the snackbar action." with no action of its own.
//!
//! What upstream gets from `ScaffoldMessenger` and gets lost here:
//!
//! - Upstream's snackbar dismisses itself after `SnackBar.duration` (four
//!   seconds by default). The overlay below reproduces that on the frame
//!   clock, which is the only clock a demo has.
//! - Pressing the button while a snackbar is already up restarts it from the
//!   first message upstream (`hideCurrentSnackBar` then `showSnackBar`). Here
//!   the overlay stays mounted while `DemoState::snackbar_open` holds, so its
//!   per-demo state -- which snackbar it is on -- survives the second press.
//!   The launcher cannot reach that state without a new field on the shared
//!   `DemoState`, which this batch does not own.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::widgets::{Center, Empty};

use crate::app::{ids, GalleryState};

use super::DemoState;

/// How long a snackbar stays up, in frame-clock microseconds. Upstream's
/// default `SnackBar.duration`, `_kSnackBarDisplayDuration` (four seconds).
const SNACKBAR_DURATION_MICROS: i64 = 4_000_000;

/// Whether the snackbar shown at `shown_micros` has served its time.
fn should_dismiss(shown_micros: i64, frame_time_micros: i64) -> bool {
    frame_time_micros - shown_micros >= SNACKBAR_DURATION_MICROS
}

/// Which snackbar is on screen: the first, or the one its action showed.
/// Upstream's two `SnackBar`s in `SnackbarsDemo.build`.
fn snackbar_content(action_pressed: bool) -> (&'static str, Option<&'static str>) {
    if action_pressed {
        ("You pressed the snackbar action.", None)
    } else {
        ("This is a snackbar.", Some("ACTION"))
    }
}

/// The demo body: upstream's `Center(child: ElevatedButton(...))`.
pub(super) fn snackbar_launcher(
    state: &DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let _ = state;
    single(
        component(
            Button::new(ids::DEMO_LOCAL, "SHOW A SNACKBAR")
                .with_pressed(pressed == Some(ids::DEMO_LOCAL))
                .wired(handle, |s| &mut s.pressed, |s| s.demo.snackbar_open = true),
        ),
        |button| Box::new(Center::new(button)),
    )
}

/// The snackbar over the demo page, while `DemoState::snackbar_open` holds.
///
/// Stateful because the two-snackbar sequence and the four-second lifetime are
/// the overlay's own state upstream too: `ScaffoldMessenger` keeps them, not
/// the widget that asked for the snackbar.
pub(super) fn snackbar_overlay(handle: StateHandle<GalleryState>) -> AnyWidget {
    stateful(SnackbarOverlay { gallery: handle })
}

struct SnackbarOverlay {
    /// The way back to the shared state that decides whether this overlay is
    /// mounted at all, for the timer that takes it down.
    gallery: StateHandle<GalleryState>,
}

/// Upstream's half of the conversation `ScaffoldMessenger` would remember.
#[derive(Default)]
struct SnackbarOverlayState {
    /// Whether the first snackbar's ACTION was pressed.
    action_pressed: bool,
    /// When the current snackbar appeared, on the frame clock. `None` until
    /// the first advance after it showed, which is also when the clock is
    /// first known -- upstream's timer starts from `showSnackBar`, and the
    /// first frame after mounting is that moment here.
    shown_micros: Option<i64>,
}

impl StatefulComponent for SnackbarOverlay {
    type State = SnackbarOverlayState;

    fn advance(&self, state: &mut SnackbarOverlayState, frame_time_micros: i64) -> bool {
        let shown = *state.shown_micros.get_or_insert(frame_time_micros);
        if should_dismiss(shown, frame_time_micros) {
            // The timer upstream's `SnackBar` runs on its `duration`: the bar
            // leaves on its own, four seconds in.
            self.gallery.set_state(|s| s.demo.snackbar_open = false);
            return false;
        }
        true
    }

    fn build(
        &self,
        state: &SnackbarOverlayState,
        handle: StateHandle<SnackbarOverlayState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let (message, action) = snackbar_content(state.action_pressed);
        let mut bar = Snackbar::new(ids::DEMO_LOCAL + 1, message);
        if let Some(label) = action {
            bar = bar.with_action(label).wired(handle, |s| {
                // Upstream's `SnackBarAction.onPressed`: the current snackbar
                // goes and the confirmation takes its place, with a fresh
                // four seconds of its own.
                s.action_pressed = true;
                s.shown_micros = None;
            });
        }
        many(vec![component(bar)], |mut rendered| {
            let bar = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(rustflutter::widgets::Stack::new().push_positioned(
                bar,
                rustflutter::render::StackPosition {
                    left: Some(16.0),
                    right: Some(16.0),
                    bottom: Some(16.0),
                    ..Default::default()
                },
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_snackbar_has_the_action_the_second_does_not() {
        assert_eq!(
            snackbar_content(false),
            ("This is a snackbar.", Some("ACTION"))
        );
        assert_eq!(
            snackbar_content(true),
            ("You pressed the snackbar action.", None)
        );
    }

    #[test]
    fn a_snackbar_dismisses_after_four_seconds_and_not_before() {
        let shown = 1_000_000;
        assert!(!should_dismiss(shown, shown));
        assert!(!should_dismiss(shown, shown + SNACKBAR_DURATION_MICROS - 1));
        assert!(should_dismiss(shown, shown + SNACKBAR_DURATION_MICROS));
    }
}
