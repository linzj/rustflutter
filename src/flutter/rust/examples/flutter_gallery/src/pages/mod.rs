// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Mirrors upstream `lib/pages/` (flutter/gallery @ d12640d), plus
//! `adaptive_layout`, which is upstream's `lib/layout/adaptive.dart` -- it
//! landed with the pages batch (M-C) and lives here; the path delta is logged
//! in PORTING.md.

pub mod about;
pub mod adaptive_layout;
pub mod backdrop;
pub mod category_list_item;
pub mod demo;
pub mod home;
pub mod settings;
pub mod splash;
