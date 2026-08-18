// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/progress_indicator_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's one `ProgressIndicatorDemo` is keyed by
//! `ProgressIndicatorDemoType` (circular, linear); the catalogue here flattens
//! every demo to one configuration (PORTING.md), so both sections stack on
//! the one `progress-indicator` page.
//!
//! Divergences, each also noted at its site:
//!
//! - **The controller is the frame clock, not an `AnimationController`.**
//!   Upstream runs a 1500ms controller forward and back, driving the
//!   determinate indicators through `Interval(0.0, 0.9, curve:
//!   Curves.fastOutSlowIn)`. Here [`ProgressIndicatorDemo`]'s `advance` walks
//!   the same timeline off the frame clock; [`animated_value`] is the curved
//!   value, forward and reverse.
//! - **Indeterminate indicators are fed the app's looping value.** The
//!   framework's `Spinner` and `ProgressBar` are determinate drawings; the
//!   indeterminate look comes from the sawtooth [`SpinnerValue`] the app
//!   publishes, so the "indeterminate" linear bar fills and restarts rather
//!   than sliding a block along the track the way upstream's
//!   `LinearProgressIndicator` does.
//! - **Bars are 280px, not stretched.** Upstream's indicators take the full
//!   content width; the framework's `ProgressBar` has a fixed width.

use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent};
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Center, FullWidth};

use super::{caption, column, DemoState};

/// The demo body for the `progress-indicator` slug.
///
/// The signature is the dispatch's (mod.rs); the state it hands over is this
/// demo's own now, the way upstream's `_ProgressIndicatorDemoState` is.
pub(super) fn progress(_state: &DemoState, _context: &mut BuildContext) -> AnyWidget {
    stateful(ProgressIndicatorDemo)
}

/// Upstream's `ProgressIndicatorDemo`.
struct ProgressIndicatorDemo;

/// Upstream's `_ProgressIndicatorDemoState`: where along the 1500ms run the
/// controller is, and which way it is going.
struct ProgressState {
    /// The frame time the current run started at.
    started_micros: Option<i64>,
    /// Forward on the way to 1, reverse on the way back.
    forward: bool,
    /// The curved value, kept for the tests.
    value: f32,
}

impl Default for ProgressState {
    fn default() -> ProgressState {
        ProgressState {
            started_micros: None,
            forward: true,
            value: 0.0,
        }
    }
}

/// Upstream's `Duration(milliseconds: 1500)`.
const DURATION_MICROS: i64 = 1_500_000;
/// Upstream's `Interval(0.0, 0.9, ...)`: the eased run finishes at 90% of the
/// controller and holds for the last tenth.
const INTERVAL_END: f32 = 0.9;

/// The animation's value at `t` (0..1 along the current run).
///
/// Forward is `Interval(0.0, 0.9, curve: Curves.fastOutSlowIn)`; reverse is
/// `reverseCurve: Curves.fastOutSlowIn` over the controller running 1 to 0,
/// with no interval.
fn animated_value(t: f32, forward: bool) -> f32 {
    if forward {
        Curve::FAST_OUT_SLOW_IN.transform((t / INTERVAL_END).clamp(0.0, 1.0))
    } else {
        Curve::FAST_OUT_SLOW_IN.transform(1.0 - t)
    }
}

/// A centred, shrink-wrapped column: upstream's `Column` inside the demo's
/// `Center`, with `FullWidth` standing in for the width the page hands it.
fn centered_column(children: Vec<AnyWidget>, spacing: f32) -> AnyWidget {
    many(children, move |rendered| {
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            // Upstream's `SizedBox(height: 32)` between the two indicators.
            .with_spacing(spacing);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(FullWidth::new(Center::new(flex)))
    })
}

impl StatefulComponent for ProgressIndicatorDemo {
    type State = ProgressState;

    /// Upstream's status listener: a completed run reverses, a dismissed one
    /// goes forward. This demo never stops, so it always asks for the next
    /// frame; the app lists `progress-indicator` among its animated demos, so
    /// the frames come.
    fn advance(&self, state: &mut ProgressState, frame_time_micros: i64) -> bool {
        let started = *state.started_micros.get_or_insert(frame_time_micros);
        let t = ((frame_time_micros - started) as f32 / DURATION_MICROS as f32).clamp(0.0, 1.0);
        state.value = animated_value(t, state.forward);
        if t >= 1.0 {
            state.forward = !state.forward;
            state.started_micros = Some(frame_time_micros);
        }
        true
    }

    fn build(
        &self,
        state: &ProgressState,
        _handle: StateHandle<ProgressState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        // The looping value the app publishes stands in for an indeterminate
        // indicator; see the module header.
        let spin = context
            .inherited::<SpinnerValue>()
            .map(|value| value.0)
            .unwrap_or(0.0);
        let value = state.value;

        // `_buildIndicators`, once per `ProgressIndicatorDemoType`.
        column(
            vec![
                caption("Circular Progress Indicator"),
                centered_column(
                    vec![
                        component(Spinner::new(spin)),
                        component(Spinner::new(value)),
                    ],
                    32.0,
                ),
                caption("Linear Progress Indicator"),
                centered_column(
                    vec![
                        component(ProgressBar::new(spin).with_width(280.0)),
                        component(ProgressBar::new(value).with_width(280.0)),
                    ],
                    32.0,
                ),
            ],
            16.0,
        )
    }
}

/// The spinner's current value, published by the app so the progress demo can
/// read it without the demo owning the controller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinnerValue(pub f32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_forward_run_holds_at_one_for_the_last_tenth() {
        // Inside the interval the value is eased; past 0.9 it sits at 1.
        assert_eq!(animated_value(0.0, true), 0.0);
        assert_eq!(animated_value(0.9, true), 1.0);
        assert_eq!(animated_value(0.95, true), 1.0);
        assert_eq!(animated_value(1.0, true), 1.0);
        let halfway = animated_value(0.45, true);
        assert!(halfway > 0.0 && halfway < 1.0);
    }

    #[test]
    fn the_reverse_run_covers_the_whole_duration() {
        // No interval on the way back: 90% through the reverse the value is
        // still falling, not parked at 0.
        assert_eq!(animated_value(0.0, false), 1.0);
        assert_eq!(animated_value(1.0, false), 0.0);
        assert!(animated_value(0.9, false) < 1.0);
        assert!(animated_value(0.95, false) > 0.0);
    }

    #[test]
    fn a_full_run_turns_around() {
        let mut state = ProgressState::default();
        let demo = ProgressIndicatorDemo;
        assert!(demo.advance(&mut state, 0));
        assert!(state.forward);
        assert!(demo.advance(&mut state, DURATION_MICROS));
        assert!(!state.forward, "a completed run reverses");
        assert!(demo.advance(&mut state, DURATION_MICROS * 2));
        assert!(state.forward, "a dismissed run goes forward");
    }
}
