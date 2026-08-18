// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/routes.dart` (flutter/gallery @ d12640d):
//! the two route names the Reply app's navigators know.
//!
//! Upstream these drive `MaterialApp.onGenerateRoute` and the nested mail
//! `Navigator`s. Here the study runs inside the gallery's single-screen
//! scaffold, so the routes are not pushed anywhere -- the study's page
//! switching is state on `EmailStore` instead (`selectedEmailId`,
//! `onSearchPage`, and the compose flag). The constants are kept because the
//! navigation structure they name is ported, and so that a reader mapping the
//! files across finds the same strings.

/// Upstream's `homeRoute`.
pub const HOME_ROUTE: &str = "/reply";

/// Upstream's `composeRoute`.
pub const COMPOSE_ROUTE: &str = "/reply/compose";

/// Upstream `_MailNavigatorState.inboxRoute` in `adaptive_nav.dart`, the one
/// route the nested mail navigators are constructed with.
pub const INBOX_ROUTE: &str = "/reply/inbox";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routes_are_upstreams_strings() {
        assert_eq!(HOME_ROUTE, "/reply");
        assert_eq!(COMPOSE_ROUTE, "/reply/compose");
        assert_eq!(INBOX_ROUTE, "/reply/inbox");
    }
}
