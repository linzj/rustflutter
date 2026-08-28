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
//! snackbar and shows "This is a snackbar." with an ACTION; pressing the action
//! shows "You pressed the snackbar action." with no action of its own.
//!
//! That is upstream's `ScaffoldMessenger`, and this demo now uses the
//! framework's: [`rustflutter::Messenger`] holds the queue and the overlay
//! entries, so `hideCurrentSnackBar` then `showSnackBar` is the same pair of
//! calls it is upstream.
//!
//! This file used to say what it lost by not having one -- the four-second
//! lifetime reimplemented on the frame clock, and a second press that could not
//! restart the sequence because the launcher could not reach the overlay's
//! state. Both are gone: the messenger owns the lifetime, and the button holds
//! the messenger.
//!
//! # The ACTION bar does not time out, and that is upstream
//!
//! `SnackBar`'s constructor ends `persist = persist ?? action != null`. A
//! message the reader is being asked to *act* on stays until they act or
//! dismiss it, because leaving would take the action with it. So the first
//! snackbar here waits, and the confirmation that replaces it -- which has no
//! action -- goes after four seconds.
//!
//! The old demo dismissed both after four seconds, including the one with the
//! ACTION. That was wrong about upstream and the rewiring is what found it.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::scaffold_messenger::SnackBarClosedReason;
use rustflutter::widgets::Center;
use rustflutter::{Messenger, OverlayHandle, SnackBarDisplay};

use crate::app::{ids, GalleryState};

use super::DemoState;

/// The id the messenger registers its scaffold under. Upstream asserts a
/// messenger has at least one registered scaffold, because a bar shown to
/// nobody would sit at the head of the queue for ever.
const DEMO_SCAFFOLD: u64 = 1;

/// Which snackbar is on screen: the first, or the one its action showed.
/// Upstream's two `SnackBar`s in `SnackbarsDemo.build`.
pub(super) fn snackbar_content(action_pressed: bool) -> (&'static str, Option<&'static str>) {
    if action_pressed {
        ("You pressed the snackbar action.", None)
    } else {
        ("This is a snackbar.", Some("ACTION"))
    }
}

/// How a bar with this content is displayed. The action is the whole of the
/// difference -- see the module docs.
pub(super) fn display_for(action: Option<&str>) -> SnackBarDisplay {
    match action {
        Some(_) => SnackBarDisplay::with_action(),
        None => SnackBarDisplay::new(),
    }
}

/// The demo body: upstream's `Center(child: ElevatedButton(...))`.
pub(super) fn snackbar_launcher(
    _state: &DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    stateful(SnackbarLauncher {
        pressed,
        gallery: handle,
    })
}

struct SnackbarLauncher {
    pressed: Option<u64>,
    gallery: StateHandle<GalleryState>,
}

/// The messenger. Which of the two messages is showing is the messenger's own
/// business now -- the bar that is up carries its own action handler, so there
/// is nothing left for this component to remember.
#[derive(Default)]
struct LauncherState {
    messenger: Option<Messenger>,
}

impl StatefulComponent for SnackbarLauncher {
    type State = LauncherState;

    /// The messenger's timer runs on the frame clock, as upstream's
    /// `_snackBarTimer` runs on a real one. Answering true keeps the frames
    /// coming while something is counting down.
    fn advance(&self, state: &mut LauncherState, frame_time_micros: i64) -> bool {
        match &state.messenger {
            Some(messenger) => messenger.advance(frame_time_micros),
            None => false,
        }
    }

