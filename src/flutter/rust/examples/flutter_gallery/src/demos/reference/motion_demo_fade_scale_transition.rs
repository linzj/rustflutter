// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/reference/motion_demo_fade_scale_transition.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `FadeScaleTransitionDemo` is a `Scaffold` whose app bar titles
//! the pattern ("Fade" / "(Modal and FAB)"), whose body is empty, whose
//! floating action button is wrapped in a `FadeScaleTransition` driven by a
//! 150ms/75ms controller, and whose bottom bar carries two buttons: "SHOW
//! MODAL" opens an `AlertDialog` through `showModal`'s
//! `FadeScaleTransitionConfiguration`, and "HIDE FAB"/"SHOW FAB" runs the
//! controller backwards or forwards. The transition itself is the
//! `animations` package's `FadeScaleTransition`, reproduced here by
//! [`transitions::fade_scale_enter`] and [`transitions::fade_scale_exit`].
//!
//! Divergences, each also marked at its site:
//!
//! * The demo is one of six sections stacked on the single `motion` stage
//!   (see `mod.rs`'s header), so its screen is height-bounded
//!   ([`BODY_HEIGHT`]) and its modal renders in the section's own stack
//!   rather than on a route -- `showModal` has no counterpart, the same
//!   presentation the cupertino demos use.
//! * The controller's two durations are ticked by hand ([`tick`]): the
//!   framework's `Controller` carries a single duration, and upstream's
//!   `reverseDuration` (75ms against 150ms) is the point of the pattern's
//!   exit.
//! * The FAB and the alert dialog are drawn locally: the framework has no
//!   `FloatingActionButton`, and its `Dialog` insists on a title where
//!   upstream's `AlertDialog` here has only content. The FAB's `onPressed`
//!   is empty upstream, so the drawn button is unwired.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, MainAxisAlignment, MainAxisSize, RenderFlex, StackPosition,
};
use rustflutter::widgets::{Align, Center, Empty, Opacity, Pointer, Positioned, Stack, Transform};

use crate::app::ids;
use crate::data::demos::{icon, MATERIAL_ICONS};
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::themes::material_demo_theme_data::COLOR_SCHEME;

use super::{screen_column, transitions};

/// The hit-test ids this section's controls take from.
const ID_BASE: u64 = ids::DEMO_LOCAL + 1500;

/// Upstream's `_controller.duration`.
const FORWARD_MICROS: i64 = 150_000;
/// Upstream's `_controller.reverseDuration`.
const REVERSE_MICROS: i64 = 75_000;

/// The height the section's body stands in for; upstream's is the screen
/// between the app bar and the bottom bar.
const BODY_HEIGHT: f32 = 260.0;

/// Upstream's `ModalConfiguration.barrierColor` default, `Colors.black54`.
const BARRIER_COLOR: Color = Color(0x8A00_0000);

/// Upstream's `_fabDimension` in the container demo; the FAB's own size.
const FAB_SIZE: f32 = 56.0;

/// One tick of upstream's `_controller`: `value` moves by `elapsed` of the
/// duration for its direction, clamped to 0..1. The second return is whether
/// it is still going -- `Controller::tick`'s answer, for the frame clock.
fn tick(value: f32, forward: bool, elapsed_micros: i64) -> (f32, bool) {
    let duration = if forward {
        FORWARD_MICROS
    } else {
        REVERSE_MICROS
    };
    let step = elapsed_micros as f32 / duration as f32;
    let next = if forward { value + step } else { value - step };
    let settled = if forward { next >= 1.0 } else { next <= 0.0 };
    (next.clamp(0.0, 1.0), !settled)
}

/// Upstream's `_isAnimationRunningForwardsOrComplete`: the controller's
/// status is `forward` while it runs forwards, `completed` once it has --
/// and likewise `reverse`/`dismissed` at the other end.
fn running_forwards_or_complete(value: f32, forward: bool, running: bool) -> bool {
    if running {
        forward
    } else {
        value >= 1.0
    }
}

/// Whether upstream's `Visibility(visible: status != dismissed)` shows the
/// FAB: everything but settled at zero.
fn fab_visible(value: f32, running: bool) -> bool {
    running || value > 0.0
}

/// The demo's section: upstream's `FadeScaleTransitionDemo`.
pub(super) fn section() -> AnyWidget {
    stateful(FadeScaleTransitionDemo)
}

struct FadeScaleTransitionDemo;

/// Upstream's `_FadeScaleTransitionDemoState`: the controller, plus the open
/// modal and its own entrance clock (upstream's route's).
#[derive(Default)]
struct FadeScaleDemoState {
    /// `_controller.value`, 0 at rest: the FAB starts dismissed.
    fab_value: f32,
    /// The controller's direction when it is running.
    fab_forward: bool,
    fab_running: bool,
    /// Whether the alert dialog is up, and how far through its entrance it
    /// is (the modal route's 150ms forward transition).
    dialog_open: bool,
    dialog_value: f32,
    last_frame_micros: Option<i64>,
    /// The held button, for its pressed fade.
    pressed: Option<u64>,
}

