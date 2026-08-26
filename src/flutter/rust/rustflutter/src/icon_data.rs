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

impl ContextMenuButtonType {
    /// Upstream `CupertinoTextSelectionToolbarButton.getButtonLabel`, minus
    /// the item's own label, which the caller has already checked.
    ///
    /// Two entries answer with nothing at all. `Delete` and `LiveTextInput`
    /// are not entries an iOS text menu has, and `Custom` has no label but
    /// the one it was given -- so all three fall to the empty string rather
    /// than to a name this crate would have had to invent.
    pub fn cupertino_label(self) -> &'static str {
        use crate::cupertino_app::DefaultCupertinoLocalizations as L;
        match self {
            ContextMenuButtonType::Cut => L::CUT_BUTTON_LABEL,
            ContextMenuButtonType::Copy => L::COPY_BUTTON_LABEL,
            ContextMenuButtonType::Paste => L::PASTE_BUTTON_LABEL,
            ContextMenuButtonType::SelectAll => L::SELECT_ALL_BUTTON_LABEL,
            ContextMenuButtonType::LookUp => L::LOOK_UP_BUTTON_LABEL,
            ContextMenuButtonType::SearchWeb => L::SEARCH_WEB_BUTTON_LABEL,
            ContextMenuButtonType::Share => L::SHARE_BUTTON_LABEL,
            ContextMenuButtonType::LiveTextInput
            | ContextMenuButtonType::Delete
            | ContextMenuButtonType::Custom => "",
        }
    }

    /// Upstream `AdaptiveTextSelectionToolbar.getButtonLabel`.
    ///
    /// One rule with a Cupertino branch rather than two rules: upstream's
    /// switch hands iOS and macOS **straight to the Cupertino one**, so those
    /// two platforms get "Select All" and "Share..." from a Material menu
    /// too.
    ///
    /// Where the two tables both answer, they mostly agree. Where they do not
    /// is `selectAll`'s capital and `share`'s ellipsis, and those survive
    /// into every locale because each table is translated separately.
    ///
    /// And two entries exist here that do not exist there. `Delete` borrows
    /// the delete *tooltip* and upper-cases it -- the only label in either
    /// table that is derived rather than looked up, which is why it cannot be
    /// a constant. `LiveTextInput` takes "Scan text", which names the camera
    /// rather than the entry.
    pub fn material_label(self, platform: crate::editable_text::TargetPlatform) -> String {
        use crate::editable_text::TargetPlatform;
        use crate::material_app::DefaultMaterialLocalizations as L;
        if matches!(platform, TargetPlatform::IOS | TargetPlatform::MacOS) {
            return self.cupertino_label().to_string();
        }
        match self {
            ContextMenuButtonType::Cut => L::CUT_BUTTON_LABEL.to_string(),
            ContextMenuButtonType::Copy => L::COPY_BUTTON_LABEL.to_string(),
            ContextMenuButtonType::Paste => L::PASTE_BUTTON_LABEL.to_string(),
            ContextMenuButtonType::SelectAll => L::SELECT_ALL_BUTTON_LABEL.to_string(),
            ContextMenuButtonType::Delete => L::DELETE_BUTTON_TOOLTIP.to_uppercase(),
            ContextMenuButtonType::LookUp => L::LOOK_UP_BUTTON_LABEL.to_string(),
            ContextMenuButtonType::SearchWeb => L::SEARCH_WEB_BUTTON_LABEL.to_string(),
            ContextMenuButtonType::Share => L::SHARE_BUTTON_LABEL.to_string(),
            ContextMenuButtonType::LiveTextInput => L::SCAN_TEXT_BUTTON_LABEL.to_string(),
            ContextMenuButtonType::Custom => String::new(),
        }
    }
}

