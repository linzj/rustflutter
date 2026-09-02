// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/app.dart` (flutter/gallery @ d12640d):
//! `CraneApp`, the study's root.
//!
//! Upstream this is a `MaterialApp` with one route (the `Backdrop`) and the
//! Crane theme over it. Here the gallery's own app owns the `MaterialApp`
//! equivalent -- routing included (`src/app.rs`, `src/studies/mod.rs`), so
//! what remains of CraneApp is its title, its default route
//! ([`super::routes`]), and the one thing that is genuinely the study's own:
//! applying the Crane theme. [`themed`] is that application; the screen in
//! `backdrop.rs` is built under it. `ApplyTextOptions` is the gallery's own
//! MediaQuery override, applied at the gallery root, not here.

use rustflutter::framework::{AnyWidget, provide};

use super::theme;

/// Upstream's `title: 'Crane'`.
#[allow(dead_code)] // The gallery's route table names the study; the title is
// what upstream's MaterialApp carried.
pub const TITLE: &str = "Crane";

/// Builds `body` under the Crane theme -- upstream's
/// `MaterialApp(theme: craneTheme)`. Crane's theme is light-based whatever
/// the app's brightness; see `theme.rs`.
pub(crate) fn themed(body: AnyWidget) -> AnyWidget {
    provide(theme::crane_theme(), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_is_upstream_s() {
        assert_eq!(TITLE, "Crane");
    }
}
