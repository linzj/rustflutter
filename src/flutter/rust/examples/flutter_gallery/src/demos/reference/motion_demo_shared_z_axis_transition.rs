// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/reference/motion_demo_shared_z_axis_transition.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `SharedZAxisTransitionDemo` is a two-route `Navigator`: the
//! recipe page ("Saved Recipes", six `_RecipeTile`s) and the settings page
//! the app bar's gear pushes, both routes built with a scaled
//! `SharedAxisTransition` (`fillColor: Colors.transparent`,
//! `PageRouteBuilder`'s 300ms). The transition is reproduced here by
//! [`transitions::shared_axis_enter`] and [`transitions::shared_axis_exit`]
//! with [`SharedAxis::Scaled`].
//!
//! Divergences, each also marked at its site:
//!
//! * The demo is one of six sections stacked on the single `motion` stage
//!   (see `mod.rs`'s header), so its routes are states of the section rather
//!   than a `Navigator`'s, and its pages are height-bounded
//!   ([`BODY_HEIGHT`]). The settings page's back affordance is an explicit
//!   arrow where upstream's pushed route gets the implied leading.
//! * The settings tiles' icons are the bundled Material Icons font's
//!   codepoints for `Icons.person`/`notifications`/`security`/`help`, which
//!   predate the font's current mapping ([`settings_icons`]); the shared
//!   table in `data/demos.rs` is generated and does not carry them.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{
    Align, ClipRRect, Empty, ImageView, Opacity, Pointer, Stack, Transform,
};

use crate::app::ids;
use crate::data::demos::{icon, MATERIAL_ICONS};
use crate::l10n::gallery_localizations::GalleryLocalizations;

use super::{
    screen_column,
    transitions::{self, SharedAxis},
};

/// The hit-test ids this section's controls take from.
const ID_BASE: u64 = ids::DEMO_LOCAL + 1300;

/// `PageRouteBuilder`'s default `transitionDuration`.
const TRANSITION_MICROS: i64 = 300_000;

/// The height the pages stand in at; see the module header.
const BODY_HEIGHT: f32 = 430.0;

/// The recipe photographs, upstream's `crane/destinations/eat_*.jpg` assets
/// (`flutter_gallery_assets`), copied into `assets/crane/` (assets/README.md
/// has the provenance convention).
const RECIPE_IMAGES: [(&str, &[u8]); 6] = [
    (
        "crane/destinations/eat_2.jpg",
        include_bytes!("../../../assets/crane/eat_2.jpg"),
    ),
    (
        "crane/destinations/eat_3.jpg",
        include_bytes!("../../../assets/crane/eat_3.jpg"),
    ),
    (
        "crane/destinations/eat_4.jpg",
        include_bytes!("../../../assets/crane/eat_4.jpg"),
    ),
    (
        "crane/destinations/eat_6.jpg",
        include_bytes!("../../../assets/crane/eat_6.jpg"),
    ),
    (
        "crane/destinations/eat_8.jpg",
        include_bytes!("../../../assets/crane/eat_8.jpg"),
    ),
    (
        "crane/destinations/eat_10.jpg",
        include_bytes!("../../../assets/crane/eat_10.jpg"),
    ),
];

/// The bundled font's codepoints for the settings tiles' icons (see the
/// module header).
mod settings_icons {
    pub const PERSON: &str = "\u{e491}"; // Icons.person
    pub const NOTIFICATIONS: &str = "\u{e44f}"; // Icons.notifications
    pub const SECURITY: &str = "\u{e569}"; // Icons.security
    pub const HELP: &str = "\u{e309}"; // Icons.help
}

/// The demo's section: upstream's `SharedZAxisTransitionDemo`.
pub(super) fn section() -> AnyWidget {
    stateful(SharedZAxisTransitionDemo)
}

struct SharedZAxisTransitionDemo;

/// The two routes' state: which is on top, and the transition's clock.
/// Upstream keeps this in the `Navigator`; the section keeps it here.
struct SharedZAxisDemoState {
    /// Whether the settings route is pushed.
    settings_open: bool,
    progress: f32,
    running: bool,
    last_frame_micros: Option<i64>,
    pressed: Option<u64>,
}

impl Default for SharedZAxisDemoState {
    fn default() -> Self {
        SharedZAxisDemoState {
            settings_open: false,
            progress: 0.0,
            running: false,
            last_frame_micros: None,
            pressed: None,
        }
    }
}

/// The gear's push: the settings route goes on, its transition forward.
fn push_settings(state: &mut SharedZAxisDemoState) {
    state.settings_open = true;
    state.progress = 0.0;
    state.running = true;
}

/// The back arrow's pop: the home route returns, the transition in reverse.
fn pop_settings(state: &mut SharedZAxisDemoState) {
    state.settings_open = false;
    state.progress = 0.0;
    state.running = true;
}

impl StatefulComponent for SharedZAxisTransitionDemo {
    type State = SharedZAxisDemoState;

