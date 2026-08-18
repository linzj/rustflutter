// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/demo_types.dart` (flutter/gallery @
//! d12640d).
//!
//! Upstream's Cupertino demo type enums: a demo with variants takes one of
//! these to pick its configuration, and the catalogue builds one
//! `GalleryDemoConfiguration` per variant. `AlertDemoType` is the only one
//! upstream declares. Here the catalogue is flattened to one configuration
//! per demo (PORTING.md: "demo options section is unreachable"), so nothing
//! selects a variant and the enum is metadata only -- kept, the way
//! `material/material_demo_types.rs` keeps its enums, so the port batch has
//! the variant list without re-reading upstream.

#![allow(dead_code)]

/// Upstream `AlertDemoType`.
pub enum AlertDemoType {
    Alert,
    AlertTitle,
    AlertButtons,
    AlertButtonsOnly,
    ActionSheet,
}
