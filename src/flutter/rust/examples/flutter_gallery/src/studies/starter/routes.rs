// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/starter/routes.dart` (flutter/gallery @ d12640d):
//! the `defaultRoute` constant.
//!
//! The constant survives, but the routing it fed does not: upstream's
//! `StarterApp` hands it to `MaterialApp.initialRoute`, and here the studies
//! are screens on the gallery's own navigator (`src/studies/mod.rs`), reached
//! by the `starter` slug rather than by a route of the study's own.

/// Upstream's `defaultRoute`.
pub const DEFAULT_ROUTE: &str = "/starter";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_route_is_upstream() {
        assert_eq!(DEFAULT_ROUTE, "/starter");
    }
}
