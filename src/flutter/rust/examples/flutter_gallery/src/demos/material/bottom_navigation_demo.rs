// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/bottom_navigation_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream the file defines one `BottomNavigationDemo` used by two
//! catalogue configurations: `withLabels` ("Persistent labels", three
//! destinations, every label shown) and `withoutLabels` ("Selected label",
//! five destinations, only the selected one labelled). The catalogue here is
//! flattened to one configuration per demo (PORTING.md: "demo options
//! section is unreachable"), so the stage shows both variants, one under the
//! other, each with its own selection -- upstream keeps them in two separate
//! `_BottomNavigationDemoState`s, and so does [`BottomNavigationDemoState`].
//!
//! Divergences from upstream, each also marked at its site:
//!
//! * The destination view's background image
//!   (`assets/demos/bottom_navigation_background.png`) is not shipped with
//!   this port; a primary-tinted panel stands in for it. The 80dp white icon
//!   over it is ported as drawn.
//! * The `PageTransitionSwitcher`/`FadeThroughTransition` between
//!   destinations is not ported: the framework has no implicit cross-fade.
//!   The view is keyed by the selection instead, so a change swaps the
//!   element rather than mutating it in place -- the same stand-in
//!   `mod.rs`'s `FadedPanel` documents.
//! * The restoration machinery (`RestorationMixin`, `_currentIndex` as a
//!   `RestorableInt`) has no counterpart and is not carried.

use rustflutter::components::K_TOOLBAR_HEIGHT;
use rustflutter::framework::{BuildContext, Key, StatefulComponent};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    BoxConstraints, CrossAxisAlignment, EdgeInsets, MainAxisSize, RenderConstrainedBox, RenderFlex,
};
use rustflutter::widgets::{
    Center, Container, Pointer, RenderNavigationToolbar, Text, K_MIDDLE_SPACING,
};

use crate::app::{ids, GalleryState};
use crate::data::demos;
use crate::themes::material_demo_theme_data::MaterialDemoThemeData;

use super::{column, DemoState};

/// The demo body for the `bottom-navigation` slug. `DemoState.bottom_nav`
/// served the aggregate port; the aligned demo keeps upstream's per-variant
/// `_currentIndex` in [`BottomNavigationDemoState`] instead.
pub(super) fn bottom_navigation(
    _state: &DemoState,
    _handle: StateHandle<GalleryState>,
) -> AnyWidget {
    stateful(BottomNavigationDemoStage)
}

/// The stage holding both variants, since the flattened catalogue cannot
/// offer them separately. Upstream: two entries in `lib/data/demos.dart`
/// building `BottomNavigationDemo` with each `BottomNavigationDemoType`.
struct BottomNavigationDemoStage;

/// Upstream's two `_BottomNavigationDemoState._currentIndex` fields.
#[derive(Clone, Copy, Debug, Default)]
struct BottomNavigationDemoState {
    with_labels_index: usize,
    without_labels_index: usize,
}

/// Upstream's `BottomNavigationDemoType` (`material_demo_types.dart`, where
/// the enum itself also lives).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BottomNavigationDemoType {
    WithLabels,
    WithoutLabels,
}

/// One destination: the label and the glyph standing in for its icon.
struct DestinationItem {
    label: &'static str,
    glyph: &'static str,
}

/// Upstream's `bottomNavigationBarItems`: Comments, Calendar, Account, Alarm
/// and Camera, with `Icons.add_comment`, `Icons.calendar_today`,
/// `Icons.account_circle`, `Icons.alarm_on` and `Icons.camera_enhance`. The
/// codepoints are the shipped font's, resolved against
/// `assets/fonts/MaterialIcons-Regular.otf` (`data/demos.rs`'s `icon` module
/// does not carry these five).
const DESTINATIONS: [DestinationItem; 5] = [
    DestinationItem {
        label: "Comments",
        glyph: "\u{e051}",
    },
    DestinationItem {
        label: "Calendar",
        glyph: "\u{e122}",
    },
    DestinationItem {
        label: "Account",
        glyph: "\u{e043}",
    },
    DestinationItem {
        label: "Alarm",
        glyph: "\u{e075}",
    },
    DestinationItem {
        label: "Camera",
        glyph: "\u{e131}",
    },
];

/// The destinations a variant shows. Upstream the `withLabels` build takes
/// the first three (`sublist(0, length - 2)`); `withoutLabels` takes all
/// five.
fn visible_destinations(demo_type: BottomNavigationDemoType) -> &'static [DestinationItem] {
    match demo_type {
        BottomNavigationDemoType::WithLabels => &DESTINATIONS[..DESTINATIONS.len() - 2],
        BottomNavigationDemoType::WithoutLabels => &DESTINATIONS,
    }
}

