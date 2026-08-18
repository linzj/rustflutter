// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Mirrors upstream `lib/studies/starter/` (flutter/gallery @ d12640d): one
//! child module per upstream file.
//!
//! All three are ported: `app` holds the study's theme and colour scheme,
//! `home` the adaptive page (`studies::page` routes the `starter` slug to
//! `home::screen`), and `routes` the `defaultRoute` constant.

pub mod app;
pub mod home;
pub mod routes;