    fn advance(&self, state: &mut SharedZAxisDemoState, frame_time_micros: i64) -> bool {
        let elapsed = match state.last_frame_micros.replace(frame_time_micros) {
            Some(previous) => (frame_time_micros - previous).clamp(0, crate::app::MAX_FRAME_MICROS),
            None => 0,
        };
        if !state.running {
            return false;
        }
        state.progress = (state.progress + elapsed as f32 / TRANSITION_MICROS as f32).min(1.0);
        if state.progress >= 1.0 {
            state.running = false;
        }
        true
    }

    fn build(
        &self,
        state: &SharedZAxisDemoState,
        handle: StateHandle<SharedZAxisDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let canvas = theme.background;

        // The two routes. During the transition both render, the arriving
        // one on top -- a route push's arrangement upstream.
        let reverse = !state.settings_open;
        let arriving = if state.settings_open {
            settings_page(&handle)
        } else {
            home_page(state, &handle)
        };
        let body = if state.running {
            let enter = transitions::shared_axis_enter(state.progress, SharedAxis::Scaled, reverse);
            let exit = transitions::shared_axis_exit(state.progress, SharedAxis::Scaled, reverse);
            let leaving = if state.settings_open {
                home_page(state, &handle)
            } else {
                settings_page(&handle)
            };
            many(vec![leaving, arriving], move |mut rendered| {
                let arriving = rendered.pop().unwrap_or_else(|| boxed(Empty));
                let leaving = rendered.pop().unwrap_or_else(|| boxed(Empty));
                Box::new(
                    Stack::new()
                        .push(Opacity::new(
                            exit.opacity,
                            Transform::scale(exit.scale, leaving),
                        ))
                        .push(Opacity::new(
                            enter.opacity,
                            Transform::scale(enter.scale, arriving),
                        )),
                )
            })
        } else {
            arriving
        };
        single(body, move |inner| {
            Box::new(
                Container::new()
                    .with_height(BODY_HEIGHT)
                    .with_color(canvas)
                    .with_child(inner),
            )
        })
    }
}

/// An icon button's target: the glyph centered in a padded, tappable box.
fn icon_button(
    id: u64,
    glyph: &'static str,
    color: Color,
    handle: &StateHandle<SharedZAxisDemoState>,
    action: fn(&mut SharedZAxisDemoState),
) -> AnyWidget {
    let handle = handle.clone();
    leaf(move || {
        let tap_handle = handle.clone();
        Pointer::new(
            id,
            Container::new()
                .with_padding(EdgeInsets::all(12.0))
                .with_child(
                    Text::new(glyph)
                        .with_font_family(MATERIAL_ICONS)
                        .with_size(24.0)
                        .with_color(color),
                ),
        )
        .with_handlers(
            rustflutter::gestures::PointerHandlers::new().with_tap(move |_| {
                tap_handle.set_state(action);
            }),
        )
    })
}

/// The home route: upstream's `_createHomeRoute` page -- the app bar with
/// the gear over `_RecipePage`.
fn home_page(
    _state: &SharedZAxisDemoState,
    handle: &StateHandle<SharedZAxisDemoState>,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let gear_color = Color(0x8A00_0000);
    let gear = icon_button(ID_BASE, icon::SETTINGS, gear_color, handle, push_settings);
    let app_bar = component(
        AppBar::new(l10n.demo_shared_z_axis_title())
            .with_subtitle(format!("({})", l10n.demo_shared_z_axis_demo_instructions()))
            .with_trailing(gear),
    );
    screen_column(vec![app_bar, recipe_page()])
}

/// `_RecipePage`: the "Saved Recipes" header over the six recipe tiles.
fn recipe_page() -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let recipes: [(&str, &str); 6] = [
        (
            l10n.demo_shared_z_axis_burger_recipe_title(),
            l10n.demo_shared_z_axis_burger_recipe_description(),
        ),
        (
            l10n.demo_shared_z_axis_sandwich_recipe_title(),
            l10n.demo_shared_z_axis_sandwich_recipe_description(),
        ),
        (
            l10n.demo_shared_z_axis_dessert_recipe_title(),
            l10n.demo_shared_z_axis_dessert_recipe_description(),
        ),
        (
            l10n.demo_shared_z_axis_shrimp_plate_recipe_title(),
            l10n.demo_shared_z_axis_shrimp_plate_recipe_description(),
        ),
        (
            l10n.demo_shared_z_axis_crab_plate_recipe_title(),
            l10n.demo_shared_z_axis_crab_plate_recipe_description(),
        ),
        (
            l10n.demo_shared_z_axis_beef_sandwich_recipe_title(),
            l10n.demo_shared_z_axis_beef_sandwich_recipe_description(),
        ),
    ];
    let mut tiles: Vec<AnyWidget> = Vec::new();
    for (index, ((title, description), (cache_key, bytes))) in
        recipes.iter().zip(RECIPE_IMAGES.iter()).enumerate()
    {
        tiles.push(recipe_tile(title, description, cache_key, bytes, index));
    }
    many(tiles, move |rendered| {
        let mut column = Column::new()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(8.0)
            .push(
                Container::new()
                    .with_padding(EdgeInsets::only(8.0, 8.0, 0.0, 4.0))
                    .with_child(
                        Text::new(l10n.demo_shared_z_axis_saved_recipes_list_title())
                            .with_size(14.0),
                    ),
            );
        for tile in rendered {
            column = column.push(tile);
        }
        Box::new(
            Container::new()
                .with_padding(EdgeInsets::all(8.0))
                .with_child(column),
        )
    })
}

