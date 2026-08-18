// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/bottom_app_bar_demo.dart` (flutter/gallery
//! @ d12640d), aligned with upstream.
//!
//! Upstream the demo is a `Scaffold` with a "Bottom app bar" app bar, a
//! `ListView` of two `SwitchListTile`s (floating action button, notch) and
//! four `RadioListTile`s picking the button's location, a
//! `FloatingActionButton` at that location, and a `_DemoBottomAppBar` whose
//! icons leave a gap when the button docks or floats in the centre. The state
//! is upstream's `_BottomAppBarDemoState` (`_showFab`, `_showNotch`,
//! `_currentFabLocation`), kept here in [`BottomAppBarDemoState`]; the
//! `RestorationMixin` around it has no counterpart and is not carried.
//!
//! Divergences from upstream, each also marked at its site:
//!
//! * The notch (`CircularNotchedRectangle`) is drawn as a background-coloured
//!   circle behind the docked button: the framework's clip shapes are
//!   rectangles and rounded rectangles, so a bar with a bite taken out of it
//!   cannot be clipped honestly.
//! * The floating-action-button locations are geometric approximations of
//!   `endDocked`/`centerDocked`/`endFloat`/`centerFloat`: those are
//!   `Scaffold` layout delegates upstream, and the demo here composes the
//!   pieces itself.
//! * The icon buttons and the FAB are drawn but not wired; upstream's
//!   callbacks are empty (`onPressed: () {}`), so all an unwired tap cannot
//!   do is splash. Their tooltips are likewise not shown: a tooltip bubble
//!   needs the button's position, which a build does not have.
//! * The body is a fixed height rather than the screen's remainder: the demo
//!   renders in a card on the demo page, not on a display of its own.

use rustflutter::components::K_TOOLBAR_HEIGHT;
use rustflutter::framework::{BuildContext, StatefulComponent};
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxConstraints, CrossAxisAlignment, EdgeInsets, FlexChild, MainAxisSize,
    RenderConstrainedBox, RenderFlex, StackPosition,
};
use rustflutter::widgets::{Align, Container, RenderNavigationToolbar, Text, K_MIDDLE_SPACING};

use crate::app::ids;
use crate::data::demos::{self, icon};
use crate::themes::material_demo_theme_data::{MaterialDemoThemeData, COLOR_SCHEME};

/// The demo body for the `bottom-app-bar` slug. It takes nothing from the
/// shared `DemoState`: everything it remembers is upstream's
/// `_BottomAppBarDemoState`, kept in [`BottomAppBarDemoState`].
pub(super) fn stage() -> AnyWidget {
    stateful(BottomAppBarDemo)
}

/// Upstream's `BottomAppBarDemo`.
struct BottomAppBarDemo;

/// Upstream's `_BottomAppBarDemoState`, minus the restoration wrappers.
#[derive(Clone, Copy, Debug)]
struct BottomAppBarDemoState {
    show_fab: bool,
    show_notch: bool,
    /// The index into [`FAB_LOCATIONS`], upstream's `_currentFabLocation`.
    fab_location: usize,
}

impl Default for BottomAppBarDemoState {
    fn default() -> BottomAppBarDemoState {
        // Upstream's three `Restorable*` initial values.
        BottomAppBarDemoState {
            show_fab: true,
            show_notch: true,
            fab_location: 0,
        }
    }
}

/// Upstream's `_fabLocations`, as data: whether the button docks into the
/// bar or floats above it, and whether it sits at the end or the centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FabLocation {
    docked: bool,
    centered: bool,
}

/// The four locations, in upstream's order: `endDocked`, `centerDocked`,
/// `endFloat`, `centerFloat`.
const FAB_LOCATIONS: [FabLocation; 4] = [
    FabLocation {
        docked: true,
        centered: false,
    },
    FabLocation {
        docked: true,
        centered: true,
    },
    FabLocation {
        docked: false,
        centered: false,
    },
    FabLocation {
        docked: false,
        centered: true,
    },
];

/// The radio labels, in upstream's order
/// (`bottomAppBarPositionDockedEnd` and friends).
const LOCATION_LABELS: [&str; 4] = [
    "Docked - End",
    "Docked - Center",
    "Floating - End",
    "Floating - Center",
];

