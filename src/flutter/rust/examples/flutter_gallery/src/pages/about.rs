// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The about dialog.
//!
//! Ported from `lib/pages/about.dart` (flutter/gallery @ d12640d): the name
//! and version, the description with the repository link picked out in
//! primary, the legalese, and a close action.
//!
//! Two deltas, both logged in PORTING.md: the repo link is coloured but not
//! tappable, because upstream opens it through `url_launcher` and there is no
//! counterpart here; and the "View licenses" action is omitted, because
//! upstream's pushes the framework's `LicensePage`, which the framework does
//! not have.

use rustflutter::controls::{Dialog, Scrim};
use rustflutter::framework::{component, many, AnyWidget, StateHandle};
use rustflutter::prelude::*;
use rustflutter::widgets::{boxed, Center, Empty, Stack};

use crate::app::{ids, GalleryState};
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// Upstream's `getVersionNumber`. There it is async because it reads the
/// platform's package info; the value is a constant either way.
pub const VERSION: &str = "2.10.2+021002";
/// Upstream's `_AboutDialog`, as the overlay `pages/backdrop.rs` puts over the
/// whole app when `GalleryState::about_open` is set. `None` when it is not.
pub fn overlay(state: &GalleryState, handle: StateHandle<GalleryState>) -> Option<AnyWidget> {
    if !state.about_open {
        return None;
    }

    let l10n = GalleryLocalizations::lookup(&state.options.locale());
    let name = "Flutter Gallery"; // Don't need to localize.
    let legalese = "© 2021 The Flutter team"; // Don't need to localize.
    let description = l10n.about_dialog_description(l10n.github_repo(name));

    let close_handle = handle.clone();
    let body = format!("{description}\n\n{legalese}");

    Some(many(
        vec![
            component(Scrim::new(ids::SCRIM).wired(handle, |s| s.about_open = false)),
            component(
                Dialog::new(format!("{name} {VERSION}"))
                    .with_body(body)
                    .with_width(400.0)
                    .with_action(component(
                        Button::new(ids::SETTINGS_LOCAL + 700, "Close")
                            .with_style(ButtonStyle::Text)
                            .wired(close_handle, |s| &mut s.pressed, |s| s.about_open = false),
                    )),
            ),
        ],
        |mut rendered| {
            let dialog = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let scrim = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(
                Stack::new()
                    .push_positioned(scrim, rustflutter::widgets::Positioned::fill())
                    .push(Center::new(dialog)),
            )
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_upstreams() {
        // `getVersionNumber` upstream returns exactly this.
        assert_eq!(VERSION, "2.10.2+021002");
    }
}
