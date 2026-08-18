// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Skeleton for `lib/studies/reply/app.dart` (flutter/gallery @ d12640d), upstream's
//! the `ReplyApp` root widget.
//!
//! Not ported yet: renders the shared not-written-yet placeholder, the way the
//! demos/cupertino skeleton did. The per-file port is the study batch's. This is the module `studies::page` routes the
//! `reply` slug to.

use rustflutter::engine::Color;
use rustflutter::framework::{AnyWidget, BuildContext};

use super::colors::reply_colors;

/// The study's own theme -- upstream's `ThemeData` the `ReplyApp` root
/// builds. Only the piece the interrupted port already reads is here; the
/// resumed batch replaces this with the real theme and its provider.
pub struct ReplyTheme {
    /// Upstream's `Theme.of(context).cardColor`: the dark theme's card color.
    pub card: Color,
}

/// The study theme for a subtree. The skeleton has no provider to read, so
/// the constants answer directly; `profile_avatar.rs` is the one caller.
pub fn reply_theme_of(_context: &mut BuildContext) -> ReplyTheme {
    ReplyTheme {
        card: reply_colors::DARK_CARD_BACKGROUND,
    }
}

/// The stand-in body; see the module header.
pub(crate) fn screen() -> AnyWidget {
    crate::studies::not_written_yet("reply")
}
