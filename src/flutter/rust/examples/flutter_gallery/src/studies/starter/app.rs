// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/starter/app.dart` (flutter/gallery @ d12640d):
//! the `StarterApp` root widget.
//!
//! Upstream's `StarterApp` is a `MaterialApp`: a light-only theme over one
//! route (`defaultRoute` -> `_Home`, an `ApplyTextOptions` around
//! `HomePage`). Here the gallery's navigator already owns routing and the
//! gallery root already applies the text-scale option, so what survives is
//! the theme, published inside the study by [`app`], and the colour scheme
//! constants the home page draws with. This undoes the
//! "studies-share-gallery-theme" divergence (PORTING.md) for this study.
//!
//! Divergences, each also marked at its site:
//!
//! * The scheme maps onto the framework's smaller [`Theme`], which has no
//!   secondary or container roles; the home page reads [`SECONDARY`] and
//!   [`ON_SECONDARY`] directly for the floating action button.
//! * Upstream's typeface is the platform default (`ThemeData` carries no
//!   `fontFamily`, so Roboto); Roboto does not ship here, so the study draws
//!   in the framework's default face -- the same substitution PORTING.md
//!   logs for the typography demo.
//! * `highlightColor: Colors.transparent` and `platform` have no
//!   counterparts: the framework's ink and host model do not read them.
//! * `restorationScopeId` / `RestorationMixin` are not carried anywhere
//!   (PORTING.md's M-D note).

use rustflutter::components::Theme;
use rustflutter::engine::Color;
use rustflutter::framework::{AnyWidget, provide};

/// Upstream's `_primaryColor`.
pub(crate) const PRIMARY: Color = Color(0xFF6200EE);
/// Upstream's `ColorScheme.primaryContainer`.
#[allow(dead_code)] // Part of the upstream scheme; nothing here reads it yet.
pub(crate) const PRIMARY_CONTAINER: Color = Color(0xFF3700B3);
/// Upstream's `ColorScheme.secondary` -- the floating action button's fill.
pub(crate) const SECONDARY: Color = Color(0xFF03DAC6);
/// Upstream's `ColorScheme.secondaryContainer`.
#[allow(dead_code)] // Part of the upstream scheme; nothing here reads it yet.
pub(crate) const SECONDARY_CONTAINER: Color = Color(0xFF018786);
/// Upstream's `ColorScheme.error`.
pub(crate) const ERROR: Color = Color(0xFFB00020);
/// Upstream's `ColorScheme.onSecondary` -- what sits on the button's fill,
/// and the headline's ink.
pub(crate) const ON_SECONDARY: Color = Color::BLACK;
/// Upstream's `DividerThemeData.color`.
pub(crate) const DIVIDER: Color = Color(0xFFE5E5E5);

/// The component library's view of upstream's `ColorScheme`.
///
/// The scheme is light-only upstream (`brightness: Brightness.light`), so
/// there is one theme and the app's brightness option does not follow it.
/// The mapping: `background` and `surface` are upstream's, `outline` is the
/// divider colour (the one border colour upstream names), and the text roles
/// take `onSurface`/`onBackground`, both black. `text_muted` is black at
/// 54%, Material's light-theme secondary text (`Colors.black54`, what the
/// drawer's unselected icons and hints read).
pub(crate) fn theme() -> Theme {
    Theme {
        background: Color::WHITE,
        surface: Color::WHITE,
        surface_variant: Color::WHITE,
        outline: DIVIDER,
        primary: PRIMARY,
        on_primary: Color::WHITE,
        danger: ERROR,
        text: Color::BLACK,
        text_muted: Color::BLACK.with_alpha(0x8A),
        // Upstream's shapes are Material 2's: 4 on the small components the
        // framework draws with this radius.
        radius: 4.0,
        spacing: 8.0,
        // Upstream's `bodyMedium` and `titleLarge` (the 2018 scale's
        // bodyText2 and headline6).
        body_size: 14.0,
        title_size: 20.0,
        // Upstream's default is Roboto, which does not ship here; None is
        // the framework's default face. See the module header.
        font_family: None,
    }
}

/// What is left of `StarterApp.build`: the study's theme over its home page.
///
/// The `MaterialApp` around it -- the route table, the localizations
/// delegates, the locale option -- is the gallery's own, and
/// `ApplyTextOptions` is the gallery root's `MediaQuery` text-scale override
/// (`src/app.rs`); neither is re-done inside the study.
pub(crate) fn app(child: AnyWidget) -> AnyWidget {
    provide(theme(), child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scheme_constants_are_upstream() {
        // flutter/gallery @ d12640d, `lib/studies/starter/app.dart`.
        assert_eq!(PRIMARY, Color(0xFF6200EE));
        assert_eq!(PRIMARY_CONTAINER, Color(0xFF3700B3));
        assert_eq!(SECONDARY, Color(0xFF03DAC6));
        assert_eq!(SECONDARY_CONTAINER, Color(0xFF018786));
        assert_eq!(ERROR, Color(0xFFB00020));
        assert_eq!(DIVIDER, Color(0xFFE5E5E5));
    }

    #[test]
    fn the_theme_carries_the_scheme() {
        let theme = theme();
        assert_eq!(theme.primary, PRIMARY);
        assert_eq!(theme.background, Color::WHITE);
        assert_eq!(theme.surface, Color::WHITE);
        assert_eq!(theme.outline, DIVIDER);
        assert_eq!(theme.on_primary, Color::WHITE);
        assert_eq!(theme.text, Color::BLACK);
        assert_eq!(theme.danger, ERROR);
    }
}