impl StatefulComponent for FadeScaleTransitionDemo {
    type State = FadeScaleDemoState;

    fn advance(&self, state: &mut FadeScaleDemoState, frame_time_micros: i64) -> bool {
        let elapsed = match state.last_frame_micros.replace(frame_time_micros) {
            Some(previous) => (frame_time_micros - previous).clamp(0, crate::app::MAX_FRAME_MICROS),
            None => 0,
        };
        let mut running = false;
        if state.fab_running {
            let (value, going) = tick(state.fab_value, state.fab_forward, elapsed);
            state.fab_value = value;
            state.fab_running = going;
            running = true;
        }
        if state.dialog_open && state.dialog_value < 1.0 {
            let (value, going) = tick(state.dialog_value, true, elapsed);
            state.dialog_value = value;
            running |= going;
        }
        running
    }

    fn build(
        &self,
        state: &FadeScaleDemoState,
        handle: StateHandle<FadeScaleDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let l10n = GalleryLocalizations::en();
        let theme = theme_of(context);
        let surface = theme.surface;
        let outline = theme.outline;
        let text = theme.text;

        let forwards_or_complete =
            running_forwards_or_complete(state.fab_value, state.fab_forward, state.fab_running);

        // The app bar: upstream's two-line title.
        let app_bar = component(
            AppBar::new(l10n.demo_fade_scale_title())
                .with_subtitle(format!("({})", l10n.demo_fade_scale_demo_instructions())),
        );

        // The body: empty but for the FAB at the bottom end, upstream's
        // `floatingActionButton` wrapped in the `FadeScaleTransition`.
        let show_fab = fab_visible(state.fab_value, state.fab_running);
        let placement = if state.fab_forward {
            transitions::fade_scale_enter(state.fab_value)
        } else {
            transitions::fade_scale_exit(state.fab_value)
        };
        let body = leaf(move || {
            let mut body_stack = Stack::new();
            if show_fab {
                body_stack = body_stack.push_positioned(
                    Opacity::new(placement.opacity, Transform::scale(placement.scale, fab())),
                    StackPosition {
                        right: Some(16.0),
                        bottom: Some(16.0),
                        ..Default::default()
                    },
                );
            }
            Container::new()
                .with_height(BODY_HEIGHT)
                .with_child(body_stack)
        });

        // The bottom bar: upstream's `bottomNavigationBar`, a divider over a
        // centered row of the two buttons.
        let show_modal = component(
            Button::new(ID_BASE, l10n.demo_fade_scale_show_alert_dialog_button())
                .with_pressed(state.pressed == Some(ID_BASE))
                .wired(
                    handle.clone(),
                    |s| &mut s.pressed,
                    |s| {
                        s.dialog_open = true;
                        s.dialog_value = 0.0;
                    },
                ),
        );
        let toggle_id = ID_BASE + 1;
        let toggle_label = if forwards_or_complete {
            l10n.demo_fade_scale_hide_fab_button()
        } else {
            l10n.demo_fade_scale_show_fab_button()
        };
        let toggle_fab = component(
            Button::new(toggle_id, toggle_label)
                .with_pressed(state.pressed == Some(toggle_id))
                .wired(
                    handle.clone(),
                    |s| &mut s.pressed,
                    |s| {
                        // Upstream's toggle: reverse when running forwards or
                        // complete, forward otherwise.
                        if running_forwards_or_complete(s.fab_value, s.fab_forward, s.fab_running) {
                            s.fab_forward = false;
                        } else {
                            s.fab_forward = true;
                        }
                        s.fab_running = true;
                    },
                ),
        );
        let bottom_bar = many(vec![show_modal, toggle_fab], move |rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(10.0);
            for button in rendered {
                row = row.push(button);
            }
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::symmetric(0.0, 8.0))
                    .with_child(
                        Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .push(Container::new().with_height(1.0).with_color(outline))
                            .push(Container::new().with_height(8.0))
                            .push(row),
                    ),
            )
        });

        let screen = screen_column(vec![app_bar, body, bottom_bar]);

        // The modal: upstream's `showModal` route -- the barrier, then the
        // dialog fading and scaling in over it.
        match state.dialog_open {
            false => screen,
            true => {
                let placement = transitions::fade_scale_enter(state.dialog_value);
                let barrier = leaf({
                    let handle = handle.clone();
                    move || {
                        let barrier_handle = handle.clone();
                        Pointer::new(ID_BASE + 2, Container::new().with_color(BARRIER_COLOR))
                            .with_handlers(rustflutter::gestures::PointerHandlers::new().with_tap(
                                move |_| {
                                    // A barrier tap dismisses, as upstream's
                                    // `barrierDismissible: true` default does.
                                    barrier_handle.set_state(|s| s.dialog_open = false);
                                },
                            ))
                    }
                });
                let dialog = dialog(state, handle, surface, outline, text);
                many(vec![screen, barrier, dialog], move |mut rendered| {
                    let dialog = rendered.pop().unwrap_or_else(|| boxed(Empty));
                    let barrier = rendered.pop().unwrap_or_else(|| boxed(Empty));
                    let screen = rendered.pop().unwrap_or_else(|| boxed(Empty));
                    Box::new(
                        Stack::new()
                            .push(screen)
                            .push_positioned(barrier, Positioned::fill())
                            .push(Center::new(Opacity::new(
                                placement.opacity,
                                Transform::scale(placement.scale, dialog),
                            ))),
                    )
                })
            }
        }
    }
}

