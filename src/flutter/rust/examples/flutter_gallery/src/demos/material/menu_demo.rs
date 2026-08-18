// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/menu_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `MenuDemo` has four configurations -- `_ContextMenuDemo`,
//! `_SectionedMenuDemo`, `_ChecklistMenuDemo` and `_SimpleMenuDemo`, picked
//! through the demo's options section. The catalogue here flattens every demo
//! to one configuration (PORTING.md's "demo options section is unreachable"),
//! so the stage shows all four list items stacked, in upstream's
//! configuration order; each opens its own popup menu.
//!
//! Divergences, each also marked at its site:
//!
//! * `MenuDemo.build`'s `Scaffold`/`AppBar` (title "Menu") and its centering
//!   `Padding` are the demo page's own chrome now (`pages/demo.rs`); the
//!   stage starts at the four items.
//! * Upstream shows a menu as a route whose `RelativeRect` anchor is measured
//!   from the button's render box. There is no route and no way to read a
//!   sibling's rect at build time, so an open menu is stacked over the stage
//!   at a fixed offset next to its row ([`menu_position`]) rather than at the
//!   button's measured position, and `menu.rs`'s `popup_menu_offset` fitting
//!   math has no anchor rect to work from.
//! * The sectioned menu's items lead with icons upstream (`ListTile` children
//!   with `Icons.visibility`/`person_add`/`link`/`delete`);
//!   `PopupMenuItem` here carries a label only, so the icons are absent.
//! * The snackbar upstream auto-dismisses after four seconds and
//!   `hideCurrentSnackBar` replaces a showing one. There is no timer in the
//!   frame scheduler, so the snackbar stays until it is tapped or another
//!   selection replaces it.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::StackPosition;
use rustflutter::widgets::{Positioned, Stack};

use crate::app::ids;

use super::column;

/// Upstream's `SimpleValue`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SimpleValue {
    #[default]
    One,
    Two,
    Three,
}

/// Upstream's `CheckedValue`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckedValue {
    One,
    Two,
    Three,
    Four,
}

/// Which of the four demos' menus is open, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenMenu {
    Context,
    Sectioned,
    Simple,
    Checklist,
}

/// Upstream's `_MenuDemoState` (the snackbar) with the two stateful item
/// demos' state folded in: `_SimpleMenuDemoState._simpleValue` and
/// `_ChecklistMenuDemoState._checkedValues`.
#[derive(Default)]
struct MenuDemoState {
    open: Option<OpenMenu>,
    snackbar: Option<String>,
    simple: SimpleValue,
    checked: [bool; 4],
}

/// Upstream's `simpleValueToString`: `demoMenuItemValueOne/Two/Three`.
fn simple_label(value: SimpleValue) -> &'static str {
    match value {
        SimpleValue::One => "Menu item one",
        SimpleValue::Two => "Menu item two",
        SimpleValue::Three => "Menu item three",
    }
}

/// Upstream's `checkedValueToString`: `demoMenuOne` through `demoMenuFour`.
fn checked_label(value: CheckedValue) -> &'static str {
    match value {
        CheckedValue::One => "One",
        CheckedValue::Two => "Two",
        CheckedValue::Three => "Three",
        CheckedValue::Four => "Four",
    }
}

const CHECKED_VALUES: [CheckedValue; 4] = [
    CheckedValue::One,
    CheckedValue::Two,
    CheckedValue::Three,
    CheckedValue::Four,
];

/// Upstream's `demoMenuChecked(checkedValuesToString(...))`: Dart interpolates
/// the mapped iterable as `(One, Three)`, so that is the format here.
fn checked_summary(checked: &[bool; 4]) -> String {
    let names: Vec<&str> = CHECKED_VALUES
        .iter()
        .enumerate()
        .filter(|(index, _)| checked[*index])
        .map(|(_, value)| checked_label(*value))
        .collect();
    format!("Checked: ({})", names.join(", "))
}

/// Upstream's `_MenuDemoState.showInSnackBar`: replace whatever is showing.
fn show_in_snackbar(state: &mut MenuDemoState, message: String) {
    state.snackbar = Some(message);
}

/// Upstream's `_ChecklistMenuDemoState.showCheckedMenuSelections`.
fn check_toggled(state: &mut MenuDemoState, value: CheckedValue) {
    let index = CHECKED_VALUES
        .iter()
        .position(|v| *v == value)
        .expect("a known value");
    state.checked[index] = !state.checked[index];
    let summary = checked_summary(&state.checked);
    show_in_snackbar(state, summary);
}

/// The demo body for the `menu` slug.
pub(super) fn stage() -> AnyWidget {
    stateful(MenuDemo)
}

struct MenuDemo;

impl StatefulComponent for MenuDemo {
    type State = MenuDemoState;

