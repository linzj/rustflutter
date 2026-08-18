// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_slider_demo.dart` (flutter/
//! gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `_CupertinoSliderDemoState` holds two values -- a continuous
//! one (`RestorableDouble(25.0)`) and a discrete one (`RestorableDouble(20.0)`
//! with `divisions: 5`) -- over `min: 0, max: 100`, each with an enabled
//! slider, a disabled copy and a readout, in two centered columns in a `Wrap`
//! inside a `CupertinoPageScaffold`. All of that is here as one per-demo
//! [`StatefulComponent`].
//!
//! Divergences, each commented at its site as well:
//!
//! * The framework's `CupertinoSlider` is 0..1 and fixed-width; upstream's
//!   0..100 range is scaled in and out of it ([`to_unit`]/[`from_unit`]) and
//!   the discrete slider's `divisions: 5` snaps in the demo's own setter
//!   (slider.dart's `_discretize`), the same approach as the material sliders
//!   demo.
//! * `RestorationMixin` is not carried: nothing here restores.
//! * The `MergeSemantics` around each readout is semantics-only; the readout
//!   `Text` stands on its own.
//! * The readouts wear the Cupertino text style directly (upstream's
//!   `DefaultTextStyle` from `CupertinoTheme.of(context)`); there is no
//!   ambient-default-text mechanism to install instead.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Center, SizedBox, Wrap};

use crate::app::{ids, GalleryState};

/// Hit-test ids, from the demo-local block (PORTING.md: fixed bases, no
/// counters).
const CONTINUOUS: u64 = ids::DEMO_LOCAL;
const CONTINUOUS_DISABLED: u64 = ids::DEMO_LOCAL + 1;
const DISCRETE: u64 = ids::DEMO_LOCAL + 2;
const DISCRETE_DISABLED: u64 = ids::DEMO_LOCAL + 3;

/// The demo body for the `cupertino-slider` slug.
///
/// `state` is read for the resolved brightness only: upstream's demo runs
/// under the app's `CupertinoTheme`, which the gallery derives from the
/// options' brightness, so the same theme is provided over the stage here.
pub(super) fn stage(state: &GalleryState) -> AnyWidget {
    let theme = match state.options.resolved_brightness() {
        Brightness::Light => CupertinoTheme::light(),
        Brightness::Dark => CupertinoTheme::dark(),
    };
    provide(theme, stateful(CupertinoSliderDemo))
}

/// Upstream's `CupertinoSliderDemo`.
struct CupertinoSliderDemo;

/// Upstream's `_CupertinoSliderDemoState`: `_value` is 25.0, `_discreteValue`
/// is 20.0.
struct SliderDemoState {
    value: f32,
    discrete_value: f32,
}

impl Default for SliderDemoState {
    fn default() -> SliderDemoState {
        SliderDemoState {
            value: 25.0,
            discrete_value: 20.0,
        }
    }
}

/// Upstream's `min: 0.0, max: 100.0`, shared by both sliders.
const MIN: f32 = 0.0;
const MAX: f32 = 100.0;
/// Upstream's `divisions: 5` on the discrete slider.
const DIVISIONS: u32 = 5;

/// A value in `min..=max` as the framework slider's 0..1.
fn to_unit(value: f32) -> f32 {
    ((value - MIN) / (MAX - MIN)).clamp(0.0, 1.0)
}

/// The framework slider's 0..1 as a value in `min..=max`.
fn from_unit(unit: f32) -> f32 {
    MIN + unit.clamp(0.0, 1.0) * (MAX - MIN)
}

/// `value` snapped to the nearest of [`DIVISIONS`] steps -- what upstream's
/// `divisions` does inside the slider (slider.dart's `_discretize`).
fn snap(value: f32) -> f32 {
    let step = (MAX - MIN) / DIVISIONS as f32;
    ((value - MIN) / step).round() * step + MIN
}

impl StatefulComponent for CupertinoSliderDemo {
    type State = SliderDemoState;