/// `_RecipeTile`: the 100x70 rounded photograph, the title and description
/// with the "0N" trailing, over the divider.
fn recipe_tile(
    title: &'static str,
    description: &'static str,
    cache_key: &'static str,
    bytes: &'static [u8],
    index: usize,
) -> AnyWidget {
    leaf(move || {
        let mut photo = Container::new().with_size(100.0, 70.0);
        if let Some(image) = Image::shared(cache_key, bytes) {
            // Upstream's `ClipRRect(radius: 4, Image(fit: BoxFit.fill))`.
            photo = photo.with_child(ClipRRect::new(
                4.0,
                ImageView::with_fit(image, rustflutter::render::BoxFit::Fill),
            ));
        }
        RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(24.0)
            .push(photo)
            .push_flex(FlexChild::expanded(
                Column::new()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .push_flex(FlexChild::expanded(
                                Column::new()
                                    .with_main_axis_size(MainAxisSize::Min)
                                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                                    .with_spacing(2.0)
                                    .push(Text::new(title).with_size(15.0))
                                    .push(Text::new(description).with_size(12.0)),
                                1,
                            ))
                            .push(Text::new(format!("0{}", index + 1)).with_size(12.0)),
                    )
                    // Upstream's `Divider(thickness: 2)`.
                    .push(
                        Container::new().with_height(8.0).with_child(Align::new(
                            Alignment::BOTTOM_CENTER,
                            rustflutter::widgets::FullWidth::new(
                                Container::new()
                                    .with_height(1.0)
                                    .with_color(Color(0x1F00_0000)),
                            ),
                        )),
                    ),
                1,
            ))
    })
}

/// The settings route: upstream's `_SettingsPage` -- the titled app bar
/// (with the back arrow a pushed route implies) over the four settings
/// tiles.
fn settings_page(handle: &StateHandle<SharedZAxisDemoState>) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let back = icon_button(
        ID_BASE + 1,
        icon::ARROW_BACK,
        Color(0x8A00_0000),
        handle,
        pop_settings,
    );
    let app_bar =
        component(AppBar::new(l10n.demo_shared_z_axis_settings_page_title()).with_trailing(back));

    let settings: [(&str, &str); 4] = [
        (
            settings_icons::PERSON,
            l10n.demo_shared_z_axis_profile_setting_label(),
        ),
        (
            settings_icons::NOTIFICATIONS,
            l10n.demo_shared_z_axis_notification_setting_label(),
        ),
        (
            settings_icons::SECURITY,
            l10n.demo_shared_z_axis_privacy_setting_label(),
        ),
        (
            settings_icons::HELP,
            l10n.demo_shared_z_axis_help_setting_label(),
        ),
    ];
    let mut tiles: Vec<AnyWidget> = Vec::new();
    for (glyph, label) in settings {
        tiles.push(leaf(move || {
            Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(16.0)
                        .push(
                            Container::new()
                                .with_padding(EdgeInsets::only(16.0, 12.0, 0.0, 12.0))
                                .with_child(
                                    Text::new(glyph)
                                        .with_font_family(MATERIAL_ICONS)
                                        .with_size(24.0)
                                        .with_color(Color(0x8A00_0000)),
                                ),
                        )
                        .push(Text::new(label).with_size(15.0)),
                )
                // Upstream's `Divider(thickness: 2)`.
                .push(
                    Container::new().with_height(8.0).with_child(Align::new(
                        Alignment::BOTTOM_CENTER,
                        rustflutter::widgets::FullWidth::new(
                            Container::new()
                                .with_height(1.0)
                                .with_color(Color(0x1F00_0000)),
                        ),
                    )),
                )
        }));
    }
    let body = many(tiles, move |rendered| {
        let mut column = Column::new()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for tile in rendered {
            column = column.push(tile);
        }
        Box::new(column)
    });
    screen_column(vec![app_bar, body])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gear_pushes_and_the_arrow_pops() {
        let mut state = SharedZAxisDemoState::default();
        push_settings(&mut state);
        assert!(state.settings_open);
        assert!(state.running);
        assert_eq!(state.progress, 0.0);
        pop_settings(&mut state);
        assert!(!state.settings_open);
        assert!(state.running, "the pop animates too");
    }

    #[test]
    fn the_recipe_images_are_the_upstream_six_in_order() {
        // Upstream's `_RecipePage.savedRecipes` assets.
        let names: Vec<&str> = RECIPE_IMAGES.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            names,
            [
                "crane/destinations/eat_2.jpg",
                "crane/destinations/eat_3.jpg",
                "crane/destinations/eat_4.jpg",
                "crane/destinations/eat_6.jpg",
                "crane/destinations/eat_8.jpg",
                "crane/destinations/eat_10.jpg",
            ]
        );
    }
}
