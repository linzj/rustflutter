// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The component demos.
//!
//! Mirrors `lib/demos/material/` (flutter/gallery @ d12640d): one child module
//! per upstream file. What the upstream files share stays here: [`DemoState`]
//! (one struct rather than one per demo -- a demo is opened, used and left, and
//! the router resets it on every open, so a field that another demo also uses
//! cannot leak state across a visit), the `stage()`/`overlay()` dispatch the
//! demo page calls, and the layout helpers every demo builds with.
//!
//! The four reference demos (colors, typography, motion, 2d-transformations)
//! moved to `src/demos/reference/` with their per-file split; the demo page
//! routes their slugs there.
//!
//! Every demo is interactive. A demo that only drew a picture of a control
//! would be a screenshot with extra steps.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::Align;

use crate::app::GalleryState;
use crate::data::demos::Demo;

mod app_bar_demo;
mod banner_demo;
mod bottom_app_bar_demo;
mod bottom_navigation_demo;
mod bottom_sheet_demo;
mod button_demo;
mod cards_demo;
mod chip_demo;
mod data_table_demo;
mod dialog_demo;
mod divider_demo;
mod grid_list_demo;
mod list_demo;
mod material_demo_types;
mod material_demos;
mod menu_demo;
mod navigation_drawer;
mod navigation_rail_demo;
mod picker_demo;
mod progress_indicator_demo;
mod selection_controls_demo;
mod sliders_demo;
mod snackbar_demo;
mod tabs_demo;
mod text_field_demo;
mod tooltip_demo;

// Published by the app so the progress demo can read the spinner's controller
// without owning it; re-exported so `app.rs` keeps one import path.
pub use progress_indicator_demo::SpinnerValue;

/// Everything the demos need to remember between frames.
///
/// One struct rather than one per demo: a demo is opened, used and left, and
/// the router resets this on every open, so a field that another demo also uses
/// cannot leak state across a visit.
#[derive(Clone, Debug)]
pub struct DemoState {
    // Moved out: the per-demo files these fields belonged to keep their own
    // state now (batch M-D). The fields stay -- demos whose ports predate the
    // split still take the shared `DemoState` -- so they are annotated rather
    // than deleted, and a port that needs one back removes the annotation.
    #[allow(dead_code)]
    pub checkbox_a: bool,
    #[allow(dead_code)]
    pub checkbox_b: bool,
    #[allow(dead_code)]
    pub radio: usize,
    #[allow(dead_code)]
    pub switch: bool,
    #[allow(dead_code)]
    pub slider: f32,
    #[allow(dead_code)]
    pub tab: usize,
    #[allow(dead_code)]
    pub bottom_nav: usize,
    #[allow(dead_code)]
    pub chips: Vec<bool>,
    pub sheet_open: bool,
    pub snackbar_open: bool,
    #[allow(dead_code)]
    pub banner_open: bool,
    #[allow(dead_code)]
    pub rail: usize,
    #[allow(dead_code)]
    pub rail_extended: bool,
    pub counter: i32,
}

impl Default for DemoState {
    fn default() -> DemoState {
        DemoState {
            checkbox_a: true,
            checkbox_b: false,
            radio: 0,
            switch: true,
            slider: 0.35,
            tab: 0,
            bottom_nav: 0,
            chips: vec![true, false, false, false],
            sheet_open: false,
            snackbar_open: false,
            banner_open: true,
            rail: 1,
            rail_extended: false,
            counter: 0,
        }
    }
}

/// Builds the demo itself, dispatched by slug.
///
/// Upstream this is the body of `DemoPage` (`lib/pages/demo.dart`); the page
/// scaffold, the app bar and the info section all live in `pages/demo.rs`
/// now, so what remains here is the stage and its overlay.
pub fn stage(
    demo: &'static Demo,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    component(Stage {
        demo,
        state: state.demo.clone(),
        pressed: state.pressed,
        handle,
    })
}