/// A floating action button is 56 across, upstream's default `FABSize`.
const FAB_SIZE: f32 = 56.0;
/// The bottom app bar's height, upstream's `BottomAppBar` default.
const BAR_HEIGHT: f32 = 56.0;
/// The gap a floating button leaves above the bar, upstream's
/// `kFloatingActionButtonMargin`.
const FAB_MARGIN: f32 = 16.0;

/// Where the bar and the button sit in the bottom region, and how tall the
/// region is. Docked, the button's middle is the bar's top edge; floating,
/// the button sits a margin above the bar.
fn bottom_region(location: FabLocation) -> (f32, f32, f32) {
    if location.docked {
        // Region 84 tall: the button rides the top half, the bar the rest.
        (FAB_SIZE / 2.0 + BAR_HEIGHT, 0.0, FAB_SIZE / 2.0)
    } else {
        (
            FAB_SIZE + FAB_MARGIN + BAR_HEIGHT,
            0.0,
            FAB_SIZE + FAB_MARGIN,
        )
    }
}

/// A 48-by-48 icon button, upstream's `IconButton`. Drawn but not wired --
/// see the module header.
fn icon_button(glyph: &str, color: Color) -> AnyWidget {
    let glyph = glyph.to_string();
    leaf(move || {
        Container::new()
            .with_size(48.0, 48.0)
            .with_child(Align::new(
                Alignment::CENTER,
                Text::new(glyph.clone())
                    .with_font_family(demos::MATERIAL_ICONS)
                    .with_size(24.0)
                    .with_color(color),
            ))
    })
}

/// The floating action button: a 56 circle in the scheme's secondary colour
/// (the demo theme sets no `floatingActionButtonTheme`, so the M2 default
/// applies), with the add glyph in on-secondary. Upstream's `tooltip` is
/// "Create" (`buttonTextCreate`); see the module header for why no bubble.
fn fab() -> AnyWidget {
    leaf(|| {
        Container::new()
            .with_size(FAB_SIZE, FAB_SIZE)
            .with_color(COLOR_SCHEME.secondary)
            .with_corner_radius(FAB_SIZE / 2.0)
            .with_elevation(6)
            .with_child(Align::new(
                Alignment::CENTER,
                Text::new(icon::ADD)
                    .with_font_family(demos::MATERIAL_ICONS)
                    .with_size(24.0)
                    .with_color(COLOR_SCHEME.on_secondary),
            ))
    })
}

/// Upstream's `_DemoBottomAppBar`: menu, search and favorite icons, with a
/// gap in the middle when the button is centred. The bar's colour is the
/// demo theme's `bottomAppBarTheme` (primary) and its icons are on-primary,
/// upstream's `IconTheme` override.
fn demo_bottom_app_bar(centered: bool) -> AnyWidget {
    let bar_color = MaterialDemoThemeData::bottom_app_bar_color();
    let ink = COLOR_SCHEME.on_primary;
    let mut children = vec![icon_button(icon::MENU, ink)];
    if centered {
        // Upstream's `if (centerLocations.contains(fabLocation))
        // const Spacer()`.
        children.push(leaf(|| Container::new()));
    }
    children.push(icon_button(icon::SEARCH, ink));
    children.push(icon_button(icon::FAVORITE, ink));

    many(children, move |mut rendered| {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        if centered {
            let favorite = rendered.pop().expect("the favorite button");
            let search = rendered.pop().expect("the search button");
            let spacer = rendered.pop().expect("the spacer");
            let menu = rendered.pop().expect("the menu button");
            row = row
                .push(menu)
                .push_flex(FlexChild::expanded(spacer, 1))
                .push(search)
                .push(favorite);
        } else {
            for child in rendered {
                row = row.push(child);
            }
        }
        Box::new(
            Container::new().with_color(bar_color).with_child(
                RenderConstrainedBox::new(BoxConstraints::new(
                    0.0,
                    f32::INFINITY,
                    BAR_HEIGHT,
                    BAR_HEIGHT,
                ))
                .with_child(row),
            ),
        )
    })
}

impl StatefulComponent for BottomAppBarDemo {
    type State = BottomAppBarDemoState;