/// Upstream's clamp in the `withLabels` build: an index selected among five
/// destinations is brought back into the three that remain.
fn clamped_index(index: usize, demo_type: BottomNavigationDemoType) -> usize {
    index.min(visible_destinations(demo_type).len() - 1)
}

/// The app bar's title per variant, upstream's `_title`
/// (`demoBottomNavigationPersistentLabels` /
/// `demoBottomNavigationSelectedLabel`).
fn variant_title(demo_type: BottomNavigationDemoType) -> &'static str {
    match demo_type {
        BottomNavigationDemoType::WithLabels => "Persistent labels",
        BottomNavigationDemoType::WithoutLabels => "Selected label",
    }
}

/// The height of the destination view. Upstream it fills the screen's
/// remainder; the demo renders in a card on the demo page, so the remainder
/// is a fixed stand-in.
const DESTINATION_VIEW_HEIGHT: f32 = 180.0;

/// Upstream's `_NavigationDestinationView`: the background panel with the
/// destination's icon centred over it in white at 80.
struct NavigationDestinationView {
    glyph: &'static str,
    key_index: usize,
}

impl Component for NavigationDestinationView {
    /// The key is what a cross-fade would animate between; see the module
    /// header.
    fn key(&self) -> Key {
        Some(self.key_index as u64)
    }

    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let glyph = self.glyph;
        // Upstream's semantics label
        // (`bottomNavigationContentPlaceholder`, "Placeholder for {title}
        // tab") is not set; the destination's label is already beside the
        // icon in the bar below.
        let fill = theme.primary.with_alpha(0x55);
        let radius = 8.0;
        leaf(move || {
            rustflutter::widgets::Stack::new()
                .push(
                    Container::new()
                        .with_height(DESTINATION_VIEW_HEIGHT)
                        .with_color(fill)
                        .with_corner_radius(radius),
                )
                .push(Center::new(
                    Text::new(glyph)
                        .with_font_family(demos::MATERIAL_ICONS)
                        .with_size(80.0)
                        .with_color(Color::WHITE),
                ))
        })
    }
}

/// The variant's bottom bar. Upstream's `BottomNavigationBar` with
/// `backgroundColor: colorScheme.primary`, `selectedItemColor:
/// colorScheme.onPrimary` and the unselected at 38% of it, at
/// `kBottomNavigationBarHeight` -- the M2 bar, so there is no indicator
/// pill; selection shows in the colour alone. The framework's
/// [`BottomNavigation`] draws the M3 bar, which is why this is composed
/// here.
fn bottom_bar(
    demo_type: BottomNavigationDemoType,
    selected: usize,
    first_id: u64,
    handle: StateHandle<BottomNavigationDemoState>,
    select: fn(&mut BottomNavigationDemoState, usize),
    context: &mut BuildContext,
) -> AnyWidget {
    let theme = theme_of(context);
    let bar_color = theme.primary;
    let selected_color = theme.on_primary;
    let unselected_color = theme.on_primary.with_alpha(0x61);
    // Upstream's `BottomNavigationBar` grows by the gesture-bar inset; the
    // same `additionalBottomPadding` the framework's bar takes.
    let bottom = media_query_of(context).view_padding.bottom;
    let show_unselected = demo_type == BottomNavigationDemoType::WithLabels;
    let destinations = visible_destinations(demo_type);

    let mut items: Vec<AnyWidget> = Vec::new();
    for (index, destination) in destinations.iter().enumerate() {
        let active = index == selected;
        let color = if active {
            selected_color
        } else {
            unselected_color
        };
        let glyph = destination.glyph;
        let label = destination.label;
        // `BottomNavigationBarType.fixed` with `showUnselectedLabels:
        // false`: the unselected destinations are the icon alone, and both
        // label sizes are `bodySmall` upstream.
        let show_label = active || show_unselected;
        let tap_handle = handle.clone();
        items.push(leaf(move || {
            // The tap closure takes its own copy of the handle: this `leaf`
            // is `Fn`, so nothing may be moved out of its captures.
            let tap = PointerHandlers::new().with_tap({
                let tap_handle = tap_handle.clone();
                move |_| {
                    tap_handle.set_state(move |state| select(state, index));
                }
            });
            let mut tile = Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(3.0)
                .push(
                    Text::new(glyph)
                        .with_font_family(demos::MATERIAL_ICONS)
                        .with_size(24.0)
                        .with_color(color),
                );
            if show_label {
                tile = tile.push(
                    Text::new(label)
                        .with_size(12.0)
                        .with_weight(if active { 700 } else { 400 })
                        .with_color(color),
                );
            }
            Pointer::new(first_id + index as u64, Center::new(tile)).with_handlers(tap)
        }));
    }

    many(items, move |rendered| {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for item in rendered {
            row = row.push_flex(rustflutter::render::FlexChild::expanded(item, 1));
        }
        Box::new(
            Container::new()
                .with_height(56.0 + bottom)
                .with_color(bar_color)
                .with_padding(EdgeInsets::only(0.0, 0.0, 0.0, bottom))
                .with_child(row),
        )
    })
}