impl ContextMenuButtonItem {
    /// What this entry says, on `platform`.
    ///
    /// An item's own label wins outright and is checked first in both of
    /// upstream's versions -- a custom entry has no other source, and a
    /// standard entry an application has renamed keeps the name it was given.
    pub fn resolved_label(&self, platform: crate::editable_text::TargetPlatform) -> String {
        match &self.label {
            Some(label) => label.clone(),
            None => self.button_type.material_label(platform),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::leaf;
    use crate::widgets::SizedBox;

    // -- What a context menu entry says, tick 259 ----------------------------
    //
    // `ContextMenuButtonItem` travelled as a type with `label: None`, and the
    // comment above the enum said why -- "the label is translated and the
    // platform may also want to supply its own". True, and it left the crate
    // with no way to turn a type into a label at all.

    #[test]
    fn an_items_own_label_wins_and_is_asked_first() {
        // Upstream checks it before touching the localizations in both of its
        // versions. A custom entry has no other source, and a standard entry
        // an application has renamed keeps the name it was given.
        use crate::editable_text::TargetPlatform;
        let renamed =
            ContextMenuButtonItem::new(ContextMenuButtonType::Copy).with_label("Duplicate");
        assert_eq!(renamed.resolved_label(TargetPlatform::Android), "Duplicate");
        assert_eq!(renamed.resolved_label(TargetPlatform::IOS), "Duplicate");

        let standard = ContextMenuButtonItem::new(ContextMenuButtonType::Copy);
        assert_eq!(standard.resolved_label(TargetPlatform::Android), "Copy");
    }

    #[test]
    fn a_custom_entry_with_no_label_says_nothing_rather_than_something_invented() {
        use crate::editable_text::TargetPlatform;
        let bare = ContextMenuButtonItem::new(ContextMenuButtonType::Custom);
        assert_eq!(bare.resolved_label(TargetPlatform::Android), "");
        assert_eq!(bare.resolved_label(TargetPlatform::IOS), "");
    }

    #[test]
    fn ios_and_macos_read_the_cupertino_table_from_a_material_menu() {
        // Upstream's `AdaptiveTextSelectionToolbar.getButtonLabel` switches on
        // the platform and hands those two straight to the Cupertino version.
        // One rule with a Cupertino branch, not two rules.
        use crate::editable_text::TargetPlatform;
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert_eq!(
                ContextMenuButtonType::SelectAll.material_label(platform),
                ContextMenuButtonType::SelectAll.cupertino_label(),
                "{platform:?}"
            );
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert_ne!(
                ContextMenuButtonType::SelectAll.material_label(platform),
                ContextMenuButtonType::SelectAll.cupertino_label(),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_same_entry_says_a_different_thing_on_ios() {
        // Two differences, and both survive into every locale because the two
        // tables are translated separately -- neither is a typo something
        // downstream would catch.
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            ContextMenuButtonType::SelectAll.material_label(TargetPlatform::Android),
            "Select all"
        );
        assert_eq!(
            ContextMenuButtonType::SelectAll.cupertino_label(),
            "Select All"
        );

        assert_eq!(
            ContextMenuButtonType::Share.material_label(TargetPlatform::Android),
            "Share"
        );
        assert_eq!(
            ContextMenuButtonType::Share.cupertino_label(),
            "Share...",
            "on iOS sharing opens a sheet, and the dots promise it will"
        );

        // The five that do agree, so the two above are a disagreement rather
        // than two unrelated tables.
        for button in [
            ContextMenuButtonType::Cut,
            ContextMenuButtonType::Copy,
            ContextMenuButtonType::Paste,
            ContextMenuButtonType::LookUp,
            ContextMenuButtonType::SearchWeb,
        ] {
            assert_eq!(
                button.material_label(TargetPlatform::Android),
                button.cupertino_label(),
                "{button:?}"
            );
        }
    }

    #[test]
    fn two_entries_do_not_exist_on_ios_and_answer_with_nothing() {
        // A Cupertino text menu has no delete and no scan-text entry, so
        // there is no string for them -- and upstream answers `''` rather
        // than inventing one.
        use crate::editable_text::TargetPlatform;
        for button in [
            ContextMenuButtonType::Delete,
            ContextMenuButtonType::LiveTextInput,
        ] {
            assert_eq!(button.cupertino_label(), "", "{button:?}");
            assert_ne!(
                button.material_label(TargetPlatform::Android),
                "",
                "{button:?} does exist on Android"
            );
        }
    }

    #[test]
    fn the_delete_entry_borrows_the_tooltip_and_shouts_it() {
        // The only label in either table that is *derived* rather than looked
        // up, which is why it cannot be a tenth constant. The delete entry has
        // no label string of its own, so the menu takes the tooltip and
        // upper-cases it to match the other entries.
        use crate::editable_text::TargetPlatform;
        let shouted = ContextMenuButtonType::Delete.material_label(TargetPlatform::Android);
        assert_eq!(shouted, "DELETE");
        assert_eq!(
            shouted,
            crate::material_app::DefaultMaterialLocalizations::DELETE_BUTTON_TOOLTIP.to_uppercase()
        );
        assert_ne!(
            shouted,
            crate::material_app::DefaultMaterialLocalizations::DELETE_BUTTON_TOOLTIP,
            "and it is not the tooltip as written"
        );
    }

    #[test]
    fn scan_text_names_the_camera_and_not_the_menu_entry() {
        // `liveTextInput` is the entry; "Scan text" is what it does. Worth
        // saying because the constant's name and the entry's name do not
        // match, and the next reader will look for a `liveTextInputLabel`.
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            ContextMenuButtonType::LiveTextInput.material_label(TargetPlatform::Android),
            "Scan text"
        );
    }

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
