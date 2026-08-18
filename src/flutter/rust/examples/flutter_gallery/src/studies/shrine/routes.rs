// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/routes.dart` (flutter/gallery @ d12640d):
//! the route constants.
//!
//! Upstream these are the two routes of the study's own `MaterialApp`. Here
//! the study is one screen inside the gallery's navigator, so the routes are
//! states of the study's root component (`app.rs`) rather than gallery
//! routes; the constants keep upstream's names and values.

/// Upstream's `loginRoute`.
pub const LOGIN_ROUTE: &str = "/shrine/login";
/// Upstream's `homeRoute`.
pub const HOME_ROUTE: &str = "/shrine";

/// Which of the two routes the study is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShrineRoute {
    /// Upstream's `initialRoute`: the login page.
    #[default]
    Login,
    /// The backdrop with the product page, the menu and the cart.
    Home,
}

impl ShrineRoute {
    /// The route's upstream name, for diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            ShrineRoute::Login => LOGIN_ROUTE,
            ShrineRoute::Home => HOME_ROUTE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_route_names_are_upstreams() {
        assert_eq!(LOGIN_ROUTE, "/shrine/login");
        assert_eq!(HOME_ROUTE, "/shrine");
    }

    #[test]
    fn the_study_opens_on_login_like_upstreams_initial_route() {
        assert_eq!(ShrineRoute::default(), ShrineRoute::Login);
        assert_eq!(ShrineRoute::Login.name(), LOGIN_ROUTE);
    }
}
