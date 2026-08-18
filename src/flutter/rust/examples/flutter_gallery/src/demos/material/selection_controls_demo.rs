// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/selection_controls_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's one `SelectionControlsDemo` is keyed by
//! `SelectionControlsDemoType` (checkbox, radio, switches) and each variant is
//! its own `StatefulWidget`; the catalogue here flattens every demo to one
//! configuration (PORTING.md), so the three sections stack on the one
//! `selection-controls` page, each with its own component and state.
//!
//! Divergences, each also noted at its site:
//!
//! - **The tristate checkbox has no indeterminate mark.** The framework's
//!   `Checkbox` is a bool: [`next_tristate`] cycles the value the way
//!   upstream's `tristate: true` does, but a `None` value draws as unchecked
//!   because there is no dash to draw.
//! - **Disabled switches are dimmed, not recolored.** The framework's
//!   `Switch` has no enabled flag (unlike its `Checkbox` and `Radio`), so the
//!   disabled row is an unwired switch under 38% opacity -- upstream's
//!   disabled content opacity -- rather than the disabled track/thumb
//!   colours.
//! - **No restoration.** Upstream's `RestorationMixin` has no counterpart
//!   here; the values are plain component state.

use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent};
use rustflutter::prelude::*;
use rustflutter::render::{
    CrossAxisAlignment, MainAxisAlignment, MainAxisSize, RenderFlex, RenderRef,
};
use rustflutter::widgets::{Center, Empty, Opacity};

use crate::app::{ids, GalleryState};

use super::{caption, column, DemoState};

/// The demo body for the `selection-controls` slug.
///
/// The signature is the dispatch's (mod.rs); each variant's state is its own
/// component's now, the way upstream's `_CheckboxDemoState` and friends are.
pub(super) fn selection_controls(
    _state: &DemoState,
    _handle: StateHandle<GalleryState>,
) -> AnyWidget {
    column(
        vec![
            caption("Checkbox"),
            stateful(CheckboxDemo),
            caption("Radio"),
            stateful(RadioDemo),
            caption("Switch"),
            stateful(SwitchDemo),
        ],
        8.0,
    )
}

/// A horizontally centred row: upstream's
/// `Row(mainAxisAlignment: MainAxisAlignment.center)`.
fn centered_row(children: Vec<AnyWidget>) -> AnyWidget {
    many(children, move |rendered| {
        let mut flex = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(Center::new(flex))
    })
}

/// The enabled row over the disabled one: upstream's
/// `Column(mainAxisAlignment: MainAxisAlignment.center)`.
fn enabled_over_disabled(enabled: Vec<AnyWidget>, disabled: Vec<AnyWidget>) -> AnyWidget {
    many(
        vec![centered_row(enabled), centered_row(disabled)],
        move |mut rendered| {
            let disabled = rendered.pop().unwrap_or_else(|| RenderRef::new(Empty));
            let enabled = rendered.pop().unwrap_or_else(|| RenderRef::new(Empty));
            Box::new(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .push(enabled)
                    .push(disabled),
            )
        },
    )
}

/// A control drawn as its disabled self: unwired under upstream's 38%
/// disabled content opacity.
fn dimmed(child: AnyWidget) -> AnyWidget {
    single(child, |rendered| Box::new(Opacity::new(0.38, rendered)))
}

// -- Checkbox (BEGIN selectionControlsDemoCheckbox) -----------------------------

/// Upstream's `_CheckboxDemo`.
struct CheckboxDemo;

/// Upstream's `_CheckboxDemoState`: `checkboxValueA`, `checkboxValueB` and
/// the tristate `checkboxValueC`.
#[derive(Default)]
struct CheckboxState {
    a: bool,
    b: bool,
    c: Option<bool>,
}

impl CheckboxState {
    /// The starting values: a on, b off, c indeterminate.
    fn upstream() -> CheckboxState {
        CheckboxState {
            a: true,
            b: false,
            c: None,
        }
    }
}

/// The next value of a tristate checkbox, in upstream's order: false -> true,
/// true -> null, null -> false (`Checkbox._handleTap` with `tristate: true`).
fn next_tristate(value: Option<bool>) -> Option<bool> {
    match value {
        Some(false) => Some(true),
        Some(true) => None,
        None => Some(false),
    }
}

impl StatefulComponent for CheckboxDemo {
    type State = CheckboxState;

    fn initial_state(&self) -> CheckboxState {
        CheckboxState::upstream()
    }