    /// The bar goes when the page does.
    ///
    /// Upstream's `ScaffoldMessengerState.dispose` cancels `_snackBarTimer`,
    /// and the bar goes with the messenger's own subtree -- the gallery gives
    /// each demo page its own `ScaffoldMessenger` (`pages/demo.dart`'s
    /// `ScaffoldMessenger(child: DemoWrapper(...))`), so leaving the page ends
    /// the bar it was showing.
    ///
    /// Here the messenger presents into the *root* overlay, which is above the
    /// navigator and outlives this page by design -- so leaving is not enough
    /// on its own. Without this the bar stayed on screen over the demo list,
    /// with no clock left to time it out and no handle left to take it down:
    /// popping the route dropped the only `Messenger` there was.
    fn dispose(&self, state: &mut LauncherState) {
        if let Some(messenger) = &state.messenger {
            messenger.clear();
        }
    }

    fn build(
        &self,
        state: &LauncherState,
        handle: StateHandle<LauncherState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        // The messenger is made once, when the overlay is first in scope, and
        // kept: a fresh one each frame would lose the queue.
        if state.messenger.is_none() {
            if let Some(overlay) = OverlayHandle::of(context) {
                let messenger = Messenger::new(overlay, DEMO_SCAFFOLD);
                handle.set_state(move |state| state.messenger = Some(messenger));
            }
        }

        // The button's handlers are built by hand rather than through
        // `Button::wired`, which takes a `fn` and so cannot carry the
        // messenger. The pressed-highlight bookkeeping `wired` would have done
        // is here too.
        let messenger = state.messenger.clone();
        let gallery = self.gallery.clone();
        let id = ids::DEMO_LOCAL;
        let down = gallery.clone();
        let up = gallery.clone();
        let handlers = rustflutter::gestures::PointerHandlers::new()
            .with_pointer_down(move |_| {
                down.set_state(move |s| s.pressed = Some(id));
            })
            .with_pointer_up(move |_| {
                up.set_state(|s| s.pressed = None);
            })
            .with_tap(move |_| {
                let Some(messenger) = messenger.clone() else {
                    return;
                };
                // Upstream's `hideCurrentSnackBar` then `showSnackBar`: a
                // second press restarts from the first message.
                messenger.hide_current(SnackBarClosedReason::Hide);
                show(&messenger, false);
            });

        single(
            component(
                Button::new(id, "SHOW A SNACKBAR")
                    .with_pressed(self.pressed == Some(id))
                    .with_handlers(handlers),
            ),
            |button| Box::new(Center::new(button)),
        )
    }
}

/// Puts one of the two messages up, with the display rules its content implies.
fn show(messenger: &Messenger, action_pressed: bool) {
    let (message, action) = snackbar_content(action_pressed);
    let display = display_for(action);
    let messenger_for_action = messenger.clone();
    messenger.show_snack_bar_with(display, move || {
        let mut bar = Snackbar::new(ids::DEMO_LOCAL + 1, message);
        if let Some(label) = action {
            let messenger = messenger_for_action.clone();
            bar = bar.with_action(label).on_action(move || {
                // Upstream's `SnackBarAction.onPressed`: the current snackbar
                // goes -- with the reason that says why -- and the
                // confirmation takes its place, with four seconds of its own.
                messenger.hide_current(SnackBarClosedReason::Action);
                show(&messenger, true);
            });
        }
        component(bar)
    });
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
    fn the_bar_with_the_action_waits_and_the_confirmation_does_not() {
        // Upstream's `persist = persist ?? action != null`. The old demo timed
        // both out at four seconds, including the one carrying the ACTION,
        // which would take the action away while the reader was reaching for
        // it.
        let (_, first_action) = snackbar_content(false);
        let (_, second_action) = snackbar_content(true);

        assert!(
            display_for(first_action).persist,
            "the reader is being asked to act, so it stays"
        );
        assert!(
            !display_for(second_action).persist,
            "a confirmation has nothing to act on and goes on its own"
        );
    }

    #[test]
    fn the_confirmation_takes_upstreams_four_seconds() {
        let (_, action) = snackbar_content(true);
        assert_eq!(
            display_for(action).duration_micros,
            SnackBarDisplay::DEFAULT_DURATION_MICROS
        );
        assert_eq!(SnackBarDisplay::DEFAULT_DURATION_MICROS, 4_000_000);
    }
}
