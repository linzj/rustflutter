// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/app.dart` (flutter/gallery @ d12640d):
//! `ReplyApp`, the study's root.
//!
//! Upstream this is a `MultiProvider` holding an `EmailStore` over a
//! `MaterialApp` with two routes (`/reply` → `AdaptiveNav`, `/reply/compose` →
//! `ComposePage`) and both Reply themes. Here the gallery's own app owns the
//! routing (`src/app.rs`, `src/studies/mod.rs`), so what remains of `ReplyApp`
//! is the two things that are genuinely the study's: **holding the store** and
//! **applying the theme**.
//!
//! The store is upstream's `ChangeNotifierProvider<EmailStore>`. There is no
//! provider tree carrying mutable models here, so it is the state of a
//! `StatefulComponent` -- `notifyListeners()` is a `set_state`, and the
//! framework rebuilds after every one. [`ReplyState`] is that state, and the
//! handle is threaded down to whatever needs to mutate it, which is the way
//! the demos already pass `GalleryState` around.
//!
//! Reply follows the application's brightness, unlike Crane and Fortnightly:
//! upstream hands its `MaterialApp` both themes and lets `themeMode` pick. See
//! `theme.rs`.
//!
//! # What is not here yet
//!
//! The mobile inbox is what this batch renders: the mailbox list, the notched
//! bottom app bar and the compose button. The four screens behind it --
//! `mail_view_page`, `compose_page`, `search_page` and the `bottom_drawer` the
//! bar's arrow opens -- are still the skeletons they were, and their entry
//! points here do nothing. The desktop `_DesktopNav` (a navigation rail and a
//! two-pane list/detail) is not ported either; `adaptive_nav.rs` renders the
//! mobile branch at every width and says so.

use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, provide};
use rustflutter::platform::Brightness;

use crate::app::GalleryState;

use super::model::email_store::EmailStore;
use super::theme;

/// Upstream's `title: 'Reply'`.
#[allow(dead_code)] // The gallery's route table names the study; the title is
// what upstream's MaterialApp carried.
pub const TITLE: &str = "Reply";

/// Upstream's `EmailStore`, held where a `StatefulComponent`'s state is held.
///
/// A type alias rather than a wrapper: everything upstream's provider carried
/// is already on the store, and a second struct around it would only be a
/// second name for the same thing.
pub type ReplyState = EmailStore;

/// The Reply theme for the ambient brightness -- upstream's
/// `Theme.of(context)` inside the study, which is one of the two themes
/// `ReplyApp` handed its `MaterialApp`.
///
/// Reads the theme the study provided rather than answering with constants,
/// which is what the skeleton did while there was no provider to read.
pub fn reply_theme_of(context: &mut BuildContext) -> std::rc::Rc<rustflutter::components::Theme> {
    rustflutter::components::theme_of(context)
}

/// Builds `body` under the Reply theme -- upstream's
/// `MaterialApp(theme:, darkTheme:, themeMode:)`.
pub(crate) fn themed(brightness: Brightness, body: AnyWidget) -> AnyWidget {
    provide(theme::reply_theme(brightness), body)
}

/// The study's screen: upstream's home route, `AdaptiveNav`, under the theme.
pub(crate) fn screen(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let brightness = state.options.resolved_brightness();
    themed(
        brightness,
        super::adaptive_nav::screen(state, handle, brightness),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_is_upstreams() {
        assert_eq!(TITLE, "Reply");
    }

    #[test]
    fn the_store_starts_where_upstream_starts() {
        // Upstream's `EmailStore` seeds ids 7 and 8 into the trash and starts
        // on the inbox with nothing starred and no mail open.
        let state = ReplyState::default();
        assert_eq!(state.selected_email_id, -1, "no mail open");
        assert!(state.starred_email_ids.is_empty());
        assert!(state.trash_email_ids.contains(&7));
        assert!(state.trash_email_ids.contains(&8));
        // So the inbox is the six that are neither trashed nor spam.
        assert_eq!(state.inbox_emails().len(), 6);
    }
}
