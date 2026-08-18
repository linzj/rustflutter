// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Skeleton for `lib/studies/shrine/supplemental/desktop_product_columns.dart` (flutter/gallery @ d12640d), upstream's
//! the `DesktopProductColumns`.
//!
//! Not ported yet: renders the shared not-written-yet placeholder, the way the
//! demos/cupertino skeleton did. The per-file port is the study batch's.

use rustflutter::framework::AnyWidget;

/// Upstream's `productCardAdditionalHeight` (`84.0 * 2`): the height of the
/// text below each product card. The layout math in
/// [`super::balanced_layout`] reads it; the widget tree it belongs to is the
/// skeleton below.
pub const PRODUCT_CARD_ADDITIONAL_HEIGHT: f64 = 84.0 * 2.0;

/// Upstream's `columnTopSpace`: the space at the top of every other column.
pub const COLUMN_TOP_SPACE: f64 = 84.0;

/// The stand-in body; see the module header.
#[allow(dead_code)]
pub(crate) fn screen() -> AnyWidget {
    crate::studies::not_written_yet("shrine")
}
