// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The Cupertino demos.
//!
//! Mirrors `lib/demos/cupertino/` (flutter/gallery @ d12640d): one child
//! module per upstream file. This is scaffolding: every demo still renders
//! the shared not-written-yet placeholder, and the per-file ports are the
//! next batch. What is settled here is the shape the ports land in -- the
//! module list, and the `stage()`/`overlay()` dispatch that `pages/demo.rs`
//! routes the thirteen `cupertino-*` slugs to.

use rustflutter::prelude::*;

use crate::app::GalleryState;
use crate::data::demos::Demo;

mod cupertino_activity_indicator_demo;
mod cupertino_alert_demo;
mod cupertino_button_demo;
mod cupertino_context_menu_demo;
mod cupertino_demos;
mod cupertino_navigation_bar_demo;
mod cupertino_picker_demo;
mod cupertino_scrollbar_demo;
mod cupertino_search_text_field_demo;
mod cupertino_segmented_control_demo;
mod cupertino_slider_demo;
mod cupertino_switch_demo;
mod cupertino_tab_bar_demo;
mod cupertino_text_field_demo;
mod demo_types;

/// Builds the demo itself, dispatched by slug.
///
/// The signature mirrors `demos::material::stage` so the demo page routes by
/// slug prefix without touching the call sites; `state` and `handle` are
/// what the ported demos will drive.
pub fn stage(
    demo: &'static Demo,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let _ = (state, handle);
    match demo.slug {
        "cupertino-activity-indicator" => cupertino_activity_indicator_demo::stage(),
        "cupertino-alerts" => cupertino_alert_demo::stage(),
        "cupertino-buttons" => cupertino_button_demo::stage(),
        "cupertino-context-menu" => cupertino_context_menu_demo::stage(),
        "cupertino-navigation-bar" => cupertino_navigation_bar_demo::stage(),
        "cupertino-picker" => cupertino_picker_demo::stage(),
        "cupertino-scrollbar" => cupertino_scrollbar_demo::stage(),
        "cupertino-search-text-field" => cupertino_search_text_field_demo::stage(),
        "cupertino-segmented-control" => cupertino_segmented_control_demo::stage(),
        "cupertino-slider" => cupertino_slider_demo::stage(state),
        "cupertino-switch" => cupertino_switch_demo::stage(state),
        "cupertino-tab-bar" => cupertino_tab_bar_demo::stage(state),
        "cupertino-text-field" => cupertino_text_field_demo::stage(state),
        other => not_written_yet(other),
    }
}

/// The modal a demo puts over its own page, if it has one open.
///
/// No Cupertino demo is ported yet, so none has an overlay; the signature
/// mirrors `demos::material::overlay` for the same reason `stage`'s does.
pub fn overlay(
    _demo: &'static Demo,
    _state: &GalleryState,
    _handle: StateHandle<GalleryState>,
) -> Option<AnyWidget> {
    None
}

/// The stand-in every unwritten demo renders -- the same one
/// `demos::material` uses (its copy is private to that module).
fn not_written_yet(slug: &str) -> AnyWidget {
    let slug = slug.to_string();
    leaf(move || Text::new(format!("The demo for {slug} is not written yet.")).with_size(13.0))
}