    fn build(
        &self,
        state: &SliderDemoState,
        handle: StateHandle<SliderDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        // text_theme.dart's `_kDefaultTextStyle`, upstream's DefaultTextStyle
        // (see the module header).
        let text_style = theme.text_style();

        // The continuous group: `_value`, its disabled copy, and
        // `demoCupertinoSliderContinuous(_value.toStringAsFixed(1))`.
        let continuous = slider_group(
            state.value,
            CONTINUOUS,
            CONTINUOUS_DISABLED,
            Some(handle.clone()),
            |s, unit| s.value = from_unit(unit),
            format!("Continuous: {:.1}", state.value),
            text_style.clone(),
        );
        // The discrete group: `_discreteValue` with `divisions: 5`, its
        // disabled copy, and `demoCupertinoSliderDiscrete(...)`.
        let discrete = slider_group(
            snap(state.discrete_value),
            DISCRETE,
            DISCRETE_DISABLED,
            Some(handle.clone()),
            |s, unit| s.discrete_value = snap(from_unit(unit)),
            format!("Discrete: {:.1}", snap(state.discrete_value)),
            text_style,
        );

        // Upstream's `Center(child: Wrap(children: [column, column]))`.
        let columns = many(vec![continuous, discrete], |rendered| {
            let mut wrap = Wrap::new();
            for column in rendered {
                wrap = wrap.push(column);
            }
            Box::new(Center::new(wrap))
        });

        component(
            CupertinoPageScaffold::new(columns).with_navigation_bar(component(
                // `automaticallyImplyLeading: false` is the framework's
                // default: no back button unless asked for.
                CupertinoNavigationBar::new().with_middle("Slider"), // demoCupertinoSliderTitle
            )),
        )
    }
}

/// One of upstream's two columns: a 32 spacer, the slider, its disabled copy
/// and the readout, centered (`MainAxisAlignment.center` under the `Wrap`).
///
/// The disabled copy is the same slider unwired: upstream's `onChanged: null`,
/// which slider.dart draws exactly like an enabled slider (there is no
/// disabled color path), so no dimming is added.
fn slider_group(
    value: f32,
    id: u64,
    disabled_id: u64,
    handle: Option<StateHandle<SliderDemoState>>,
    set: fn(&mut SliderDemoState, f32),
    readout: String,
    text_style: TextStyle,
) -> AnyWidget {
    let slider = CupertinoSlider::new(id, to_unit(value));
    let slider = match handle {
        Some(handle) => slider.wired(handle, set),
        None => slider,
    };
    let readout_style = text_style;
    many(
        vec![
            component(slider),
            component(CupertinoSlider::new(disabled_id, to_unit(value))),
            leaf(move || Text::new(readout.clone()).with_style(readout_style.clone())),
        ],
        |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                // Upstream's `SizedBox(height: 32)` above the first slider.
                .push(SizedBox::height(32.0));
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starting_values_are_upstreams() {
        // `RestorableDouble(25.0)` and `RestorableDouble(20.0)`.
        let state = SliderDemoState::default();
        assert_eq!(state.value, 25.0);
        assert_eq!(state.discrete_value, 20.0);
    }

    #[test]
    fn unit_scaling_round_trips_and_clamps() {
        assert_eq!(to_unit(25.0), 0.25);
        assert_eq!(from_unit(0.25), 25.0);
        assert_eq!(to_unit(-5.0), 0.0);
        assert_eq!(from_unit(1.5), 100.0);
    }

    #[test]
    fn the_discrete_slider_snaps_to_five_divisions() {
        // 0..100 with 5 divisions: steps of 20. A halfway value rounds away
        // from zero, as Dart's `double.round` does.
        assert_eq!(snap(20.0), 20.0);
        assert_eq!(snap(29.0), 20.0);
        assert_eq!(snap(31.0), 40.0);
        assert_eq!(snap(10.0), 20.0);
        assert_eq!(snap(100.0), 100.0);
    }

    #[test]
    fn the_readouts_are_upstreams_format() {
        // `demoCupertinoSliderContinuous(value.toStringAsFixed(1))` and its
        // discrete sibling resolve to "Continuous: {value}" / "Discrete:
        // {value}" in English.
        let state = SliderDemoState::default();
        assert_eq!(
            format!("Continuous: {:.1}", state.value),
            "Continuous: 25.0"
        );
        assert_eq!(
            format!("Discrete: {:.1}", snap(state.discrete_value)),
            "Discrete: 20.0"
        );
    }
}
