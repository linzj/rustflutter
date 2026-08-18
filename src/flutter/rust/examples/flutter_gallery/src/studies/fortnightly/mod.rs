// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Mirrors upstream `lib/studies/fortnightly/` (flutter/gallery @ d12640d): one
//! child module per upstream file.
//!
//! Ported. `app.rs` is the `FortnightlyApp` root and the two homes,
//! `shared.rs` the article data, previews, navigation and theme, `routes.rs`
//! the `defaultRoute` constant. The study provides its own (light-only) theme
//! at its root, per-file headers carry the divergences.

pub mod app;
pub mod routes;
pub mod shared;
