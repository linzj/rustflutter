// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/navigation_rail_demo.dart` (flutter/gallery
//! @ d12640d), aligned with upstream.
//!
//! Upstream's `_NavRailDemoState.build` is a `Scaffold` whose body is a `Row`:
//! a `NavigationRail` with a `FloatingActionButton` leading and
//! `labelType: NavigationRailLabelType.selected`, a `VerticalDivider`, and an
//! `Expanded(Center(Text(selected label)))`. The selected index is the demo's
//! own state, upstream's `RestorableInt _selectedIndex` starting at 0.
//!
//! Divergences, each also marked at its site:
//!
//! * The `Scaffold`/`AppBar` (title "Navigation Rail") is the demo page's own
//!   chrome now (`pages/demo.rs`); the stage starts at the body `Row`.
//! * The framework's `NavigationRail` (`controls.rs`) has no `leading` slot
//!   and always shows every label, so the rail is assembled by hand here --
//!   FAB on top, then the destinations with only the selected one labelled --
//!   following the framework rail's own styling (80 wide, the pill mark, the
//!   spacing) so the two read alike.
//! * `Destination.mark` is a one- or two-character stand-in for an icon
//!   (`controls.rs` has no icon font), so the destinations are marked "F",
//!   "B", "S" for upstream's `Icons.favorite`/`bookmark`/`star` pair; the
//!   outlined/unoutlined icon swap on selection is not expressible.
//! * The FAB's `onPressed: () {}` is as empty here as it is upstream; its
//!   tooltip ("Create", `buttonTextCreate`) shows on hover, listed at the
//!   bottom of the rail's column rather than floating beside the FAB -- the
//!   framework's `Tooltip` is a bubble the application stacks itself, and the
//!   rail has no overlay of its own.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, Empty};

use crate::app::{ids, GalleryState};
use crate::data::demos::{self as catalog, icon};

use super::DemoState;

/// Upstream's destination labels: `demoNavigationRailFirst/Second/Third`.
const DESTINATIONS: [(&str, &str); 3] = [("First", "F"), ("Second", "B"), ("Third", "S")];

/// The demo body for the `nav_rail` slug.
///
/// The dispatch in `mod.rs` hands every demo the shared `DemoState`; this
/// demo's selection is its own `State` instead, the way upstream's
/// `_NavRailDemoState` owns `_selectedIndex`, so the arguments go unused.
pub(super) fn navigation_rail(
    _state: &DemoState,
    _pressed: Option<u64>,
    _handle: StateHandle<GalleryState>,
) -> AnyWidget {
    stateful(NavRailDemo)
}

/// Upstream's `NavRailDemo`.
struct NavRailDemo;

/// Upstream's `_NavRailDemoState`: `_selectedIndex`, plus whether the FAB's
/// tooltip is showing (upstream's `Tooltip` state, the application's here).
#[derive(Default)]
struct NavRailState {
    selected: usize,
    tooltip: bool,
}

impl StatefulComponent for NavRailDemo {
    type State = NavRailState;

    fn build(
        &self,
        state: &NavRailState,
        handle: StateHandle<NavRailState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let selected = state.selected.min(DESTINATIONS.len() - 1);

        let rail = component(Rail {
            selected,
            tooltip: state.tooltip,
            handle,
        });

        // Upstream's `VerticalDivider(thickness: 1, width: 1)`.
        let outline = theme.outline;
        let divider = leaf(move || Container::new().with_width(1.0).with_color(outline));

        // Upstream's `Expanded(Center(Text(selectedItem[_selectedIndex])))`.
        let label = DESTINATIONS[selected].0;
        let body_style = theme.body();
        let body = leaf(move || {
            Align::new(
                Alignment::CENTER,
                Text::new(label).with_style(body_style.clone()),
            )
        });

        many(vec![rail, divider, body], |mut rendered| {
            let body = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let divider = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let rail = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(rail)
                    .push(divider)
                    .push_flex(FlexChild::expanded(body, 1)),
            )
        })
    }
}

/// The rail, hand-assembled: the framework's `NavigationRail` has no
/// `leading` slot for upstream's FAB and always shows every label, where
/// upstream's `labelType: NavigationRailLabelType.selected` labels only the
/// selected destination. The styling follows `controls.rs`'s rail: 80 wide,
/// the 34x30 pill behind the mark, the same paddings and spacing.
struct Rail {
    selected: usize,
    tooltip: bool,
    handle: StateHandle<NavRailState>,
}

impl Component for Rail {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let tooltip = self.tooltip;
        let surface = theme.surface;
        let outline = theme.outline;
        let primary = theme.primary;
        let on_primary = theme.on_primary;
        let spacing = theme.spacing;

