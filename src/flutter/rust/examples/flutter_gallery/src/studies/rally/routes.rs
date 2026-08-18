// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/rally/routes.dart` (flutter/gallery @ d12640d),
//! upstream's route name constants.

/// Upstream's `loginRoute`.
pub const LOGIN_ROUTE: &str = "/rally/login";

/// Upstream's `homeRoute`.
pub const HOME_ROUTE: &str = "/rally";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routes_are_upstream() {
        assert_eq!(LOGIN_ROUTE, "/rally/login");
        assert_eq!(HOME_ROUTE, "/rally");
    }
}
