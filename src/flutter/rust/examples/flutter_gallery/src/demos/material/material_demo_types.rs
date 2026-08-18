// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/material_demo_types.dart` (flutter/gallery
//! @ d12640d).
//!
//! Upstream each demo with variants takes one of these enums to pick its
//! configuration, and the catalogue builds one `GalleryDemoConfiguration` per
//! variant. Here the catalogue is flattened to one configuration per demo
//! (PORTING.md: "demo options section is unreachable"), so nothing selects a
//! variant and the enums are metadata only -- kept so the phase-2 alignment
//! has the variant lists without re-reading upstream.

#![allow(dead_code)]

/// Upstream `BottomNavigationDemoType`.
pub enum BottomNavigationDemoType {
    WithLabels,
    WithoutLabels,
}

/// Upstream `BottomSheetDemoType`.
pub enum BottomSheetDemoType {
    Persistent,
    Modal,
}

/// Upstream `ButtonDemoType`.
pub enum ButtonDemoType {
    Text,
    Elevated,
    Outlined,
    Toggle,
    Floating,
}

/// Upstream `ChipDemoType`.
pub enum ChipDemoType {
    Action,
    Choice,
    Filter,
    Input,
}

/// Upstream `DialogDemoType`.
pub enum DialogDemoType {
    Alert,
    AlertTitle,
    Simple,
    Fullscreen,
}

/// Upstream `GridListDemoType`.
pub enum GridListDemoType {
    ImageOnly,
    Header,
    Footer,
}

/// Upstream `ListDemoType`.
pub enum ListDemoType {
    OneLine,
    TwoLine,
}

/// Upstream `MenuDemoType`.
pub enum MenuDemoType {
    ContextMenu,
    SectionedMenu,
    SimpleMenu,
    ChecklistMenu,
}

/// Upstream `PickerDemoType`.
pub enum PickerDemoType {
    Date,
    Time,
    Range,
}

/// Upstream `ProgressIndicatorDemoType`.
pub enum ProgressIndicatorDemoType {
    Circular,
    Linear,
}

/// Upstream `SelectionControlsDemoType`.
pub enum SelectionControlsDemoType {
    Checkbox,
    Radio,
    Switches,
}

/// Upstream `SlidersDemoType`.
pub enum SlidersDemoType {
    Sliders,
    RangeSliders,
    CustomSliders,
}

/// Upstream `TabsDemoType`.
pub enum TabsDemoType {
    Scrollable,
    NonScrollable,
}

/// Upstream `DividerDemoType`.
pub enum DividerDemoType {
    Horizontal,
    Vertical,
}