    fn build(
        &self,
        state: &BottomAppBarDemoState,
        handle: StateHandle<BottomAppBarDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let (bar_fill, bar_ink) = MaterialDemoThemeData::app_bar_theme();
        let background = theme.background;
        let body_style = theme.body();

        // The app bar, upstream's `AppBar(automaticallyImplyLeading: false,
        // title: demoBottomAppBarTitle)`.
        let title = leaf(move || {
            Text::new("Bottom app bar")
                .with_size(20.0)
                .with_weight(500)
                .with_color(bar_ink)
        });
        let app_bar = many(vec![title], move |mut rendered| {
            let toolbar = RenderNavigationToolbar::new()
                .with_center_middle(false)
                .with_middle_spacing(K_MIDDLE_SPACING)
                .with_middle(rendered.pop().expect("the title"));
            Box::new(
                Container::new().with_color(bar_fill).with_child(
                    RenderConstrainedBox::new(BoxConstraints::new(
                        0.0,
                        f32::INFINITY,
                        K_TOOLBAR_HEIGHT,
                        K_TOOLBAR_HEIGHT,
                    ))
                    .with_child(toolbar),
                ),
            )
        });

        // The body, upstream's `ListView`: two `SwitchListTile`s, the
        // position header and four `RadioListTile`s.
        let mut controls: Vec<AnyWidget> = vec![
            component(
                ListTile::new("Floating Action Button").with_trailing(component(
                    Switch::new(ids::DEMO_LOCAL, state.show_fab)
                        .wired(handle.clone(), |state| state.show_fab = !state.show_fab),
                )),
            ),
            component(
                ListTile::new("Notch").with_trailing(component(
                    Switch::new(ids::DEMO_LOCAL + 1, state.show_notch)
                        .wired(handle.clone(), |state| state.show_notch = !state.show_notch),
                )),
            ),
            leaf(move || {
                Container::new()
                    .with_padding(EdgeInsets::all(16.0))
                    .with_child(Align::new(
                        Alignment::CENTER_LEFT,
                        Text::new("Floating Action Button Position").with_style(body_style.clone()),
                    ))
            }),
        ];
        // The four `RadioListTile`s, written out the way upstream writes
        // them: `Radio::wired` takes a `fn` pointer, so each button's value
        // is a literal rather than a loop's capture.
        controls.push(component(
            Radio::new(ids::DEMO_LOCAL + 2, state.fab_location == 0)
                .with_label(LOCATION_LABELS[0])
                .wired(handle.clone(), |state| state.fab_location = 0),
        ));
        controls.push(component(
            Radio::new(ids::DEMO_LOCAL + 3, state.fab_location == 1)
                .with_label(LOCATION_LABELS[1])
                .wired(handle.clone(), |state| state.fab_location = 1),
        ));
        controls.push(component(
            Radio::new(ids::DEMO_LOCAL + 4, state.fab_location == 2)
                .with_label(LOCATION_LABELS[2])
                .wired(handle.clone(), |state| state.fab_location = 2),
        ));
        controls.push(component(
            Radio::new(ids::DEMO_LOCAL + 5, state.fab_location == 3)
                .with_label(LOCATION_LABELS[3])
                .wired(handle.clone(), |state| state.fab_location = 3),
        ));
        // The fixed height is the stand-in for the screen's remainder; see
        // the module header.
        let body = many(controls, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for control in rendered {
                column = column.push(control);
            }
            Box::new(
                Container::new()
                    .with_height(280.0)
                    .with_alignment(Alignment::TOP_LEFT)
                    .with_child(column),
            )
        });

        // The bottom region: the bar, the notch, and the button. The button
        // is a child of the `many` so the assembler sees it as a render
        // object; the notch is drawn directly, being decoration.
        let location = FAB_LOCATIONS[state.fab_location.min(FAB_LOCATIONS.len() - 1)];
        let (region_height, fab_top, bar_top) = bottom_region(location);
        let bar = demo_bottom_app_bar(location.centered);

