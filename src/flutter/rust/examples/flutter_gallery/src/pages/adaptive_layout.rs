// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The display-size breakpoints every page adapts by.
//!
//! Ported from `lib/layout/adaptive.dart` (flutter/gallery @ d12640d). It
//! lives under `pages/` rather than `layout/` because the batch that ported it
//! (M-C) is the pages batch; the path delta is logged in PORTING.md.
//!
//! Upstream's `getWindowType` comes from the `adaptive_breakpoints` package;
//! the ranges below are that package's `breakpointSystem` table collapsed to
//! the edges the gallery's two questions (`isDisplayDesktop`,
//! `isDisplaySmallDesktop`) can tell apart.
//!
//! `isDisplayFoldable` reads upstream's `MediaQuery.hinge`, which the
//! framework's `MediaQueryData` does not carry -- there is one embedder and it
//! reports no hinge -- so it is always false here, with the reason kept at the
//! function rather than left to look like an omission.

use rustflutter::framework::BuildContext;
use rustflutter::media_query::size_of;

/// The maximum width taken up by each item on the home screen.
pub const MAX_HOME_ITEM_WIDTH: f32 = 1400.0;

/// Upstream's `AdaptiveWindowType`, from the `adaptive_breakpoints` package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdaptiveWindowType {
    /// Under 600 logical pixels across.
    XSmall,
    /// 600 to 1023.
    Small,
    /// 1024 to 1439.
    Medium,
    /// 1440 to 1919.
    Large,
    /// 1920 and over.
    XLarge,
}

/// The package's `breakpointSystem` ranges, as their lower edges. Each window
/// type runs from its edge to the next one's.
const EDGES: &[(f32, AdaptiveWindowType)] = &[
    (0.0, AdaptiveWindowType::XSmall),
    (600.0, AdaptiveWindowType::Small),
    (1024.0, AdaptiveWindowType::Medium),
    (1440.0, AdaptiveWindowType::Large),
    (1920.0, AdaptiveWindowType::XLarge),
];

/// Upstream's `getWindowType`.
pub fn window_type_of(context: &BuildContext) -> AdaptiveWindowType {
    let width = size_of(context).width;
    let mut kind = AdaptiveWindowType::XSmall;
    for (edge, window_type) in EDGES {
        if width >= *edge {
            kind = *window_type;
        }
    }
    kind
}

/// Whether the display has a hinge that splits it into left and right halves.
///
/// Always false: the framework's `MediaQueryData` has no hinge to read
/// (upstream reads `MediaQuery.of(context).hinge`), so the foldable layouts
/// upstream keeps behind this check are unreachable here. Logged in
/// PORTING.md.
pub fn is_display_foldable(_context: &BuildContext) -> bool {
    false
}

/// Whether the window counts as medium or large -- upstream's
/// `isDisplayDesktop`. A foldable is never desktop, because only part of its
/// display is available to a widget.
pub fn is_display_desktop(context: &BuildContext) -> bool {
    !is_display_foldable(context) && window_type_of(context) >= AdaptiveWindowType::Medium
}

/// Whether the window is exactly medium -- upstream's `isDisplaySmallDesktop`.
#[allow(dead_code)] // Ported with the set; no page reads it yet.
pub fn is_display_small_desktop(context: &BuildContext) -> bool {
    window_type_of(context) == AdaptiveWindowType::Medium
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window type a width falls into, without a `BuildContext`: the same
    /// edges, driven directly so the table can be tested.
    fn window_type(width: f32) -> AdaptiveWindowType {
        let mut kind = AdaptiveWindowType::XSmall;
        for (edge, window_type) in EDGES {
            if width >= *edge {
                kind = *window_type;
            }
        }
        kind
    }

    #[test]
    fn the_edges_are_the_packages_table() {
        assert_eq!(window_type(0.0), AdaptiveWindowType::XSmall);
        assert_eq!(window_type(599.0), AdaptiveWindowType::XSmall);
        assert_eq!(window_type(600.0), AdaptiveWindowType::Small);
        assert_eq!(window_type(1023.0), AdaptiveWindowType::Small);
        assert_eq!(window_type(1024.0), AdaptiveWindowType::Medium);
        assert_eq!(window_type(1439.0), AdaptiveWindowType::Medium);
        assert_eq!(window_type(1440.0), AdaptiveWindowType::Large);
        assert_eq!(window_type(1919.0), AdaptiveWindowType::Large);
        assert_eq!(window_type(1920.0), AdaptiveWindowType::XLarge);
    }

    #[test]
    fn desktop_starts_at_medium() {
        // Upstream's rule: `getWindowType(context) >= AdaptiveWindowType.medium`.
        assert!(AdaptiveWindowType::Medium >= AdaptiveWindowType::Medium);
        assert!(AdaptiveWindowType::Small < AdaptiveWindowType::Medium);
    }
}
