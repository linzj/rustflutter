// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The studies: whole screens rather than single components.
//!
//! Upstream these are `lib/studies/{rally,shrine,crane,reply,fortnightly,
//! starter}` (flutter/gallery @ d12640d), each a small app with its own
//! theme, navigation and assets. What is ported here is one screen each --
//! the one that shows what the component library looks like when it is
//! composed rather than catalogued. Their own themes are not: the gallery's
//! theme is what a study demonstrates working under.
//!
//! The tree mirrors upstream one module per file. This module keeps the
//! shared [`StudyState`] and the [`page`] slug dispatch; the aggregate Rally,
//! Shrine and Crane screens are re-homed in `rally/home.rs`,
//! `shrine/home.rs` and `crane/backdrop.rs` ("current aggregate
//! implementation; per-file alignment in flight"), and every other module is
//! a placeholder or stub the per-study batches fill in.

pub mod crane;
pub mod fortnightly;
pub mod rally;
pub mod reply;
pub mod shrine;
pub mod starter;

use rustflutter::framework::{leaf, AnyWidget, StateHandle};
use rustflutter::prelude::*;

use crate::app::{self, GalleryState};
use crate::data::demos as catalog;

/// What the studies remember.
#[derive(Clone, Debug, Default)]
pub struct StudyState {
    /// Shrine's category filter.
    pub filter: usize,
    /// Shrine's cart.
    pub cart: u32,
    /// Crane's tab.
    pub tab: usize,
}

pub fn page(
    study: &'static catalog::Study,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let body: AnyWidget = match study.slug {
        "rally" => rally::home::screen(state, handle.clone()),
        "shrine" => shrine::home::screen(state, handle.clone()),
        "crane" => crane::backdrop::screen(state, handle.clone()),
        "reply" => reply::app::screen(),
        "fortnightly" => fortnightly::app::screen(),
        "starterApp" => starter::home::screen(),
        other => not_written_yet(other),
    };

    app::scaffold(study.title, Some(study.subtitle), state, handle, body)
}

/// The stand-in every unwritten study (or study file) renders -- the same
/// placeholder the demos' skeletons use, named for the study.
pub(crate) fn not_written_yet(study: &str) -> AnyWidget {
    let study = study.to_string();
    leaf(move || Text::new(format!("The {study} study is not written yet.")))
}
