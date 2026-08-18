// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_switch_demo.dart` (flutter/
//! gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `_CupertinoSwitchDemoState` holds two switches --
//! `_switchValueA` (`RestorableBool(false)`) and `_switchValueB`
//! (`RestorableBool(true)`) -- shown enabled in one row and disabled in the
//! row below, centered in a `CupertinoPageScaffold`. All of that is here as
//! one per-demo [`StatefulComponent`]; the framework's `CupertinoSwitch`
//! carries the tap/drag mechanics and the disabled look itself.
//!
//! Divergences, each commented at its site as well: `RestorationMixin` is
//! not carried (nothing here restores), and the `Semantics(container: true,
//! label: ...)` around the column is kept as a label node, the nearest the
//! framework's semantics surface has.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::semantics::{self, SemanticsProperties};
use rustflutter::widgets::Center;

use crate::app::{ids, GalleryState};

/// Hit-test ids, from the demo-local block (PORTING.md: fixed bases, no
/// counters). The disabled row needs none: it takes no input.
const SWITCH_A: u64 = ids::DEMO_LOCAL;
const SWITCH_B: u64 = ids::DEMO_LOCAL + 1;
const SWITCH_A_DISABLED: u64 = ids::DEMO_LOCAL + 2;
const SWITCH_B_DISABLED: u64 = ids::DEMO_LOCAL + 3;

/// The demo body for the `cupertino-switch` slug.
///
/// `state` is read for the resolved brightness only: upstream's demo runs
/// under the app's `CupertinoTheme`, which the gallery derives from the
/// options' brightness, so the same theme is provided over the stage here.
pub(super) fn stage(state: &GalleryState) -> AnyWidget {
    let theme = match state.options.resolved_brightness() {
        Brightness::Light => CupertinoTheme::light(),
        Brightness::Dark => CupertinoTheme::dark(),
    };
    provide(theme, stateful(CupertinoSwitchDemo))
}

/// Upstream's `CupertinoSwitchDemo`.
struct CupertinoSwitchDemo;

/// Upstream's `_CupertinoSwitchDemoState`: A starts off, B starts on.
struct SwitchDemoState {
    a: bool,
    b: bool,
}

impl Default for SwitchDemoState {
    fn default() -> SwitchDemoState {
        SwitchDemoState { a: false, b: true }
    }
}

impl StatefulComponent for CupertinoSwitchDemo {
    type State = SwitchDemoState;

    fn build(
        &self,
        state: &SwitchDemoState,
        handle: StateHandle<SwitchDemoState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let a = state.a;
        let b = state.b;

        // One row per upstream `Row(mainAxisAlignment: center)`: the enabled
        // pair, then the disabled pair mirroring the same values.
        let row = |enabled: bool| {
            let mut switches = Vec::new();
            for (id, disabled_id, value, set) in [
                (
                    SWITCH_A,
                    SWITCH_A_DISABLED,
                    a,
                    set_a as fn(&mut SwitchDemoState, bool),
                ),
                (SWITCH_B, SWITCH_B_DISABLED, b, set_b),
            ] {
                let switch = if enabled {
                    stateful(CupertinoSwitch::new(id, value).wired(handle.clone(), set))
                } else {
                    // Upstream's `onChanged: null`; the framework's switch
                    // dims itself (switch.dart's `_kDisabledOpacity`).
                    stateful(CupertinoSwitch::new(disabled_id, value).with_enabled(false))
                };
                switches.push(switch);
            }
            many(switches, |rendered| {
                let mut row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_main_axis_alignment(MainAxisAlignment::Center)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center);
                for switch in rendered {
                    row = row.push(switch);
                }
                Box::new(row)
            })
        };

        // Upstream's `Center(child: Semantics(..., child: Column(
        // mainAxisAlignment: center, ...)))`.
        let column = many(vec![row(true), row(false)], |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            for row in rendered {
                column = column.push(row);
            }
            Box::new(Center::new(column))
        });
        // demoSelectionControlsSwitchTitle.
        let body = semantics::describe(SemanticsProperties::label("Switch"), column);

        component(
            CupertinoPageScaffold::new(body).with_navigation_bar(component(
                // `automaticallyImplyLeading: false` is the framework's
                // default: no back button unless asked for.
                CupertinoNavigationBar::new().with_middle("Switch"), // demoSelectionControlsSwitchTitle
            )),
        )
    }
}

/// Upstream's `onChanged: (value) { setState(() { _switchValueA.value = value; }); }`.
fn set_a(state: &mut SwitchDemoState, value: bool) {
    state.a = value;
}

/// The same, for `_switchValueB`.
fn set_b(state: &mut SwitchDemoState, value: bool) {
    state.b = value;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starting_values_are_upstreams() {
        // `RestorableBool(false)` and `RestorableBool(true)`.
        let state = SwitchDemoState::default();
        assert!(!state.a);
        assert!(state.b);
    }

    #[test]
    fn the_switches_toggle_independently() {
        let mut state = SwitchDemoState::default();
        set_a(&mut state, true);
        assert!(state.a);
        assert!(state.b);
        set_b(&mut state, false);
        assert!(state.a);
        assert!(!state.b);
    }
}
