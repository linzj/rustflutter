// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Constants shared between files.
//!
//! Ported from `lib/constants.dart` (flutter/gallery @ d12640d). Upstream's
//! rule for this file is its own first comment: "Only put constants shared
//! between files here."

use std::time::Duration;

/// Height of the 'Gallery' header.
#[allow(dead_code)] // Shared constants, ported as a set; the screens that read
                    // some of them are later batches.
pub const GALLERY_HEADER_HEIGHT: f32 = 64.0;

/// The font size delta for headline4 font.
#[allow(dead_code)]
pub const DESKTOP_DISPLAY1_FONT_DELTA: f32 = 16.0;

/// The width of the settingsDesktop.
#[allow(dead_code)]
pub const DESKTOP_SETTINGS_WIDTH: f32 = 520.0;

/// Sentinel value for the system text scale factor option.
pub const SYSTEM_TEXT_SCALE_FACTOR_OPTION: f64 = -1.0;

/// The splash page animation duration.
#[allow(dead_code)] // The splash page itself is not ported (see PORTING.md);
                    // the duration stays so the set is upstream's set.
pub const SPLASH_PAGE_ANIMATION_DURATION: Duration = Duration::from_millis(300);

/// Half the splash page animation duration.
#[allow(dead_code)]
pub const HALF_SPLASH_PAGE_ANIMATION_DURATION: Duration = Duration::from_millis(150);

/// Duration for settings panel to open on mobile.
#[allow(dead_code)]
pub const SETTINGS_PANEL_MOBILE_ANIMATION_DURATION: Duration = Duration::from_millis(200);

/// Duration for settings panel to open on desktop.
#[allow(dead_code)]
pub const SETTINGS_PANEL_DESKTOP_ANIMATION_DURATION: Duration = Duration::from_millis(600);

/// Duration for home page elements to fade in.
#[allow(dead_code)]
pub const ENTRANCE_ANIMATION_DURATION: Duration = Duration::from_millis(200);

/// The desktop top padding for a page's first header (e.g. Gallery, Settings).
#[allow(dead_code)]
pub const FIRST_HEADER_DESKTOP_TOP_PADDING: f32 = 5.0;

/// A transparent image used to avoid loading images when they are not needed.
///
/// The bytes of the `transparent_image` package's `kTransparentImage`, a 1x1
/// transparent PNG -- upstream depends on the package for exactly this
/// constant, and a dependency for sixty-seven bytes is not one worth having.
#[allow(dead_code)]
pub const K_TRANSPARENT_IMAGE: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transparent_image_is_a_png_signature_and_all() {
        // A truncated copy would fail to decode at the first use, which is
        // exactly the bug this constant exists to be boring about.
        assert_eq!(&K_TRANSPARENT_IMAGE[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(K_TRANSPARENT_IMAGE.len(), 67);
    }
}