/// The demo itself, dispatched by slug.
struct Stage {
    demo: &'static Demo,
    state: DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for Stage {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let state = &self.state;
        let handle = self.handle.clone();
        let pressed = self.pressed;

        let content: AnyWidget = match self.demo.slug {
            "app-bar" => app_bar_demo::app_bar(state, pressed, handle),
            "banner" => banner_demo::banner(state, handle),
            "bottom-app-bar" => bottom_app_bar_demo::stage(),
            "bottom-navigation" => bottom_navigation_demo::bottom_navigation(state, handle),
            "bottom-sheet" => bottom_sheet_demo::sheet_launcher(state, pressed, handle),
            "button" => button_demo::buttons(state, pressed, handle),
            "card" => cards_demo::cards(),
            "chip" => chip_demo::chips(state, handle),
            "data-table" => data_table_demo::data_table(),
            "dialog" => dialog_demo::dialog_launcher(state, pressed, handle),
            "divider" => divider_demo::dividers(),
            "grid-lists" => grid_list_demo::grid_lists(),
            "lists" => list_demo::lists(),
            "menu" => menu_demo::stage(),
            "nav_drawer" => navigation_drawer::stage(),
            "nav_rail" => navigation_rail_demo::navigation_rail(state, pressed, handle),
            "pickers" => picker_demo::stage(),
            "progress-indicator" => progress_indicator_demo::progress(state, context),
            "selection-controls" => selection_controls_demo::selection_controls(state, handle),
            "sliders" => sliders_demo::sliders(state, handle),
            "snackbars" => snackbar_demo::snackbar_launcher(state, pressed, handle),
            "tabs" => tabs_demo::tabs(state, handle),
            "text-field" => text_field_demo::stage(),
            "tooltip" => tooltip_demo::tooltips(state, handle),
            other => not_written_yet(other),
        };

        let surface = theme.surface;
        let outline = theme.outline;
        let radius = theme.radius;
        let spacing = theme.spacing;
        rustflutter::framework::single(content, move |inner| {
            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_corner_radius(radius)
                    .with_border(1.0, outline)
                    .with_padding(EdgeInsets::all(spacing * 2.0))
                    .with_child(inner),
            )
        })
    }
}

/// The modal a demo puts over its own page, if it has one open.
pub fn overlay(
    demo: &'static Demo,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> Option<AnyWidget> {
    match demo.slug {
        "bottom-sheet" if state.demo.sheet_open => Some(bottom_sheet_demo::sheet_overlay(handle)),
        _ => None,
    }
}

// -- Layout helpers -----------------------------------------------------------

fn column(children: Vec<AnyWidget>, spacing: f32) -> AnyWidget {
    many(children, move |rendered| {
        // Start rather than Stretch. Stretch forces a tight cross-axis
        // constraint, which overrides a child's own width -- a 48px switch
        // becomes as wide as the page. Under Start a child with no width of
        // its own still takes the full width, because it takes the biggest
        // size it is offered.
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(spacing);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(flex)
    })
}

fn row(children: Vec<AnyWidget>, spacing: f32) -> AnyWidget {
    many(children, move |rendered| {
        let mut flex = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(flex)
    })
}

fn caption(text: impl Into<String>) -> AnyWidget {
    let text = text.into();
    component(Caption { text })
}

struct Caption {
    text: String,
}

impl Component for Caption {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let text = self.text.clone();
        let style = TextStyle {
            font_size: theme.body_size - 2.0,
            color: theme.text_muted,
            font_weight: 700,
            ..TextStyle::default()
        };
        leaf(move || Text::new(text.clone()).with_style(style.clone()))
    }
}

/// A panel whose key changes with the selection.
///
/// The key is the point: without one the element is reused and the text simply
/// changes, and with one it is replaced -- which is what a cross-fade would
/// animate between if the framework had an implicit one.
///
/// Nothing constructs it today: the bottom-navigation and tabs demos keyed
/// their own panels when they moved to per-file states (batch M-D). Kept as
/// the documented idiom for the next demo that needs a keyed panel; delete it
/// rather than duplicate it.
#[allow(dead_code)]
struct FadedPanel {
    text: String,
    key_index: usize,
}

impl Component for FadedPanel {
    fn key(&self) -> rustflutter::framework::Key {
        Some(self.key_index as u64)
    }

    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let text = self.text.clone();
        let surface = theme.surface_variant;
        let radius = theme.radius;
        let body = theme.body();
        leaf(move || {
            Container::new()
                .with_height(96.0)
                .with_color(surface)
                .with_corner_radius(radius)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(text.clone()).with_style(body.clone()),
                ))
        })
    }
}

/// The stand-in every unwritten demo renders.
fn not_written_yet(slug: &str) -> AnyWidget {
    let slug = slug.to_string();
    leaf(move || Text::new(format!("The demo for {slug} is not written yet.")).with_size(13.0))
}
