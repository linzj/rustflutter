// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Three small descriptions a widget is given (upstream
//! `widgets/icon_data.dart`, `widgets/context_menu_button_item.dart`,
//! `widgets/bottom_navigation_bar_item.dart`).
//!
//! None of them is a widget. Each is what a widget is *told* -- which glyph
//! to draw, what a menu entry does, what one destination in a bar is -- and
//! they are together because a file each would be three lines each.
//!
//! # Recorded divergences
//!
//! * `IconDataProperty` is a `DiagnosticsProperty` subclass, which is the
//!   diagnostics tree (P10). Ledgered.
//! * Upstream's icon-tree-shaker annotations (`@RecordUse`, `@mustBeConst`)
//!   are how the Dart build finds which icons an application actually uses
//!   and drops the rest of the font. That is a build-time analysis of Dart
//!   source; there is nothing for it to attach to here.

use crate::engine::Color;
use crate::framework::AnyWidget;

/// Upstream `IconData`: which glyph an icon is.
///
/// An icon is a character in a font. That is the whole idea and it is why
/// this is data rather than an image: the codepoint picks the glyph, the
/// family picks the font, and drawing it is drawing a one-character string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconData {
    /// The codepoint of the glyph, which for an icon font is in a
    /// private-use area -- there is no Unicode character for "settings".
    pub code_point: u32,
    /// The family the glyph is in. Absent means the default icon font, which
    /// upstream resolves to Material Icons.
    pub font_family: Option<String>,
    /// The package the font shipped in, which upstream needs because a font
    /// from a package is registered under a prefixed name.
    pub font_package: Option<String>,
    /// Whether the glyph should be mirrored in a right-to-left layout. True
    /// for anything with a direction in it -- an arrow, a "next" chevron --
    /// and false for anything symmetrical, which is why it is a flag on the
    /// icon and not a rule about the layout.
    pub match_text_direction: bool,
    /// Families to try when the first has no glyph at that codepoint.
    pub font_family_fallback: Option<Vec<String>>,
}

impl IconData {
    pub fn new(code_point: u32) -> IconData {
        IconData {
            code_point,
            font_family: None,
            font_package: None,
            match_text_direction: false,
            font_family_fallback: None,
        }
    }

    pub fn with_font_family(mut self, font_family: impl Into<String>) -> Self {
        self.font_family = Some(font_family.into());
        self
    }

    pub fn with_font_package(mut self, font_package: impl Into<String>) -> Self {
        self.font_package = Some(font_package.into());
        self
    }

    pub fn with_match_text_direction(mut self, mirror: bool) -> Self {
        self.match_text_direction = mirror;
        self
    }

    pub fn with_font_family_fallback(mut self, families: Vec<String>) -> Self {
        self.font_family_fallback = Some(families);
        self
    }

    /// The glyph as the one-character string that draws it, or nothing when
    /// the codepoint is not a character at all.
    ///
    /// This is the whole of how an icon reaches the screen in this crate: a
    /// text run of one character in the icon family. See
    /// [`with_font_family`](crate::widgets::Text::with_font_family).
    pub fn to_glyph(&self) -> Option<String> {
        char::from_u32(self.code_point).map(|glyph| glyph.to_string())
    }
}

/// Upstream `ContextMenuButtonType`: which of the standard entries a menu
/// button is.
///
/// The type rather than the label is what travels, because the label is
/// translated and the platform may also want to supply its own -- an iOS
/// "Look Up" is not a string this crate should be inventing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContextMenuButtonType {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Delete,
    LookUp,
    SearchWeb,
    Share,
    LiveTextInput,
    /// Not one of the standard entries, so the label is the application's and
    /// has to be given.
    #[default]
    Custom,
}

/// Upstream `ContextMenuButtonItem`: one entry of a text selection menu.
#[derive(Clone)]
pub struct ContextMenuButtonItem {
    /// What the entry does. Absent means the entry is shown disabled --
    /// upstream's nullable callback, and the reason it is nullable rather
    /// than a separate `enabled` flag: an entry with nothing to do and an
    /// entry that is switched off are the same thing.
    pub on_pressed: Option<std::rc::Rc<dyn Fn()>>,
    pub button_type: ContextMenuButtonType,
    /// The label, when the application has one. A standard type with no label
    /// is labelled by whatever builds the menu, in the reader's language.
    pub label: Option<String>,
}

