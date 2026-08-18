// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Mirrors upstream `lib/studies/shrine/` (flutter/gallery @ d12640d): one
//! child module per upstream file.
//!
//! `app.rs` holds the study's root stateful component (upstream's
//! `ShrineApp`); `home.rs` the product page and the home stack, and its
//! `screen()` is the entry `studies::page` dispatches to.
//! `model::products_repository` is generated (`tools/gen_shrine.py`).

pub mod app;
pub mod backdrop;
pub mod category_menu_page;
pub mod colors;
pub mod expanding_bottom_sheet;
pub mod home;
pub mod login;
pub mod model;
pub mod page_status;
pub mod routes;
pub mod scrim;
pub mod shopping_cart;
pub mod supplemental;
pub mod theme;
pub mod triangle_category_indicator;

/// Hit-test identities, allocated from the studies' base (`ids::STUDY_LOCAL`)
/// as fixed constants -- an id must be stable across rebuilds, so nothing
/// here is ever a counter.
pub(crate) mod ids {
    use crate::app::ids::STUDY_LOCAL as BASE;

    /// The backdrop's menu button (the diamond / slanted-menu crossfade).
    pub const MENU_BUTTON: u64 = BASE + 10;
    /// The backdrop's search and settings actions (upstream's are no-ops).
    pub const SEARCH_BUTTON: u64 = BASE + 11;
    pub const SETTINGS_BUTTON: u64 = BASE + 12;
    /// The front layer's top area, which closes the menu when it is open.
    pub const FRONT_TOP_AREA: u64 = BASE + 13;
    /// The scrim under the open cart.
    pub const SCRIM: u64 = BASE + 14;
    /// The collapsed cart sheet, tapped to open it.
    pub const CART_SHEET: u64 = BASE + 15;
    /// The cart page's close button.
    pub const CART_CLOSE: u64 = BASE + 16;
    /// The cart page's CLEAR CART button.
    pub const CLEAR_CART: u64 = BASE + 17;
    /// The login page's buttons and fields.
    pub const LOGIN_CANCEL: u64 = BASE + 20;
    pub const LOGIN_NEXT: u64 = BASE + 21;
    pub const LOGIN_USERNAME: u64 = BASE + 22;
    pub const LOGIN_PASSWORD: u64 = BASE + 23;
    /// The category menu's LOGOUT.
    pub const LOGOUT: u64 = BASE + 24;
    /// The desktop menu's search action.
    pub const MENU_SEARCH: u64 = BASE + 25;
    /// One per category (4), upstream's `categories`.
    pub const CATEGORY: u64 = BASE + 40;
    /// One per product (38): tapping a card adds it to the cart.
    pub const PRODUCT_CARD: u64 = BASE + 100;
    /// One per product (38): the cart row's remove button.
    pub const CART_ROW_REMOVE: u64 = BASE + 200;
    /// The study's scrollables: the product list, the cart list, the menu.
    pub const PRODUCT_SCROLL: u64 = BASE + 300;
    pub const CART_SCROLL: u64 = BASE + 301;
    pub const MENU_SCROLL: u64 = BASE + 302;
}
