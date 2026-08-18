// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/routes.dart` (flutter/gallery @ d12640d):
//! the one route constant.
//!
//! Upstream this is the route CraneApp's `MaterialApp` opens on. Here the
//! gallery's own navigator owns routing (`src/app.rs`) and Crane is reached
//! through the `study` route with slug `crane`; the constant is kept as the
//! record of what upstream's route table held.

/// Upstream's `defaultRoute`.
#[allow(dead_code)] // Documentation of the upstream route table; the gallery's
                    // router, not this string, decides where Crane lives.
pub const DEFAULT_ROUTE: &str = "/crane";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_route_is_upstream_s() {
        assert_eq!(DEFAULT_ROUTE, "/crane");
    }
}