        let show_fab = state.show_fab;
        let show_notch = state.show_notch;
        let mut region_children = vec![bar];
        if show_fab {
            region_children.push(fab());
        }
        let bottom = many(region_children, move |mut rendered| {
            let button = if show_fab { rendered.pop() } else { None };
            let bar = rendered.pop().expect("the bottom app bar");
            let mut stack = rustflutter::widgets::Stack::new();
            if show_fab && show_notch && location.docked {
                // The notch, approximated: a background-coloured circle
                // behind the button reads as the bite
                // `CircularNotchedRectangle` cuts out of the bar. See the
                // module header for why it is not a real clip.
                let notch = Container::new()
                    .with_size(FAB_SIZE + 8.0, FAB_SIZE + 8.0)
                    .with_color(background)
                    .with_corner_radius((FAB_SIZE + 8.0) / 2.0);
                stack = if location.centered {
                    stack.push_positioned(
                        Align::new(Alignment::TOP_CENTER, notch),
                        StackPosition {
                            left: Some(0.0),
                            right: Some(0.0),
                            top: Some(fab_top - 4.0),
                            ..Default::default()
                        },
                    )
                } else {
                    stack.push_positioned(
                        notch,
                        StackPosition {
                            right: Some(FAB_MARGIN - 4.0),
                            top: Some(fab_top - 4.0),
                            ..Default::default()
                        },
                    )
                };
            }
            stack = stack.push_positioned(
                bar,
                StackPosition {
                    left: Some(0.0),
                    right: Some(0.0),
                    top: Some(bar_top),
                    ..Default::default()
                },
            );
            if let Some(button) = button {
                // Centre by stretching across and aligning, so the button
                // stays 56 wide rather than filling the region.
                stack = if location.centered {
                    stack.push_positioned(
                        Align::new(Alignment::TOP_CENTER, button),
                        StackPosition {
                            left: Some(0.0),
                            right: Some(0.0),
                            top: Some(fab_top),
                            ..Default::default()
                        },
                    )
                } else {
                    stack.push_positioned(
                        button,
                        StackPosition {
                            right: Some(FAB_MARGIN),
                            top: Some(fab_top),
                            ..Default::default()
                        },
                    )
                };
            }
            let region = Container::new()
                .with_height(region_height)
                .with_alignment(Alignment::TOP_LEFT)
                .with_child(stack);
            Box::new(region)
        });

        many(vec![app_bar, body, bottom], move |mut rendered| {
            let bottom = rendered.pop().expect("the bottom region");
            let body = rendered.pop().expect("the body");
            let app_bar = rendered.pop().expect("the app bar");
            Box::new(
                Container::new().with_color(background).with_child(
                    RenderFlex::column()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(app_bar)
                        .push(body)
                        .push(bottom),
                ),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_state_is_upstreams_initial_state() {
        let state = BottomAppBarDemoState::default();
        assert!(state.show_fab);
        assert!(state.show_notch);
        assert_eq!(state.fab_location, 0, "upstream starts at endDocked");
    }

    #[test]
    fn the_locations_are_upstreams_list_in_order() {
        assert_eq!(
            FAB_LOCATIONS[0],
            FabLocation {
                docked: true,
                centered: false
            }
        );
        assert_eq!(
            FAB_LOCATIONS[1],
            FabLocation {
                docked: true,
                centered: true
            }
        );
        assert_eq!(
            FAB_LOCATIONS[2],
            FabLocation {
                docked: false,
                centered: false
            }
        );
        assert_eq!(
            FAB_LOCATIONS[3],
            FabLocation {
                docked: false,
                centered: true
            }
        );
    }

    #[test]
    fn the_center_locations_are_the_two_centred_ones() {
        // Upstream's `_DemoBottomAppBar.centerLocations`: centerDocked and
        // centerFloat, which is what the spacer between the menu and the
        // search icon keys off.
        let centered: Vec<bool> = FAB_LOCATIONS
            .iter()
            .map(|location| location.centered)
            .collect();
        assert_eq!(centered, [false, true, false, true]);
    }

    #[test]
    fn a_docked_button_rides_the_bars_top_edge() {
        let (height, fab_top, bar_top) = bottom_region(FabLocation {
            docked: true,
            centered: false,
        });
        assert_eq!(
            bar_top,
            FAB_SIZE / 2.0,
            "the bar starts under the button's middle"
        );
        assert_eq!(fab_top, 0.0);
        assert_eq!(height, bar_top + BAR_HEIGHT);
    }

    #[test]
    fn a_floating_button_sits_a_margin_above_the_bar() {
        let (height, fab_top, bar_top) = bottom_region(FabLocation {
            docked: false,
            centered: true,
        });
        assert_eq!(bar_top, FAB_SIZE + FAB_MARGIN);
        assert_eq!(fab_top, 0.0);
        assert_eq!(height, bar_top + BAR_HEIGHT);
    }
}