/// The floating action button, drawn locally (see the module header):
/// upstream's `FloatingActionButton(child: Icon(Icons.add))`, a 56 circle in
/// the scheme's secondary with the add glyph in on-secondary.
fn fab() -> impl rustflutter::render::RenderBox {
    Container::new()
        .with_size(FAB_SIZE, FAB_SIZE)
        .with_color(COLOR_SCHEME.secondary)
        .with_corner_radius(FAB_SIZE / 2.0)
        .with_elevation(6)
        .with_child(Align::new(
            Alignment::CENTER,
            Text::new(icon::ADD)
                .with_font_family(MATERIAL_ICONS)
                .with_size(24.0)
                .with_color(COLOR_SCHEME.on_secondary),
        ))
}

/// Upstream's `_ExampleAlertDialog`: content "Alert Dialog" with CANCEL and
/// DISCARD text actions, both of which pop. Drawn locally because the
/// framework's `Dialog` always shows its title (see the module header).
fn dialog(
    state: &FadeScaleDemoState,
    handle: StateHandle<FadeScaleDemoState>,
    surface: Color,
    outline: Color,
    text: Color,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let cancel_id = ID_BASE + 3;
    let discard_id = ID_BASE + 4;
    let cancel = component(
        Button::new(cancel_id, l10n.demo_fade_scale_alert_dialog_cancel_button())
            .with_style(ButtonStyle::Text)
            .with_pressed(state.pressed == Some(cancel_id))
            .wired(
                handle.clone(),
                |s| &mut s.pressed,
                |s| s.dialog_open = false,
            ),
    );
    let discard = component(
        Button::new(
            discard_id,
            l10n.demo_fade_scale_alert_dialog_discard_button(),
        )
        .with_style(ButtonStyle::Text)
        .with_pressed(state.pressed == Some(discard_id))
        .wired(handle, |s| &mut s.pressed, |s| s.dialog_open = false),
    );
    many(vec![cancel, discard], move |rendered| {
        let mut actions = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0);
        for action in rendered {
            actions = actions.push(action);
        }
        Box::new(
            Container::new()
                .with_width(280.0)
                .with_color(surface)
                .with_corner_radius(28.0)
                .with_elevation(6)
                .with_border(1.0, outline)
                .with_padding(EdgeInsets::all(24.0))
                .with_child(
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(20.0)
                        .push(
                            Text::new(l10n.demo_fade_scale_alert_dialog_header())
                                .with_size(16.0)
                                .with_color(text),
                        )
                        .push(actions),
                ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forward_tick_takes_150ms_and_a_reverse_75ms() {
        let (value, running) = tick(0.0, true, FORWARD_MICROS / 2);
        assert!((value - 0.5).abs() < 1e-5);
        assert!(running);
        let (value, running) = tick(1.0, false, REVERSE_MICROS);
        assert_eq!(value, 0.0);
        assert!(!running, "settled ticks stop asking for frames");
    }

    #[test]
    fn a_tick_clamps_at_both_ends() {
        assert_eq!(tick(0.9, true, FORWARD_MICROS).0, 1.0);
        assert_eq!(tick(0.1, false, REVERSE_MICROS).0, 0.0);
    }

    #[test]
    fn the_button_label_follows_the_controller_status() {
        // Dismissed at rest: "SHOW FAB". Running forwards or complete: "HIDE
        // FAB". Mid-reverse the status is `reverse`: "SHOW FAB" again.
        assert!(!running_forwards_or_complete(0.0, true, false));
        assert!(running_forwards_or_complete(0.4, true, true));
        assert!(running_forwards_or_complete(1.0, true, false));
        assert!(!running_forwards_or_complete(0.6, false, true));
        assert!(!running_forwards_or_complete(0.0, false, false));
    }

    #[test]
    fn the_fab_is_hidden_only_when_dismissed() {
        assert!(!fab_visible(0.0, false));
        assert!(fab_visible(0.0, true), "mid-animation it still draws");
        assert!(fab_visible(0.5, false));
    }
}