/// One variant of the demo: its app bar, the destination view and the bar.
/// Upstream this is one `BottomNavigationDemo` scaffold.
fn variant_section(
    demo_type: BottomNavigationDemoType,
    selected: usize,
    first_id: u64,
    handle: StateHandle<BottomNavigationDemoState>,
    select: fn(&mut BottomNavigationDemoState, usize),
    context: &mut BuildContext,
) -> AnyWidget {
    let (bar_fill, bar_ink) = MaterialDemoThemeData::app_bar_theme();
    let title = variant_title(demo_type);
    let destinations = visible_destinations(demo_type);
    let destination = &destinations[selected.min(destinations.len() - 1)];

    let app_bar = many(
        vec![leaf(move || {
            Text::new(title)
                .with_size(20.0)
                .with_weight(500)
                .with_color(bar_ink)
        })],
        move |mut rendered| {
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
        },
    );

    let view = component(NavigationDestinationView {
        glyph: destination.glyph,
        key_index: selected,
    });

    let bar = bottom_bar(demo_type, selected, first_id, handle, select, context);

    many(vec![app_bar, view, bar], move |mut rendered| {
        let bar = rendered.pop().expect("the bottom bar");
        let view = rendered.pop().expect("the destination view");
        let app_bar = rendered.pop().expect("the app bar");
        Box::new(
            RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push(app_bar)
                .push(view)
                .push(bar),
        )
    })
}

impl StatefulComponent for BottomNavigationDemoStage {
    type State = BottomNavigationDemoState;

    fn build(
        &self,
        state: &BottomNavigationDemoState,
        handle: StateHandle<BottomNavigationDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let background = theme_of(context).background;
        let with_labels = variant_section(
            BottomNavigationDemoType::WithLabels,
            clamped_index(
                state.with_labels_index,
                BottomNavigationDemoType::WithLabels,
            ),
            ids::DEMO_LOCAL,
            handle.clone(),
            |state, index| state.with_labels_index = index,
            context,
        );
        let without_labels = variant_section(
            BottomNavigationDemoType::WithoutLabels,
            clamped_index(
                state.without_labels_index,
                BottomNavigationDemoType::WithoutLabels,
            ),
            ids::DEMO_LOCAL + 4,
            handle,
            |state, index| state.without_labels_index = index,
            context,
        );
        rustflutter::framework::single(
            column(vec![with_labels, without_labels], 24.0),
            move |inner| Box::new(Container::new().with_color(background).with_child(inner)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variants_split_the_destinations_the_way_upstream_does() {
        // `withLabels` drops the last two (`sublist(0, length - 2)`).
        assert_eq!(
            visible_destinations(BottomNavigationDemoType::WithLabels).len(),
            3
        );
        assert_eq!(
            visible_destinations(BottomNavigationDemoType::WithoutLabels).len(),
            5
        );
        assert_eq!(
            visible_destinations(BottomNavigationDemoType::WithLabels)[0].label,
            "Comments"
        );
        assert_eq!(
            visible_destinations(BottomNavigationDemoType::WithLabels)[2].label,
            "Account"
        );
        assert_eq!(
            visible_destinations(BottomNavigationDemoType::WithoutLabels)[4].label,
            "Camera"
        );
    }

    #[test]
    fn a_selection_out_of_range_is_clamped_back() {
        // Upstream's `_currentIndex.value.clamp(0, items.length - 1)` in the
        // `withLabels` build.
        assert_eq!(clamped_index(4, BottomNavigationDemoType::WithLabels), 2);
        assert_eq!(clamped_index(1, BottomNavigationDemoType::WithLabels), 1);
        assert_eq!(clamped_index(4, BottomNavigationDemoType::WithoutLabels), 4);
    }

    #[test]
    fn the_titles_are_upstreams() {
        assert_eq!(
            variant_title(BottomNavigationDemoType::WithLabels),
            "Persistent labels"
        );
        assert_eq!(
            variant_title(BottomNavigationDemoType::WithoutLabels),
            "Selected label"
        );
    }

    #[test]
    fn both_variants_start_on_the_first_destination() {
        let state = BottomNavigationDemoState::default();
        assert_eq!(state.with_labels_index, 0);
        assert_eq!(state.without_labels_index, 0);
    }
}