impl ContextMenuButtonItem {
    pub fn new(button_type: ContextMenuButtonType) -> ContextMenuButtonItem {
        ContextMenuButtonItem {
            on_pressed: None,
            button_type,
            label: None,
        }
    }

    pub fn with_on_pressed(mut self, on_pressed: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(std::rc::Rc::new(on_pressed));
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Whether the entry can be tapped, which is upstream's "has a callback".
    pub fn is_enabled(&self) -> bool {
        self.on_pressed.is_some()
    }

    /// Upstream `copyWith`.
    ///
    /// Upstream's takes each field as nullable and keeps the old one for a
    /// null -- which means it cannot *clear* a field, and upstream lives with
    /// that. The same here, so that the two behave alike.
    pub fn copy_with(
        &self,
        on_pressed: Option<std::rc::Rc<dyn Fn()>>,
        button_type: Option<ContextMenuButtonType>,
        label: Option<String>,
    ) -> ContextMenuButtonItem {
        ContextMenuButtonItem {
            on_pressed: on_pressed.or_else(|| self.on_pressed.clone()),
            button_type: button_type.unwrap_or(self.button_type),
            label: label.or_else(|| self.label.clone()),
        }
    }
}

impl std::fmt::Debug for ContextMenuButtonItem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContextMenuButtonItem")
            .field("type", &self.button_type)
            .field("label", &self.label)
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

/// Upstream `BottomNavigationBarItem`: one destination in a bottom bar.
pub struct BottomNavigationBarItem {
    pub key: Option<u64>,
    pub icon: AnyWidget,
    /// What is shown while this destination is the selected one. Upstream
    /// defaults it to `icon`, which is why it is not optional: a bar with a
    /// filled variant for the selected tab and a bar without are the same
    /// code path.
    pub active_icon: AnyWidget,
    pub label: Option<String>,
    /// Upstream's per-item colour, for a bar whose background follows the
    /// selected destination.
    pub background_color: Option<Color>,
    pub tooltip: Option<String>,
    /// What a screen reader says. Separate from the label because the label
    /// is often a single word that only means something beside its icon.
    pub semantics_label: Option<String>,
}

impl BottomNavigationBarItem {
    /// The icon is required and everything else is not, which is upstream's
    /// shape: a destination with no icon is not a destination.
    pub fn new(icon: AnyWidget, active_icon: AnyWidget) -> BottomNavigationBarItem {
        BottomNavigationBarItem {
            key: None,
            icon,
            active_icon,
            label: None,
            background_color: None,
            tooltip: None,
            semantics_label: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn with_semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::leaf;
    use crate::widgets::SizedBox;

    #[test]
    fn an_icon_is_a_character_in_a_font() {
        // The whole idea, and why this is data rather than an image: the
        // codepoint picks the glyph and drawing it is drawing a
        // one-character string.
        let settings = IconData::new(0xE8B8).with_font_family("MaterialIcons");
        assert_eq!(settings.to_glyph(), Some("\u{E8B8}".to_string()));
        assert_eq!(settings.to_glyph().unwrap().chars().count(), 1);
        assert_eq!(settings.font_family.as_deref(), Some("MaterialIcons"));
    }

    #[test]
    fn an_icon_codepoint_is_usually_in_a_private_use_area() {
        // There is no Unicode character for "settings", which is why an icon
        // font puts its glyphs where nothing else claims.
        let settings = IconData::new(0xE8B8);
        assert!(
            (0xE000..=0xF8FF).contains(&settings.code_point),
            "the basic private use area"
        );
        // A codepoint that is not a character at all has no glyph, rather
        // than a panic on the way to the shaper.
        assert_eq!(IconData::new(0xD800).to_glyph(), None, "a lone surrogate");
        assert_eq!(IconData::new(0x11_0000).to_glyph(), None, "past the end");
    }

    #[test]
    fn mirroring_is_a_property_of_the_icon_and_not_of_the_layout() {
        // An arrow flips in a right-to-left layout and a settings cog does
        // not. Only the icon knows which it is, which is why the flag is
        // here and not a rule the layout applies to everything.
        assert!(!IconData::new(0xE8B8).match_text_direction);
        assert!(
            IconData::new(0xE5C4)
                .with_match_text_direction(true)
                .match_text_direction
        );
    }

    #[test]
    fn two_icons_are_equal_only_if_everything_about_them_is() {
        // The font matters as much as the codepoint: the same number in two
        // families is two different pictures.
        let one = IconData::new(0xE8B8).with_font_family("MaterialIcons");
        assert_eq!(one, IconData::new(0xE8B8).with_font_family("MaterialIcons"));
        assert_ne!(
            one,
            IconData::new(0xE8B8).with_font_family("CupertinoIcons")
        );
        assert_ne!(one, IconData::new(0xE8B9).with_font_family("MaterialIcons"));
        assert_ne!(
            one,
            IconData::new(0xE8B8)
                .with_font_family("MaterialIcons")
                .with_match_text_direction(true)
        );
    }

    #[test]
    fn a_menu_entry_with_nothing_to_do_is_a_disabled_entry() {
        // Upstream's nullable callback rather than a separate `enabled` flag,
        // and that is the right shape: an entry with nothing to do and one
        // that is switched off are the same thing.
        let paste = ContextMenuButtonItem::new(ContextMenuButtonType::Paste);
        assert!(!paste.is_enabled());
        assert!(paste.with_on_pressed(|| {}).is_enabled());
    }

    #[test]
    fn a_standard_entry_travels_as_its_type_and_not_its_label() {
        // The label is translated, and the platform may want to supply its
        // own -- an iOS "Look Up" is not a string this crate should invent.
        let look_up = ContextMenuButtonItem::new(ContextMenuButtonType::LookUp);
        assert_eq!(look_up.button_type, ContextMenuButtonType::LookUp);
        assert_eq!(look_up.label, None);
        // A custom entry is the one that has to bring a label, because
        // nothing else knows what it says.
        assert_eq!(
            ContextMenuButtonType::default(),
            ContextMenuButtonType::Custom
        );
    }

    #[test]
    fn copy_with_keeps_what_it_was_not_given_and_cannot_clear() {
        // Upstream's `copyWith` takes nullables and keeps the old value for a
        // null, so it cannot clear a field -- and upstream lives with that.
        // The same here, so the two behave alike rather than this one being
        // quietly better and differently.
        let item = ContextMenuButtonItem::new(ContextMenuButtonType::Copy).with_label("Copy");
        let retyped = item.copy_with(None, Some(ContextMenuButtonType::Cut), None);
        assert_eq!(retyped.button_type, ContextMenuButtonType::Cut);
        assert_eq!(
            retyped.label.as_deref(),
            Some("Copy"),
            "not cleared, because a null means keep"
        );
    }

    #[test]
    fn a_destination_without_a_selected_variant_uses_its_ordinary_icon() {
        // Upstream defaults `activeIcon` to `icon`, which is why the field is
        // not optional: a bar with a filled variant for the selected tab and
        // a bar without are the same code path.
        let item = BottomNavigationBarItem::new(
            leaf(|| SizedBox::new(24.0, 24.0)),
            leaf(|| SizedBox::new(24.0, 24.0)),
        )
        .with_label("Home");
        assert_eq!(item.label.as_deref(), Some("Home"));
        assert_eq!(item.semantics_label, None);
        assert_eq!(item.tooltip, None);
    }

    #[test]
    fn what_a_screen_reader_says_is_separate_from_the_label() {
        // A label is often one word that only means something beside its
        // icon: "Home" beside a house is clear and "Home" read aloud alone is
        // not necessarily.
        let item = BottomNavigationBarItem::new(
            leaf(|| SizedBox::new(24.0, 24.0)),
            leaf(|| SizedBox::new(24.0, 24.0)),
        )
        .with_label("Home")
        .with_semantics_label("Home tab, 1 of 3");
        assert_ne!(item.label, item.semantics_label);
    }
}