    fn build(
        &self,
        state: &CheckboxState,
        handle: StateHandle<CheckboxState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let base = ids::DEMO_LOCAL;
        // A `None` value draws as unchecked: the framework's `Checkbox` has
        // no indeterminate mark (see the module header).
        let c = state.c.unwrap_or(false);
        enabled_over_disabled(
            vec![
                component(Checkbox::new(base, state.a).wired(handle.clone(), |s| s.a = !s.a)),
                component(Checkbox::new(base + 1, state.b).wired(handle.clone(), |s| s.b = !s.b)),
                component(
                    Checkbox::new(base + 2, c).wired(handle.clone(), |s| s.c = next_tristate(s.c)),
                ),
            ],
            // The disabled row mirrors the values, as upstream's does.
            vec![
                component(Checkbox::new(base + 3, state.a).with_enabled(false)),
                component(Checkbox::new(base + 4, state.b).with_enabled(false)),
                component(Checkbox::new(base + 5, c).with_enabled(false)),
            ],
        )
    }
}

// -- Radio (BEGIN selectionControlsDemoRadio) ------------------------------------

/// Upstream's `_RadioDemo`.
struct RadioDemo;

/// Upstream's `_RadioDemoState`: `radioValue`, starting at 0.
#[derive(Default)]
struct RadioState {
    value: usize,
}

impl StatefulComponent for RadioDemo {
    type State = RadioState;

    fn build(
        &self,
        state: &RadioState,
        handle: StateHandle<RadioState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        // The three sections share the page, so each takes its own ids.
        let base = ids::DEMO_LOCAL + 10;
        // Upstream's `for (int index = 0; index < 2; ++index)` loop. The
        // framework's `Radio.wired` takes a bare `fn`, so the two arms are
        // their own closures rather than one loop body.
        let enabled = vec![
            component(Radio::new(base, state.value == 0).wired(handle.clone(), |s| s.value = 0)),
            component(
                Radio::new(base + 1, state.value == 1).wired(handle.clone(), |s| s.value = 1),
            ),
        ];
        // The disabled row mirrors the selection, as upstream's does.
        let mut disabled: Vec<AnyWidget> = Vec::new();
        for index in 0..2 {
            disabled.push(component(
                Radio::new(base + 2 + index, state.value == index as usize).with_enabled(false),
            ));
        }
        enabled_over_disabled(enabled, disabled)
    }
}

// -- Switch (BEGIN selectionControlsDemoSwitches) --------------------------------

/// Upstream's `_SwitchDemo`.
struct SwitchDemo;

/// Upstream's `_SwitchDemoState`: `switchValueA` on, `switchValueB` off.
struct SwitchState {
    a: bool,
    b: bool,
}

impl Default for SwitchState {
    fn default() -> SwitchState {
        SwitchState { a: true, b: false }
    }
}

impl StatefulComponent for SwitchDemo {
    type State = SwitchState;

    fn build(
        &self,
        state: &SwitchState,
        handle: StateHandle<SwitchState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let base = ids::DEMO_LOCAL + 20;
        enabled_over_disabled(
            vec![
                component(Switch::new(base, state.a).wired(handle.clone(), |s| s.a = !s.a)),
                component(Switch::new(base + 1, state.b).wired(handle.clone(), |s| s.b = !s.b)),
            ],
            // The disabled row mirrors the values. `Switch` has no enabled
            // flag, so these are dimmed rather than recolored; see the module
            // header.
            vec![
                dimmed(component(Switch::new(base + 2, state.a))),
                dimmed(component(Switch::new(base + 3, state.b))),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tristate_checkbox_cycles_false_true_null() {
        // Upstream's `_handleTap`: false -> true -> null -> false.
        assert_eq!(next_tristate(Some(false)), Some(true));
        assert_eq!(next_tristate(Some(true)), None);
        assert_eq!(next_tristate(None), Some(false));
        // The demo starts indeterminate, so the first tap unchecks.
        let mut value = CheckboxState::upstream().c;
        value = next_tristate(value);
        assert_eq!(value, Some(false));
        value = next_tristate(value);
        assert_eq!(value, Some(true));
        value = next_tristate(value);
        assert_eq!(value, None);
    }

    #[test]
    fn the_starting_values_are_upstreams() {
        let checkboxes = CheckboxState::upstream();
        assert!(checkboxes.a);
        assert!(!checkboxes.b);
        assert_eq!(checkboxes.c, None);
        assert_eq!(RadioState::default().value, 0);
        let switches = SwitchState::default();
        assert!(switches.a);
        assert!(!switches.b);
    }
}