    fn initial_state(&self) -> MenuDemoState {
        // `_SimpleMenuDemoState.initState` starts at `SimpleValue.two`, and
        // `_checkedValues` starts with `CheckedValue.three` checked.
        MenuDemoState {
            simple: SimpleValue::Two,
            checked: [false, false, true, false],
            ..Default::default()
        }
    }

    fn build(
        &self,
        state: &MenuDemoState,
        handle: StateHandle<MenuDemoState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let base = ids::DEMO_LOCAL;

        // The four items, in upstream's configuration order: context menu,
        // sectioned menu, checklist menu, simple menu.
        let context_item = component(
            ListTile::new("An item with a context menu").with_trailing(component(
                PopupMenuButton::new(base)
                    .wired(handle.clone(), |s| s.open = Some(OpenMenu::Context)),
            )),
        );
        let sectioned_item = component(
            ListTile::new("An item with a sectioned menu").with_trailing(component(
                PopupMenuButton::new(base + 1)
                    .wired(handle.clone(), |s| s.open = Some(OpenMenu::Sectioned)),
            )),
        );
        let checklist_item = component(
            ListTile::new("An item with a checklist menu").with_trailing(component(
                PopupMenuButton::new(base + 2)
                    .wired(handle.clone(), |s| s.open = Some(OpenMenu::Checklist)),
            )),
        );
        // `_SimpleMenuDemo`: the whole list item is the button, its subtitle
        // the current value.
        let simple_item = component(
            PopupMenuButton::new(base + 3)
                .with_child(component(
                    ListTile::new("An item with a simple menu")
                        .with_subtitle(simple_label(state.simple)),
                ))
                .wired(handle.clone(), |s| s.open = Some(OpenMenu::Simple)),
        );

        let items = column(
            vec![context_item, sectioned_item, checklist_item, simple_item],
            0.0,
        );
        // The stack needs some height of its own: an open menu hangs below
        // its row, and the snackbar pins to the bottom edge. Upstream gets
        // both from the screen; the stage is only as tall as its content
        // otherwise.
        let base_layer = single(items, |inner| {
            Box::new(Container::new().with_height(360.0).with_child(inner))
        });

        // Each layer with where it sits; `None` is the unpositioned base the
        // stack sizes itself to. The positions ride alongside the widgets and
        // are applied to the rendered children, in the same order.
        let mut layers: Vec<AnyWidget> = Vec::new();
        let mut positions: Vec<Option<StackPosition>> = Vec::new();
        layers.push(base_layer);
        positions.push(None);
        if let Some(open) = state.open {
            // The route's barrier: a tap off the menu dismisses it without a
            // selection.
            layers.push(component(
                Scrim::new(ids::SCRIM).wired(handle.clone(), |s| s.open = None),
            ));
            positions.push(Some(Positioned::fill()));
            layers.push(open_menu(open, state, handle.clone()));
            positions.push(Some(menu_position(open)));
        }
        if let Some(message) = &state.snackbar {
            // Tap to dismiss; upstream's four-second timer has no clock here.
            layers.push(component(
                Snackbar::new(base + 40, message.clone())
                    .wired(handle.clone(), |s| s.snackbar = None),
            ));
            positions.push(Some(StackPosition {
                left: Some(16.0),
                right: Some(16.0),
                bottom: Some(16.0),
                ..Default::default()
            }));
        }

        many(layers, move |rendered| {
            let mut stack = Stack::new();
            for (layer, position) in rendered.into_iter().zip(positions.iter()) {
                stack = match position {
                    Some(position) => stack.push_positioned(layer, *position),
                    None => stack.push(layer),
                };
            }
            Box::new(stack)
        })
    }
}

/// Where an open menu goes: next to the row that opened it, on the side its
/// button is on.
///
/// Upstream anchors the route to the button's measured rect
/// (`PopupMenuButtonState.showButtonMenu` builds a `RelativeRect` from the
/// box's `localToGlobal`); a build here cannot read a sibling's rect, so the
/// offsets are the rows' known geometry instead: the items are stacked from
/// the top with no spacing, a one-line `ListTile` is about 56 tall and the
/// simple item (with its subtitle) about 72.
fn menu_position(menu: OpenMenu) -> StackPosition {
    let top = match menu {
        OpenMenu::Context => 0.0,
        OpenMenu::Sectioned => 56.0,
        OpenMenu::Checklist => 112.0,
        OpenMenu::Simple => 168.0,
    };
    match menu {
        // The trailing buttons sit at the row's right edge; the menu grows
        // left from it.
        OpenMenu::Context | OpenMenu::Sectioned | OpenMenu::Checklist => StackPosition {
            right: Some(16.0),
            top: Some(top),
            ..Default::default()
        },
        // The simple menu's button is the whole row; upstream aligns the menu
        // over the item's center line with the current value highlighted.
        OpenMenu::Simple => StackPosition {
            left: Some(16.0),
            top: Some(top),
            ..Default::default()
        },
    }
}

