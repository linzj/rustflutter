// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/sliders_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's one `SlidersDemo` is keyed by `SlidersDemoType` (sliders,
//! rangeSliders, customSliders); the catalogue here flattens every demo to
//! one configuration (PORTING.md), so the three sections stack on the one
//! `sliders` page.
//!
//! Divergences, each also noted at its site:
//!
//! - **The framework's `Slider` is 0..1, fixed-width, one thumb.** Upstream's
//!   min/max are scaled in and out of it ([`to_unit`]/[`from_unit`]), the
//!   width is fixed at [`SLIDER_WIDTH`] rather than stretched, and a
//!   `RangeSlider` becomes a start and an end slider with the no-crossing
//!   rule kept ([`clamp_start`]/[`clamp_end`]).
//! - **No value indicators or custom shapes.** Upstream's `label`,
//!   `RangeLabels`, and the custom `SliderTheme` of `_CustomSliders`
//!   (`_CustomThumbShape`, `_CustomRangeThumbShape`,
//!   `_CustomValueIndicatorShape`) have no counterpart: the framework's
//!   slider draws its theme's thumb and nothing while dragging. The custom
//!   section's sliders are plain, its deep-purple theme not portable.
//! - **Disabled sliders are dimmed, not recolored.** The framework's `Slider`
//!   has no enabled flag, so the disabled copies are unwired under 38%
//!   opacity -- upstream's disabled content opacity.
//! - **The editable value field starts empty.** Upstream's `TextField` is fed
//!   a `TextEditingController` holding the current value; the framework's
//!   `TextField` has no initial text, so the current value is the
//!   placeholder instead and the typed text does not track the slider.
//! - **No restoration.** Upstream's `RestorationMixin` has no counterpart
//!   here; the values are plain component state.

use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent};
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, Center, Opacity};

use crate::app::{ids, GalleryState};

use super::{caption, column, DemoState};

/// The demo body for the `sliders` slug.
///
/// The signature is the dispatch's (mod.rs); each variant's state is its own
/// component's now, the way upstream's `_SlidersState` and friends are.
pub(super) fn sliders(_state: &DemoState, _handle: StateHandle<GalleryState>) -> AnyWidget {
    column(
        vec![
            caption("Sliders"),
            stateful(SlidersDemo),
            caption("Range Sliders"),
            stateful(RangeSlidersDemo),
            caption("Custom Sliders"),
            stateful(CustomSlidersDemo),
        ],
        12.0,
    )
}

/// The width every slider here takes. Upstream's stretch to the content
/// width (minus the 40px horizontal padding); the framework's `Slider` has a
/// fixed width.
const SLIDER_WIDTH: f32 = 280.0;

/// Upstream's `EdgeInsets.symmetric(horizontal: 40)` around each section.
fn section(children: Vec<AnyWidget>) -> AnyWidget {
    many(children, move |rendered| {
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(Center::new(
            Container::new()
                .with_padding(EdgeInsets::symmetric(40.0, 0.0))
                .with_child(flex),
        ))
    })
}

/// A disabled copy of a slider: unwired under upstream's 38% disabled
/// content opacity, the framework's `Slider` having no enabled flag.
fn disabled_slider(id: u64, value: f32) -> AnyWidget {
    single(
        component(Slider::new(id, value).with_width(SLIDER_WIDTH)),
        |rendered| Box::new(Opacity::new(0.38, rendered)),
    )
}

