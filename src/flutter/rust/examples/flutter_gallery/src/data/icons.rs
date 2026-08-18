// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The gallery's icon table.
//!
//! Ported from `lib/data/icons.dart` (flutter/gallery @ d12640d). The same
//! table is what `tools/extract_catalog.py` reads into `upstream_icons.json`
//! for the catalogue generator, so the codepoints here and the ones in
//! `data/demos.rs` come from the same parse of the same file.
//!
//! Forty of the forty-four live in the gallery's own icon font; the last four
//! upstream takes from Material's (`navigationRail` is `Icons.vertical_split`,
//! and so on), and their codepoints are Material's, matching the
//! `MATERIAL_ICONS` map in `tools/gen_catalog.py`.

/// A glyph in an icon font: the private-use codepoint and the family it is
/// registered under. That pair is all an icon is, here as upstream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconData {
    pub glyph: &'static str,
    pub font_family: &'static str,
}

/// Upstream's `GalleryIcons`, with the font family carried on each entry the
/// way upstream's `IconData(fontFamily: ...)` does.
#[allow(dead_code)] // The complete table, like upstream's; the screens that
                    // read most of these are later batches.
pub mod gallery_icons {
    use super::IconData;
    use crate::data::demos::{GALLERY_ICONS, MATERIAL_ICONS};

    const fn gallery(glyph: &'static str) -> IconData {
        IconData {
            glyph,
            font_family: GALLERY_ICONS,
        }
    }

    const fn material(glyph: &'static str) -> IconData {
        IconData {
            glyph,
            font_family: MATERIAL_ICONS,
        }
    }

    pub const TOOLTIP: IconData = gallery("\u{e900}");
    pub const TEXT_FIELDS_ALT: IconData = gallery("\u{e901}");
    pub const TABS: IconData = gallery("\u{e902}");
    pub const SWITCHES: IconData = gallery("\u{e903}");
    pub const SLIDERS: IconData = gallery("\u{e904}");
    pub const SHRINE: IconData = gallery("\u{e905}");
    pub const SENTIMENT_VERY_SATISFIED: IconData = gallery("\u{e906}");
    pub const REFRESH: IconData = gallery("\u{e907}");
    pub const PROGRESS_ACTIVITY: IconData = gallery("\u{e908}");
    pub const PHONE_IPHONE: IconData = gallery("\u{e909}");
    pub const PAGE_CONTROL: IconData = gallery("\u{e90a}");
    pub const MORE_VERT: IconData = gallery("\u{e90b}");
    pub const MENU: IconData = gallery("\u{e90c}");
    pub const LIST_ALT: IconData = gallery("\u{e90d}");
    pub const GRID_ON: IconData = gallery("\u{e90e}");
    pub const EXPAND_ALL: IconData = gallery("\u{e90f}");
    pub const EVENT: IconData = gallery("\u{e910}");
    pub const DRIVE_VIDEO: IconData = gallery("\u{e911}");
    pub const DIALOGS: IconData = gallery("\u{e912}");
    pub const DATA_TABLE: IconData = gallery("\u{e913}");
    pub const CUSTOM_TYPOGRAPHY: IconData = gallery("\u{e914}");
    pub const COLORS: IconData = gallery("\u{e915}");
    pub const CHIPS: IconData = gallery("\u{e916}");
    pub const CHECK_BOX: IconData = gallery("\u{e917}");
    pub const CARDS: IconData = gallery("\u{e918}");
    pub const BUTTONS: IconData = gallery("\u{e919}");
    pub const BOTTOM_SHEETS: IconData = gallery("\u{e91a}");
    pub const BOTTOM_NAVIGATION: IconData = gallery("\u{e91b}");
    pub const ANIMATION: IconData = gallery("\u{e91c}");
    pub const ACCOUNT_BOX: IconData = gallery("\u{e91d}");
    pub const SNACKBAR: IconData = gallery("\u{e91e}");
    pub const CATEGORY_MDC: IconData = gallery("\u{e91f}");
    pub const CUPERTINO_PROGRESS: IconData = gallery("\u{e920}");
    pub const CUPERTINO_PULL_TO_REFRESH: IconData = gallery("\u{e921}");
    pub const CUPERTINO_SWITCH: IconData = gallery("\u{e922}");
    pub const GENERIC_BUTTONS: IconData = gallery("\u{e923}");
    pub const BACKDROP: IconData = gallery("\u{e924}");
    pub const BOTTOM_APP_BAR: IconData = gallery("\u{e925}");
    pub const BOTTOM_SHEET_PERSISTENT: IconData = gallery("\u{e926}");
    pub const LISTS_LEAVE_BEHIND: IconData = gallery("\u{e927}");