/// The open menu itself, entries wired the way upstream's `onSelected` wires
/// them: every selection closes the menu and shows the snackbar.
fn open_menu(
    menu: OpenMenu,
    state: &MenuDemoState,
    handle: StateHandle<MenuDemoState>,
) -> AnyWidget {
    let base = ids::DEMO_LOCAL + 10;

    let popup: AnyWidget = match menu {
        OpenMenu::Context => {
            let select = |s: &mut MenuDemoState, value: &'static str| {
                s.open = None;
                show_in_snackbar(s, format!("Selected: {value}"));
            };
            component(
                PopupMenu::new()
                    .push(
                        PopupMenuItem::new(base, "Context menu item one", "Context menu item one")
                            .wired(handle.clone(), select),
                    )
                    .push(
                        PopupMenuItem::new(base + 1, "Disabled menu item", "Disabled menu item")
                            .with_enabled(false),
                    )
                    .push(
                        PopupMenuItem::new(
                            base + 2,
                            "Context menu item three",
                            "Context menu item three",
                        )
                        .wired(handle.clone(), select),
                    ),
            )
        }
        OpenMenu::Sectioned => {
            let select = |s: &mut MenuDemoState, value: &'static str| {
                s.open = None;
                show_in_snackbar(s, format!("Selected: {value}"));
            };
            component(
                PopupMenu::new()
                    .push(
                        PopupMenuItem::new(base + 3, "Preview", "Preview")
                            .wired(handle.clone(), select),
                    )
                    .push(
                        PopupMenuItem::new(base + 4, "Share", "Share")
                            .wired(handle.clone(), select),
                    )
                    .push(
                        PopupMenuItem::new(base + 5, "Get link", "Get link")
                            .wired(handle.clone(), select),
                    )
                    .push(PopupMenuDivider::new())
                    .push(
                        PopupMenuItem::new(base + 6, "Remove", "Remove")
                            .wired(handle.clone(), select),
                    ),
            )
        }
        OpenMenu::Simple => {
            let select = |s: &mut MenuDemoState, value: SimpleValue| {
                // `showAndSetMenuSelection`: the value first, then the snackbar.
                s.simple = value;
                s.open = None;
                show_in_snackbar(s, format!("Selected: {}", simple_label(value)));
            };
            let mut popup = PopupMenu::new().with_initial_value(state.simple);
            for (offset, value) in [SimpleValue::One, SimpleValue::Two, SimpleValue::Three]
                .into_iter()
                .enumerate()
            {
                popup = popup.push(
                    PopupMenuItem::new(base + 7 + offset as u64, simple_label(value), value)
                        .wired(handle.clone(), select),
                );
            }
            component(popup)
        }
        OpenMenu::Checklist => {
            let mut popup = PopupMenu::new();
            for (offset, value) in CHECKED_VALUES.into_iter().enumerate() {
                let item = CheckedPopupMenuItem::new(
                    base + 10 + offset as u64,
                    checked_label(value),
                    value,
                    state.checked[offset],
                )
                // Upstream's item two is `enabled: false`.
                .with_enabled(value != CheckedValue::Two)
                .wired(handle.clone(), |s, value| {
                    s.open = None;
                    check_toggled(s, value);
                });
                popup = popup.push(item);
            }
            component(popup)
        }
    };

    popup
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_initial_selection_matches_upstream_init_state() {
        let state = MenuDemo.initial_state();
        assert_eq!(state.simple, SimpleValue::Two);
        assert_eq!(state.checked, [false, false, true, false]);
        assert!(state.open.is_none());
        assert!(state.snackbar.is_none());
    }

    #[test]
    fn the_checked_summary_formats_like_a_dart_iterable() {
        assert_eq!(
            checked_summary(&[false, false, true, false]),
            "Checked: (Three)"
        );
        assert_eq!(
            checked_summary(&[true, false, true, true]),
            "Checked: (One, Three, Four)"
        );
        assert_eq!(checked_summary(&[false; 4]), "Checked: ()");
    }

    #[test]
    fn toggling_a_value_checks_and_unchecks_it() {
        let mut state = MenuDemo.initial_state();
        check_toggled(&mut state, CheckedValue::One);
        assert_eq!(state.checked, [true, false, true, false]);
        assert_eq!(state.snackbar.as_deref(), Some("Checked: (One, Three)"));
        check_toggled(&mut state, CheckedValue::Three);
        assert_eq!(state.checked, [true, false, false, false]);
        assert_eq!(state.snackbar.as_deref(), Some("Checked: (One)"));
    }
}