/// A value in `min..=max` as the framework slider's 0..1.
fn to_unit(value: f32, min: f32, max: f32) -> f32 {
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

/// The framework slider's 0..1 as a value in `min..=max`.
fn from_unit(unit: f32, min: f32, max: f32) -> f32 {
    min + unit.clamp(0.0, 1.0) * (max - min)
}

/// `value` snapped to the nearest of `divisions` steps between `min` and
/// `max` -- what upstream's `divisions` does inside the slider.
fn snap(value: f32, min: f32, max: f32, divisions: u32) -> f32 {
    let step = (max - min) / divisions as f32;
    ((value - min) / step).round() * step + min
}

/// The no-crossing rule of a range, kept by the two-thumb stand-in: the
/// start never passes the end, the end never drops below the start.
fn clamp_start(value: f32, end: f32) -> f32 {
    value.min(end)
}

fn clamp_end(start: f32, value: f32) -> f32 {
    value.max(start)
}

/// What a submitted editable value becomes: parse, and clamp into 0..100.
/// Upstream's `onSubmitted` in `_SlidersState.build`; an unparseable value
/// changes nothing.
fn submitted_value(text: &str) -> Option<f32> {
    text.trim()
        .parse::<f32>()
        .ok()
        .map(|value| value.clamp(0.0, 100.0))
}

// -- Sliders (BEGIN slidersDemo) -------------------------------------------------

/// Upstream's `_Sliders`.
struct SlidersDemo;

/// Upstream's `_SlidersState`: `_continuousValue` 25, `_discreteValue` 20.
struct SlidersState {
    continuous: f32,
    discrete: f32,
}

impl Default for SlidersState {
    fn default() -> SlidersState {
        SlidersState {
            continuous: 25.0,
            discrete: 20.0,
        }
    }
}

impl StatefulComponent for SlidersDemo {
    type State = SlidersState;

    fn build(
        &self,
        state: &SlidersState,
        handle: StateHandle<SlidersState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let base = ids::DEMO_LOCAL;

        // The 64x48 editable value field. The current value is the
        // placeholder -- the framework's `TextField` has no initial text (see
        // the module header) -- and a submission parses and clamps.
        let submit = handle.clone();
        let field = single(
            stateful(
                TextField::new(base + 1)
                    .with_placeholder(format!("{:.0}", state.continuous))
                    .with_on_submitted(move |text| {
                        if let Some(value) = submitted_value(text) {
                            submit.set_state(move |s| s.continuous = value);
                        }
                    }),
            ),
            |rendered| {
                Box::new(Align::new(
                    Alignment::CENTER,
                    Container::new().with_size(64.0, 48.0).with_child(rendered),
                ))
            },
        );

        section(vec![
            field,
            component(
                Slider::new(base + 2, to_unit(state.continuous, 0.0, 100.0))
                    .with_width(SLIDER_WIDTH)
                    .wired(handle.clone(), |s, v| {
                        s.continuous = from_unit(v, 0.0, 100.0)
                    }),
            ),
            disabled_slider(base + 4, to_unit(state.continuous, 0.0, 100.0)),
            component(Label::new("Continuous with Editable Numerical Value")),
            // Upstream's `SizedBox(height: 80)` between the two groups.
            leaf(|| rustflutter::widgets::SizedBox::height(80.0)),
            component(
                Slider::new(base + 3, to_unit(state.discrete, 0.0, 200.0))
                    .with_width(SLIDER_WIDTH)
                    .wired(handle.clone(), |s, v| {
                        s.discrete = snap(from_unit(v, 0.0, 200.0), 0.0, 200.0, 5)
                    }),
            ),
            disabled_slider(base + 5, to_unit(state.discrete, 0.0, 200.0)),
            component(Label::new("Discrete")),
        ])
    }
}

// -- Range sliders (BEGIN rangeSlidersDemo) --------------------------------------

/// Upstream's `_RangeSliders`.
struct RangeSlidersDemo;

/// Upstream's `_RangeSlidersState`: the continuous 25..75 and the discrete
/// 40..120.
struct RangeSlidersState {
    continuous_start: f32,
    continuous_end: f32,
    discrete_start: f32,
    discrete_end: f32,
}

impl Default for RangeSlidersState {
    fn default() -> RangeSlidersState {
        RangeSlidersState {
            continuous_start: 25.0,
            continuous_end: 75.0,
            discrete_start: 40.0,
            discrete_end: 120.0,
        }
    }
}

impl StatefulComponent for RangeSlidersDemo {
    type State = RangeSlidersState;

    fn build(
        &self,
        state: &RangeSlidersState,
        handle: StateHandle<RangeSlidersState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let base = ids::DEMO_LOCAL + 10;
        // Each `RangeSlider` is a start and an end slider with the
        // no-crossing rule kept; the framework has no two-thumb slider (see
        // the module header).
        section(vec![
            component(
                Slider::new(base, to_unit(state.continuous_start, 0.0, 100.0))
                    .with_width(SLIDER_WIDTH)
                    .wired(handle.clone(), |s, v| {
                        s.continuous_start = clamp_start(from_unit(v, 0.0, 100.0), s.continuous_end)
                    }),
            ),
            component(
                Slider::new(base + 1, to_unit(state.continuous_end, 0.0, 100.0))
                    .with_width(SLIDER_WIDTH)
                    .wired(handle.clone(), |s, v| {
                        s.continuous_end = clamp_end(s.continuous_start, from_unit(v, 0.0, 100.0))
                    }),
            ),
            disabled_slider(base + 4, to_unit(state.continuous_start, 0.0, 100.0)),
            disabled_slider(base + 5, to_unit(state.continuous_end, 0.0, 100.0)),
            component(Label::new("Continuous")),
            // Upstream's `SizedBox(height: 80)` between the two groups.
            leaf(|| rustflutter::widgets::SizedBox::height(80.0)),
            component(
                Slider::new(base + 2, to_unit(state.discrete_start, 0.0, 200.0))
                    .with_width(SLIDER_WIDTH)
                    .wired(handle.clone(), |s, v| {
                        let value = snap(from_unit(v, 0.0, 200.0), 0.0, 200.0, 5);
                        s.discrete_start = clamp_start(value, s.discrete_end);
                    }),
            ),
            component(
                Slider::new(base + 3, to_unit(state.discrete_end, 0.0, 200.0))
                    .with_width(SLIDER_WIDTH)
                    .wired(handle.clone(), |s, v| {
                        let value = snap(from_unit(v, 0.0, 200.0), 0.0, 200.0, 5);
                        s.discrete_end = clamp_end(s.discrete_start, value);
                    }),
            ),
            disabled_slider(base + 6, to_unit(state.discrete_start, 0.0, 200.0)),
            disabled_slider(base + 7, to_unit(state.discrete_end, 0.0, 200.0)),
            component(Label::new("Discrete")),
        ])
    }
}

// -- Custom sliders (BEGIN customSlidersDemo) ------------------------------------

/// Upstream's `_CustomSliders`.
struct CustomSlidersDemo;

/// Upstream's `_CustomSlidersState`: the continuous range 40..160 and the
/// discrete 25.
struct CustomSlidersState {
    continuous_start: f32,
    continuous_end: f32,
    discrete: f32,
}

impl Default for CustomSlidersState {
    fn default() -> CustomSlidersState {
        CustomSlidersState {
            continuous_start: 40.0,
            continuous_end: 160.0,
            discrete: 25.0,
        }
    }
}

impl StatefulComponent for CustomSlidersDemo {
    type State = CustomSlidersState;

    fn build(
        &self,
        state: &CustomSlidersState,
        handle: StateHandle<CustomSlidersState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let base = ids::DEMO_LOCAL + 20;
        // The `SliderTheme` wraps -- the custom thumb, range-thumb and
        // value-indicator shapes and the deep-purple palette -- are not
        // portable: the framework's slider draws its theme's styling only.
        // The values and divisions below are upstream's.
        section(vec![
            component(
                Slider::new(base, to_unit(state.discrete, 0.0, 200.0))
                    .with_width(SLIDER_WIDTH)
                    .wired(handle.clone(), |s, v| {
                        s.discrete = snap(from_unit(v, 0.0, 200.0), 0.0, 200.0, 5)
                    }),
            ),
            component(Label::new("Discrete Slider with Custom Theme")),
            // Upstream's `SizedBox(height: 80)` between the two groups.
            leaf(|| rustflutter::widgets::SizedBox::height(80.0)),
            component(
                Slider::new(base + 1, to_unit(state.continuous_start, 0.0, 200.0))
                    .with_width(SLIDER_WIDTH)
                    .wired(handle.clone(), |s, v| {
                        s.continuous_start = clamp_start(from_unit(v, 0.0, 200.0), s.continuous_end)
                    }),
            ),
            component(
                Slider::new(base + 2, to_unit(state.continuous_end, 0.0, 200.0))
                    .with_width(SLIDER_WIDTH)
                    .wired(handle.clone(), |s, v| {
                        s.continuous_end = clamp_end(s.continuous_start, from_unit(v, 0.0, 200.0))
                    }),
            ),
            component(Label::new("Continuous Range Slider with Custom Theme")),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_scaling_round_trips() {
        assert_eq!(to_unit(25.0, 0.0, 100.0), 0.25);
        assert_eq!(from_unit(0.25, 0.0, 100.0), 25.0);
        assert_eq!(to_unit(40.0, 0.0, 200.0), 0.2);
        // Out-of-range values clamp into the range.
        assert_eq!(to_unit(-5.0, 0.0, 100.0), 0.0);
        assert_eq!(from_unit(1.5, 0.0, 200.0), 200.0);
    }

    #[test]
    fn divisions_snap_to_the_nearest_step() {
        // 0..200 with 5 divisions: steps of 40. A halfway value rounds away
        // from zero, as Dart's `double.round` does.
        assert_eq!(snap(41.0, 0.0, 200.0, 5), 40.0);
        assert_eq!(snap(59.0, 0.0, 200.0, 5), 40.0);
        assert_eq!(snap(61.0, 0.0, 200.0, 5), 80.0);
        assert_eq!(snap(20.0, 0.0, 200.0, 5), 40.0);
        assert_eq!(snap(0.0, 0.0, 200.0, 5), 0.0);
    }

    #[test]
    fn a_range_never_crosses() {
        assert_eq!(clamp_start(80.0, 75.0), 75.0);
        assert_eq!(clamp_start(25.0, 75.0), 25.0);
        assert_eq!(clamp_end(40.0, 20.0), 40.0);
        assert_eq!(clamp_end(40.0, 120.0), 120.0);
    }

    #[test]
    fn a_submitted_value_parses_and_clamps() {
        assert_eq!(submitted_value("42"), Some(42.0));
        assert_eq!(submitted_value("-3"), Some(0.0));
        assert_eq!(submitted_value("900"), Some(100.0));
        assert_eq!(submitted_value("not a number"), None);
    }

    #[test]
    fn the_starting_values_are_upstreams() {
        let sliders = SlidersState::default();
        assert_eq!(sliders.continuous, 25.0);
        assert_eq!(sliders.discrete, 20.0);
        let range = RangeSlidersState::default();
        assert_eq!((range.continuous_start, range.continuous_end), (25.0, 75.0));
        assert_eq!((range.discrete_start, range.discrete_end), (40.0, 120.0));
        let custom = CustomSlidersState::default();
        assert_eq!(
            (custom.continuous_start, custom.continuous_end),
            (40.0, 160.0)
        );
        assert_eq!(custom.discrete, 25.0);
    }
}
