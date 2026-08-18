// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/fortnightly/routes.dart` (flutter/gallery @ d12640d), upstream's
//! the `defaultRoute` constant.
//!
//! The gallery's own router names the study by slug (`app::routes::STUDY` +
//! `fortnightly`), so this constant documents the upstream route name rather
//! than driving any navigation here.

/// Upstream's `routes.defaultRoute`, the route `FortnightlyApp` registers its
/// home under.
pub const DEFAULT_ROUTE: &str = "/fortnightly";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_route_is_upstream() {
        assert_eq!(DEFAULT_ROUTE, "/fortnightly");
    }
}