    // Upstream aliases these four to Material's own icons rather than adding
    // them to the gallery font.
    pub const NAVIGATION_RAIL: IconData = material("\u{e69f}"); // Icons.vertical_split
    pub const APPBAR: IconData = material("\u{e6de}"); // Icons.web_asset
    pub const DIVIDER: IconData = material("\u{e19f}"); // Icons.credit_card
    pub const SEARCH: IconData = material("\u{e567}"); // Icons.search
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entry in the table, so the tests below count the table rather
    /// than a restatement of it.
    const ALL: &[IconData] = &[
        gallery_icons::TOOLTIP,
        gallery_icons::TEXT_FIELDS_ALT,
        gallery_icons::TABS,
        gallery_icons::SWITCHES,
        gallery_icons::SLIDERS,
        gallery_icons::SHRINE,
        gallery_icons::SENTIMENT_VERY_SATISFIED,
        gallery_icons::REFRESH,
        gallery_icons::PROGRESS_ACTIVITY,
        gallery_icons::PHONE_IPHONE,
        gallery_icons::PAGE_CONTROL,
        gallery_icons::MORE_VERT,
        gallery_icons::MENU,
        gallery_icons::LIST_ALT,
        gallery_icons::GRID_ON,
        gallery_icons::EXPAND_ALL,
        gallery_icons::EVENT,
        gallery_icons::DRIVE_VIDEO,
        gallery_icons::DIALOGS,
        gallery_icons::DATA_TABLE,
        gallery_icons::CUSTOM_TYPOGRAPHY,
        gallery_icons::COLORS,
        gallery_icons::CHIPS,
        gallery_icons::CHECK_BOX,
        gallery_icons::CARDS,
        gallery_icons::BUTTONS,
        gallery_icons::BOTTOM_SHEETS,
        gallery_icons::BOTTOM_NAVIGATION,
        gallery_icons::ANIMATION,
        gallery_icons::ACCOUNT_BOX,
        gallery_icons::SNACKBAR,
        gallery_icons::CATEGORY_MDC,
        gallery_icons::CUPERTINO_PROGRESS,
        gallery_icons::CUPERTINO_PULL_TO_REFRESH,
        gallery_icons::CUPERTINO_SWITCH,
        gallery_icons::GENERIC_BUTTONS,
        gallery_icons::BACKDROP,
        gallery_icons::BOTTOM_APP_BAR,
        gallery_icons::BOTTOM_SHEET_PERSISTENT,
        gallery_icons::LISTS_LEAVE_BEHIND,
        gallery_icons::NAVIGATION_RAIL,
        gallery_icons::APPBAR,
        gallery_icons::DIVIDER,
        gallery_icons::SEARCH,
    ];

    #[test]
    fn the_table_is_upstream_size() {
        // 40 in the gallery font plus the four Material aliases.
        assert_eq!(ALL.len(), 44);
    }

    #[test]
    fn every_glyph_is_a_single_private_use_codepoint() {
        for icon in ALL {
            let mut chars = icon.glyph.chars();
            let glyph = chars.next().expect("an icon is one character");
            assert!(chars.next().is_none(), "more than one codepoint");
            assert!(
                (0xE000..=0xF8FF).contains(&(glyph as u32)),
                "not private use"
            );
        }
    }

    #[test]
    fn the_two_fonts_split_the_way_upstream_splits_them() {
        let gallery = ALL
            .iter()
            .filter(|icon| icon.font_family == crate::data::demos::GALLERY_ICONS)
            .count();
        assert_eq!(gallery, 40);
    }
}