        // Upstream's `leading: FloatingActionButton(...)`: 56 across, the add
        // glyph, a "Create" tooltip. Its `onPressed` is empty upstream too.
        let fab_handle = self.handle.clone();
        let fab = component(
            TooltipTrigger::new(
                ids::DEMO_LOCAL + 10,
                leaf(move || {
                    Container::new()
                        .with_size(56.0, 56.0)
                        .with_color(primary)
                        .with_corner_radius(28.0)
                        .with_elevation(6)
                        .with_child(Align::new(
                            Alignment::CENTER,
                            Text::new(icon::ADD)
                                .with_font_family(catalog::MATERIAL_ICONS)
                                .with_size(24.0)
                                .with_color(on_primary),
                        ))
                }),
            )
            .wired(fab_handle, |state, show| state.tooltip = show),
        );

        let mut destinations: Vec<AnyWidget> = Vec::new();
        for (index, (label, mark)) in DESTINATIONS.iter().enumerate() {
            let handle = self.handle.clone();
            let handlers = rustflutter::gestures::PointerHandlers::new().with_tap(move |_| {
                // Upstream's `onDestinationSelected`.
                handle.set_state(move |state| state.selected = index);
            });
            destinations.push(component(RailDestination {
                id: ids::DEMO_LOCAL + index as u64,
                label,
                mark,
                selected: index == selected,
                handlers,
            }));
        }

        let mut children = vec![fab];
        children.extend(destinations);
        if tooltip {
            children.push(component(TooltipBubble::new("Create")));
        }

        many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.0);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(
                Container::new()
                    // Upstream `NavigationRail`'s collapsed width.
                    .with_width(80.0)
                    .with_color(surface)
                    .with_border(1.0, outline)
                    .with_padding(EdgeInsets::symmetric(0.0, spacing))
                    .with_child(column),
            )
        })
    }
}

/// One destination: the pill-marked glyph, and -- only when selected,
/// upstream's `NavigationRailLabelType.selected` -- its label.
struct RailDestination {
    id: u64,
    label: &'static str,
    mark: &'static str,
    selected: bool,
    handlers: rustflutter::gestures::PointerHandlers,
}

impl Component for RailDestination {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let label = self.label;
        let mark = self.mark;
        let id = self.id;
        let handlers = self.handlers.clone();
        let primary = theme.primary;
        let muted = theme.text_muted;
        let spacing = theme.spacing;
        let color = if selected { primary } else { muted };

        leaf(move || {
            // The pill behind the mark, as `controls.rs`'s rail draws it.
            let pill = Container::new()
                .with_size(34.0, 30.0)
                .with_color(if selected {
                    primary.with_alpha(0x33)
                } else {
                    Color::TRANSPARENT
                })
                .with_corner_radius(10.0)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(mark)
                        .with_size(12.0)
                        .with_weight(700)
                        .with_color(color),
                ));
            let mut content = Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_spacing(2.0)
                .push(pill);
            if selected {
                content = content.push(
                    Text::new(label)
                        .with_size(10.0)
                        .with_weight(700)
                        .with_color(color),
                );
            }
            rustflutter::widgets::Pointer::new(
                id,
                Container::new()
                    .with_padding(EdgeInsets::symmetric(spacing, spacing * 0.75))
                    .with_child(Align::new(Alignment::CENTER, content)),
            )
            .with_handlers(handlers.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selection_starts_at_zero() {
        // Upstream's `RestorableInt(0)`.
        assert_eq!(NavRailState::default().selected, 0);
    }

    #[test]
    fn there_are_three_destinations() {
        assert_eq!(DESTINATIONS.len(), 3);
        assert_eq!(
            DESTINATIONS
                .iter()
                .map(|(label, _)| *label)
                .collect::<Vec<_>>(),
            vec!["First", "Second", "Third"]
        );
    }

    #[test]
    fn only_the_selected_destination_is_labelled() {
        // Upstream's `NavigationRailLabelType.selected`: build one selected
        // and one unselected destination; the selected one is taller by a
        // label's height.
        use rustflutter::framework::ElementTree;
        use rustflutter::render::{BoxConstraints, RenderBox};

        let height_of = |selected: bool| {
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                Theme::dark(),
                component(RailDestination {
                    id: 1,
                    label: "First",
                    mark: "F",
                    selected,
                    handlers: rustflutter::gestures::PointerHandlers::new(),
                }),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::new(0.0, 200.0, 0.0, f32::INFINITY))
                .height
        };
        assert!(height_of(true) > height_of(false));
    }
}
