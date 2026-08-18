// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Mirrors upstream `lib/studies/rally/` (flutter/gallery @ d12640d): one
//! child module per upstream file.
//!
//! The study is a small app of its own: [`app`] is the `RallyApp` root (theme,
//! route stack), [`login`] and [`home`] the two routes, [`tabs`] the five tab
//! views, and [`data`], [`formatters`], [`finance`] and [`charts`] the model
//! and drawing underneath. `studies::page` dispatches the `rally` slug to
//! [`home::screen`], which mounts the app root.
//!
//! The Material Icons codepoints Rally draws (`Icons.pie_chart` and friends)
//! are [`icons`], verified against the shipped `MaterialIcons-Regular.otf`
//! cmap; they live here rather than in `crate::data::demos::icon` because
//! that table belongs to another batch.

pub mod app;
pub mod charts;
pub mod colors;
pub mod data;
pub mod finance;
pub mod formatters;
pub mod home;
pub mod login;
pub mod routes;
pub mod tabs;

/// The Material Icons glyphs Rally uses, named for the upstream `Icons.*`
/// members. The codepoints are the shipped font's own (the gallery's
/// `MaterialIcons-Regular.otf` is the full font, not a subset).
pub mod icons {
    use crate::data::demos::MATERIAL_ICONS;
    use crate::data::icons::IconData;

    const fn material(glyph: &'static str) -> IconData {
        IconData {
            glyph,
            font_family: MATERIAL_ICONS,
        }
    }

    /// `Icons.pie_chart`.
    pub const PIE_CHART: IconData = material("\u{e4c3}");
    /// `Icons.attach_money`.
    pub const ATTACH_MONEY: IconData = material("\u{e0b2}");
    /// `Icons.money_off`.
    pub const MONEY_OFF: IconData = material("\u{e3f9}");
    /// `Icons.table_chart`.
    pub const TABLE_CHART: IconData = material("\u{e63a}");
    /// `Icons.settings`.
    pub const SETTINGS: IconData = material("\u{e57f}");
    /// `Icons.chevron_right`.
    pub const CHEVRON_RIGHT: IconData = material("\u{e15f}");
    /// `Icons.sort`.
    pub const SORT: IconData = material("\u{e5d2}");
    /// `Icons.credit_card`.
    pub const CREDIT_CARD: IconData = material("\u{e19f}");
    /// `Icons.not_interested`.
    pub const NOT_INTERESTED: IconData = material("\u{e446}");
    /// `Icons.check_circle_outline`.
    pub const CHECK_CIRCLE_OUTLINE: IconData = material("\u{e15a}");
    /// `Icons.lock`.
    pub const LOCK: IconData = material("\u{e3ae}");
    /// `Icons.arrow_back`.
    pub const ARROW_BACK: IconData = material("\u{e092}");
}

/// The hit-test ids Rally allocates, off the shared study base. One study is
/// on screen at a time, so the base cannot collide with another study's.
pub(crate) mod ids {
    use crate::app::ids::STUDY_LOCAL as BASE;

    /// The five tabs: `TAB + index`.
    pub const TAB: u64 = BASE;
    /// The scrollable views, in route/tab order: `SCROLL + view`.
    pub const SCROLL: u64 = BASE + 10;
    pub const SCROLL_LOGIN: u64 = SCROLL;
    pub const SCROLL_OVERVIEW: u64 = SCROLL + 1;
    pub const SCROLL_ACCOUNTS: u64 = SCROLL + 2;
    pub const SCROLL_BILLS: u64 = SCROLL + 3;
    pub const SCROLL_BUDGETS: u64 = SCROLL + 4;
    pub const SCROLL_SETTINGS: u64 = SCROLL + 5;
    pub const SCROLL_DETAILS: u64 = SCROLL + 6;
    /// The username and password fields.
    pub const USERNAME_FIELD: u64 = BASE + 20;
    pub const PASSWORD_FIELD: u64 = BASE + 21;
    /// The login screen's buttons.
    pub const THUMB_BUTTON: u64 = BASE + 22;
    pub const LOGIN_BUTTON: u64 = BASE + 23;
    pub const SIGN_UP_BUTTON: u64 = BASE + 24;
    /// A financial entity card opening the details page: `CARD + index`.
    pub const CARD: u64 = BASE + 30;
    /// A settings row: `SETTINGS_ITEM + index`.
    pub const SETTINGS_ITEM: u64 = BASE + 50;
    /// The details page's back button.
    pub const DETAILS_BACK: u64 = BASE + 70;
}
