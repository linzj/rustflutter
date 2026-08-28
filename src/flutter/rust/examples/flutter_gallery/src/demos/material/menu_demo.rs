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
//! * **The menus open at their buttons' measured positions.** Upstream shows a
//!   menu as a route whose `RelativeRect` anchor is measured from the button's
//!   render box, and [`rustflutter::PopupMenuButton`] does the same: the button
//!   records itself on an anchor, the menu goes into the application's overlay,
//!   and `menu.rs`'s `popup_menu_offset` -- which fits the menu on screen -- is
//!   handed the button's rectangle in the overlay's coordinates.
//!
//!   This paragraph used to say there was no route and no way to read a
//!   sibling's rect at build time, so an open menu was stacked over the stage
//!   at a fixed offset per row -- a table of 0, 56, 112 and 168 derived from
//!   the rows' known heights -- and the fitting math had no anchor to work
//!   from. The table is deleted.
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
use rustflutter::widgets::Stack;

use rustflutter::popup::{PopupMenuButton as LiveMenuButton, PopupMenuOpener};
use rustflutter::OverlayHandle;

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

/// Which of the four demos' menus a button opens.
///
/// It used to also be *which one is open*, kept in `MenuDemoState`, because the
/// stage had to build the open menu itself. The overlay holds it now.
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
        context: &mut BuildContext,
    ) -> AnyWidget {
        let base = ids::DEMO_LOCAL;
        let overlay = OverlayHandle::of(context);
        let simple = state.simple;
        let checked = state.checked;

        // The four items, in upstream's configuration order: context menu,
        // sectioned menu, checklist menu, simple menu.
        //
        // Each is a live `PopupMenuButton`. For the first three, upstream's
        // `ListTile(trailing: PopupMenuButton(...))`: the button wraps only
        // the trailing glyph, so the anchor it records is the glyph's rect
        // and the menu opens beside it. Wrapping the whole row instead puts
        // the menu at the row's top-left corner. For the simple demo the
        // whole row is the button, as upstream's `_SimpleMenuDemo` has it.
        let context_item = component(ListTile::new("An item with a context menu").with_trailing(
            anchored_menu(
                context,
                overlay.clone(),
                |open| component(PopupMenuButton::new(base).on_press(open)),
                {
                    let handle = handle.clone();
                    move || open_menu(OpenMenu::Context, simple, checked, handle.clone())
                },
            ),
        ));
        let sectioned_item = component(
            ListTile::new("An item with a sectioned menu").with_trailing(anchored_menu(
                context,
                overlay.clone(),
                |open| component(PopupMenuButton::new(base + 1).on_press(open)),
                {
                    let handle = handle.clone();
                    move || open_menu(OpenMenu::Sectioned, simple, checked, handle.clone())
                },
            )),
        );
        let checklist_item = component(
            ListTile::new("An item with a checklist menu").with_trailing(anchored_menu(
                context,
                overlay.clone(),
                |open| component(PopupMenuButton::new(base + 2).on_press(open)),
                {
                    let handle = handle.clone();
                    move || open_menu(OpenMenu::Checklist, simple, checked, handle.clone())
                },
            )),
        );
        // `_SimpleMenuDemo`: the whole list item is the button, its subtitle
        // the current value.
        let simple_item = anchored_menu(
            context,
            overlay.clone(),
            move |open| {
                component(
                    PopupMenuButton::new(base + 3)
                        .with_child(component(
                            ListTile::new("An item with a simple menu")
                                .with_subtitle(simple_label(simple)),
                        ))
                        .on_press(open),
                )
            },
            {
                let handle = handle.clone();
                move || open_menu(OpenMenu::Simple, simple, checked, handle.clone())
            },
        );

        let items = column(
            vec![context_item, sectioned_item, checklist_item, simple_item],
            0.0,
        );
        // The stack needs some height of its own so the snackbar has an edge to
        // pin to; upstream gets it from the screen. It used to also have to be
        // tall enough for an open menu to hang into, which the overlay handles
        // now.
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

/// Builds a row and the menu it opens, wired together.
///
/// The child is built *by* the caller and *inside* the menu button, which is
/// the whole point: the menu is built where the button is, so it inherits the
/// row's context, and the button records itself on the anchor the menu is
/// placed against.
///
/// `context` is the demo's, and is the one thing the menu cannot inherit: it
/// goes up in the application's overlay, above the demo page's theme rather
/// than below it. The button captures the themes here and the overlay entry
/// puts them back -- upstream's `InheritedTheme.capture`, taken at the same
/// place its `showMenu` takes it.
///
/// The opener is made fresh each build. That is safe because a menu's barrier
/// covers the button that opened it -- there is no way to press it again while
/// it is up -- and a selection closes the topmost modal rather than going back
/// through the opener that happens to be current.
fn anchored_menu(
    context: &BuildContext,
    overlay: Option<std::rc::Rc<OverlayHandle>>,
    child: impl FnOnce(Box<dyn Fn()>) -> AnyWidget,
    menu: impl Fn() -> AnyWidget + 'static,
) -> AnyWidget {
    let opener: std::rc::Rc<std::cell::RefCell<Option<PopupMenuOpener>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let press = {
        let opener = std::rc::Rc::clone(&opener);
        let overlay = overlay.clone();
        Box::new(move || {
            let (Some(overlay), Some(opener)) = (overlay.clone(), opener.borrow().clone()) else {
                return;
            };
            opener.open(overlay);
        }) as Box<dyn Fn()>
    };
    let (widget, made) = LiveMenuButton::new(child(press), menu).build(context);
    *opener.borrow_mut() = Some(made);
    widget
}

/// The open menu itself, entries wired the way upstream's `onSelected` wires
/// them: every selection closes the menu and shows the snackbar.
/// Upstream builds `itemBuilder` once, when the route is pushed, so the values
/// the menu shows are the ones that were current at the press. Passed by value
/// for the same reason.
fn open_menu(
    menu: OpenMenu,
    simple: SimpleValue,
    checked: [bool; 4],
    handle: StateHandle<MenuDemoState>,
) -> AnyWidget {
    let base = ids::DEMO_LOCAL + 10;

    let popup: AnyWidget = match menu {
        OpenMenu::Context => {
            let select = |s: &mut MenuDemoState, value: &'static str| {
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
                show_in_snackbar(s, format!("Selected: {}", simple_label(value)));
            };
            let mut popup = PopupMenu::new().with_initial_value(simple);
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
                    checked[offset],
                )
                // Upstream's item two is `enabled: false`.
                .with_enabled(value != CheckedValue::Two)
                .wired(handle.clone(), |s, value| check_toggled(s, value));
                popup = popup.push(item);
            }
            component(popup)
        }
    };

    // Upstream's menu closes itself: `PopupMenuItemState.handleTap` pops the
    // route before `onSelected` runs. `PopupMenuItem::wired` does the same
    // here -- it dismisses the topmost modal, which is this menu, and then
    // runs the selection. Nothing further to wire.
    popup
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::engine::Rect;
    use rustflutter::render::{EdgeInsets, Size};
    use rustflutter::TextDirection;

    #[test]
    fn the_initial_selection_matches_upstream_init_state() {
        let state = MenuDemo.initial_state();
        assert_eq!(state.simple, SimpleValue::Two);
        assert_eq!(state.checked, [false, false, true, false]);
        assert!(state.snackbar.is_none());
    }

    #[test]
    fn a_menu_goes_where_its_button_is_and_not_at_a_table_of_offsets() {
        // The plan's first named symptom, and the thing this demo was chosen to
        // validate: `popup_menu_offset` wants the button's rectangle in the
        // overlay's coordinates, and until L0 nothing could produce one. The
        // demo used to substitute a table -- 0, 56, 112, 168 -- derived from the
        // rows' known heights.
        //
        // Two rows at different heights get menus at different places, and the
        // difference is the difference between the rows.
        let overlay = Size::new(800.0, 600.0);
        let menu = Size::new(200.0, 120.0);
        let first = rustflutter::popup::menu_offset_for(
            overlay,
            Rect::xywh(740.0, 8.0, 40.0, 48.0),
            menu,
            EdgeInsets::ZERO,
            TextDirection::Ltr,
        );
        let third = rustflutter::popup::menu_offset_for(
            overlay,
            Rect::xywh(740.0, 120.0, 40.0, 48.0),
            menu,
            EdgeInsets::ZERO,
            TextDirection::Ltr,
        );
        assert_eq!(third.dy - first.dy, 112.0, "the rows are 112 apart");
        assert_eq!(first.dx, third.dx, "and in the same column");
    }

    #[test]
    fn a_menu_near_the_bottom_is_pulled_back_on_screen() {
        // The fitting math the old shape could not reach at all: a fixed
        // offset table has no idea where the screen ends.
        let overlay = Size::new(800.0, 600.0);
        let menu = Size::new(200.0, 200.0);
        let at = rustflutter::popup::menu_offset_for(
            overlay,
            Rect::xywh(740.0, 560.0, 40.0, 48.0),
            menu,
            EdgeInsets::ZERO,
            TextDirection::Ltr,
        );
        assert!(
            at.dy + menu.height <= overlay.height,
            "the menu fits on the screen: {at:?}"
        );
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
