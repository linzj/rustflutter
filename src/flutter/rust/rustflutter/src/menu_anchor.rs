//! Menus anchored to a widget, and the underlined letter in their labels --
//! a port of upstream's `material/menu_anchor.dart`.
//!
//! The piece with the most judgement in it is the smallest:
//! [`MenuAcceleratorLabel::strip_accelerator_markers`], which turns
//! `"&Save As..."` into `"Save As..."` and says the S is the accelerator. It
//! has to answer several questions a first attempt would not think to ask --
//! what `&&` means, what `& ` means, what a trailing `&` means, and what the
//! index refers to once the markers have been taken out.
//!
//! ## What is not here
//!
//! [`MenuAnchor`] and [`SubmenuButton`] put their menus in an `OverlayPortal`
//! and drive them with a route-aware controller; this crate has neither. What
//! is ported is the configuration those widgets carry and the accelerator
//! machinery, which is self-contained.

use crate::render::Offset;

/// Upstream `MenuAcceleratorCallbackBinding`: how a label tells the button
/// above it that its letter was pressed.
///
/// The `has_submenu` flag rides along because a menu **item** and a menu that
/// *opens* another menu do different things when their letter is pressed: the
/// first is invoked and the menu closes, the second opens its submenu and
/// stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MenuAcceleratorCallbackBinding {
    pub has_on_invoke: bool,
    pub has_submenu: bool,
}

impl MenuAcceleratorCallbackBinding {
    pub fn new(has_on_invoke: bool, has_submenu: bool) -> MenuAcceleratorCallbackBinding {
        MenuAcceleratorCallbackBinding {
            has_on_invoke,
            has_submenu,
        }
    }

    /// Upstream's `updateShouldNotify`.
    pub fn update_should_notify(&self, old: &MenuAcceleratorCallbackBinding) -> bool {
        self.has_on_invoke != old.has_on_invoke || self.has_submenu != old.has_submenu
    }
}

/// Upstream `MenuAcceleratorLabel`: a label with one letter marked as its
/// keyboard accelerator.
pub struct MenuAcceleratorLabel {
    /// The label as written, markers and all.
    pub label: String,
}

impl MenuAcceleratorLabel {
    pub fn new(label: impl Into<String>) -> MenuAcceleratorLabel {
        MenuAcceleratorLabel {
            label: label.into(),
        }
    }

    /// Upstream's `displayLabel`: what a reader sees.
    pub fn display_label(&self) -> String {
        Self::strip_accelerator_markers(&self.label).0
    }

    /// Upstream's `hasAccelerator`, whose regular expression is
    /// `&(?!([&\s]|$))` -- an ampersand **not** followed by another ampersand,
    /// by whitespace, or by the end of the string. All three exclusions are
    /// the same idea from different directions: those are the ampersands that
    /// mean a literal ampersand rather than a marker.
    ///
    /// **Derived from the stripping here rather than written twice.** Upstream
    /// has a regular expression and a loop that must agree about the same
    /// rule, and they very nearly do not: the regex matches `&x` anywhere,
    /// while the loop only sets an index for the *first* eligible marker and
    /// skips the character after any marker. For every label the two agree,
    /// because a second marker being ineligible does not stop the first from
    /// being found -- but keeping one implementation removes the question.
    pub fn has_accelerator(&self) -> bool {
        Self::strip_accelerator_markers(&self.label).1.is_some()
    }

    /// Upstream's `stripAcceleratorMarkers`.
    ///
    /// Returns the label to show and the index, **into the stripped string**,
    /// of the accelerator character. The rules, each of which upstream's
    /// implementation earns a comment for:
    ///
    /// * `&&` is a literal ampersand and does **not** mark an accelerator, so
    ///   a label like `"Search && Replace"` shows one ampersand and has none.
    /// * `&` before whitespace marks nothing either -- there is no letter
    ///   there to underline.
    /// * a bare `&` at the very end is **stripped**, not shown. Upstream's
    ///   comment calls it "just treated as a quoted ampersand", but the code
    ///   breaks out of the loop without writing it, so it disappears. Ported as
    ///   written; see the regression line.
    /// * only the **first** eligible marker counts. A second `&Letter` is
    ///   stripped like the first but does not move the index.
    /// * and the index is reduced by the number of quoted ampersands seen
    ///   before it, because it has to index the *stripped* string rather than
    ///   the original.
    pub fn strip_accelerator_markers(label: &str) -> (String, Option<usize>) {
        let characters: Vec<char> = label.chars().collect();
        let mut display = String::new();
        let mut accelerator_index: Option<usize> = None;
        let mut quoted_ampersands = 0usize;
        let mut last_was_ampersand = false;

        for (index, character) in characters.iter().enumerate() {
            if last_was_ampersand {
                last_was_ampersand = false;
                display.push(*character);
                continue;
            }
            if *character != '&' {
                display.push(*character);
                continue;
            }
            if index == characters.len() - 1 {
                // A bare ampersand at the end is dropped.
                break;
            }
            last_was_ampersand = true;
            let next = characters[index + 1];
            if accelerator_index.is_none() && next != '&' && !next.is_whitespace() {
                accelerator_index = Some(index - quoted_ampersands);
            }
            quoted_ampersands += 1;
        }
        (display, accelerator_index)
    }
}

/// Upstream `MenuStyle`'s alignment offset and the flags a menu carries --
/// the configuration half of [`MenuAnchor`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuAnchor {
    /// Upstream's `alignmentOffset`.
    pub alignment_offset: Offset,
    /// Upstream's `consumeOutsideTap`.
    ///
    /// Whether a tap that closes the menu is also delivered to whatever was
    /// under it. False by default, and the default is the considered one: a
    /// reader dismissing a menu by tapping a button usually means only to
    /// dismiss it.
    pub consume_outside_tap: bool,
    /// Upstream's deprecated `anchorTapClosesMenu`.
    ///
    /// Kept because upstream kept it. The deprecation notice points at
    /// `consumeOutsideTap`, which answers a wider question -- this one was
    /// only ever about a tap on the anchor itself.
    pub anchor_tap_closes_menu: bool,
    /// Upstream's `crossAxisUnconstrained`, true by default: a submenu is
    /// allowed to be wider than the space beside its parent, because a menu
    /// item wrapped onto two lines is worse than one that overhangs.
    pub cross_axis_unconstrained: bool,
    /// Upstream's `useRootOverlay`.
    pub use_root_overlay: bool,
    /// Upstream's `animated`.
    pub animated: bool,
}

impl Default for MenuAnchor {
    fn default() -> MenuAnchor {
        MenuAnchor::new()
    }
}

impl MenuAnchor {
    pub fn new() -> MenuAnchor {
        MenuAnchor {
            alignment_offset: Offset::ZERO,
            consume_outside_tap: false,
            anchor_tap_closes_menu: false,
            cross_axis_unconstrained: true,
            use_root_overlay: false,
            animated: false,
        }
    }

    pub fn with_alignment_offset(mut self, offset: Offset) -> Self {
        self.alignment_offset = offset;
        self
    }

    pub fn with_consume_outside_tap(mut self, consume: bool) -> Self {
        self.consume_outside_tap = consume;
        self
    }

    pub fn with_cross_axis_unconstrained(mut self, unconstrained: bool) -> Self {
        self.cross_axis_unconstrained = unconstrained;
        self
    }

    pub fn with_animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }
}

/// Upstream `MenuBar`: a row of menus along the top of a window.
///
/// Upstream's `clipBehavior` defaults to `Clip.none` here where
/// [`MenuAnchor`]'s defaults to `hardEdge`, and the difference is the point: a
/// bar's menus are *meant* to hang below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MenuBar {
    pub clip: bool,
}

impl MenuBar {
    pub fn new() -> MenuBar {
        MenuBar { clip: false }
    }

    pub fn with_clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }
}

/// Upstream `MenuItemButton`: one line of a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MenuItemButton {
    /// Upstream's `requestFocusOnHover`.
    pub request_focus_on_hover: bool,
    /// Upstream's `closeOnActivate`, true by default: pressing an item is
    /// normally the end of the interaction.
    pub close_on_activate: bool,
    pub enabled: bool,
}

impl MenuItemButton {
    pub fn new() -> MenuItemButton {
        MenuItemButton {
            request_focus_on_hover: false,
            close_on_activate: true,
            enabled: true,
        }
    }

    pub fn with_close_on_activate(mut self, close: bool) -> Self {
        self.close_on_activate = close;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Upstream `CheckboxMenuButton`: a menu item with a checkbox in its leading
/// slot.
///
/// Its `value` is tri-state for the same reason a checkbox's is: with
/// `tristate` set, `None` is a real third value rather than a missing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CheckboxMenuButton {
    pub value: Option<bool>,
    pub tristate: bool,
    pub enabled: bool,
}

impl CheckboxMenuButton {
    pub fn new(value: Option<bool>) -> CheckboxMenuButton {
        CheckboxMenuButton {
            value,
            tristate: false,
            enabled: true,
        }
    }

    pub fn with_tristate(mut self, tristate: bool) -> Self {
        self.tristate = tristate;
        self
    }

    /// Upstream's `onChanged` cycle, which a checkbox menu item shares with
    /// [`crate::controls::Checkbox`]: false, true, and -- only when tristate
    /// -- null.
    pub fn next_value(&self) -> Option<bool> {
        match (self.value, self.tristate) {
            (Some(false), _) => Some(true),
            (Some(true), true) => None,
            (Some(true), false) => Some(false),
            (None, _) => Some(false),
        }
    }
}

/// Upstream `RadioMenuButton`: a menu item with a radio in its leading slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadioMenuButton<T> {
    pub value: T,
    pub group_value: Option<T>,
    /// Upstream's `toggleable`, false by default: a radio in a group is not
    /// normally allowed to be turned off by pressing it again, because the
    /// group is meant to have an answer.
    pub toggleable: bool,
    pub enabled: bool,
}

impl<T: PartialEq + Copy> RadioMenuButton<T> {
    pub fn new(value: T) -> RadioMenuButton<T> {
        RadioMenuButton {
            value,
            group_value: None,
            toggleable: false,
            enabled: true,
        }
    }

    pub fn with_group_value(mut self, group_value: T) -> Self {
        self.group_value = Some(group_value);
        self
    }

    pub fn with_toggleable(mut self, toggleable: bool) -> Self {
        self.toggleable = toggleable;
        self
    }

    pub fn is_selected(&self) -> bool {
        self.group_value == Some(self.value)
    }

    /// What pressing this radio sets the group to.
    ///
    /// Pressing the one already selected clears the group **only** when
    /// toggleable; otherwise it stays where it is, which is what keeps a
    /// required choice required.
    pub fn next_group_value(&self) -> Option<T> {
        if self.is_selected() && self.toggleable {
            None
        } else {
            Some(self.value)
        }
    }
}

/// Upstream `SubmenuButton`: a menu item that opens another menu.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SubmenuButton {
    pub alignment_offset: Offset,
    /// Upstream's `submenuIcon` slot being present at all is what makes a
    /// submenu look different from an item.
    pub has_submenu_icon: bool,
    pub enabled: bool,
}

impl SubmenuButton {
    pub fn new() -> SubmenuButton {
        SubmenuButton {
            alignment_offset: Offset::ZERO,
            has_submenu_icon: true,
            enabled: true,
        }
    }

    pub fn with_alignment_offset(mut self, offset: Offset) -> Self {
        self.alignment_offset = offset;
        self
    }

    /// The binding a submenu publishes to its label: it has a submenu, so its
    /// accelerator opens rather than invokes.
    pub fn accelerator_binding(&self) -> MenuAcceleratorCallbackBinding {
        MenuAcceleratorCallbackBinding::new(self.enabled, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stripped(label: &str) -> (String, Option<usize>) {
        MenuAcceleratorLabel::strip_accelerator_markers(label)
    }

    #[test]
    fn the_marker_is_taken_out_and_the_letter_after_it_is_the_accelerator() {
        assert_eq!(stripped("&Save"), ("Save".to_string(), Some(0)));
        assert_eq!(stripped("Save &As..."), ("Save As...".to_string(), Some(5)));
        assert_eq!(stripped("Save"), ("Save".to_string(), None));
    }

    #[test]
    fn a_doubled_ampersand_is_a_literal_one_and_marks_nothing() {
        // Which is what a label like "Search && Replace" needs: one ampersand
        // on screen and no underlined letter.
        assert_eq!(
            stripped("Search && Replace"),
            ("Search & Replace".to_string(), None)
        );
        assert!(!MenuAcceleratorLabel::new("Search && Replace").has_accelerator());
    }

    #[test]
    fn an_ampersand_before_a_space_marks_nothing_either() {
        // There is no letter there to underline.
        let (display, index) = stripped("Save & Quit");
        assert_eq!(display, "Save  Quit", "the marker still comes out");
        assert_eq!(index, None);
    }

    #[test]
    fn a_bare_ampersand_at_the_very_end_disappears() {
        // Upstream's comment calls it "just treated as a quoted ampersand",
        // but the code breaks out of the loop without writing it, so it is
        // dropped rather than shown. Ported as written, and pinned here so the
        // disagreement between the comment and the code is not mistaken for a
        // porting slip.
        assert_eq!(stripped("Save&"), ("Save".to_string(), None));
        assert_eq!(stripped("&"), (String::new(), None));
    }

    #[test]
    fn only_the_first_eligible_marker_counts() {
        // A second &Letter is stripped like the first but does not move the
        // index -- a label has one accelerator or none.
        let (display, index) = stripped("&Save &As");
        assert_eq!(display, "Save As");
        assert_eq!(index, Some(0), "the S, not the A");
    }

    #[test]
    fn the_index_is_into_the_stripped_string_and_not_the_original() {
        // Every quoted ampersand before the marker shifts it, and the index is
        // reduced to match. Getting this wrong underlines the wrong letter,
        // and only in labels that also contain a literal ampersand.
        let (display, index) = stripped("A && B &Cut");
        assert_eq!(display, "A & B Cut");
        let index = index.expect("there is one");
        assert_eq!(
            display.chars().nth(index),
            Some('C'),
            "index {index} into {display:?}"
        );
    }

    #[test]
    fn the_index_still_lands_on_the_right_letter_with_several_quoted_ampersands() {
        let (display, index) = stripped("&& && &X");
        assert_eq!(display, "& & X");
        let index = index.expect("there is one");
        assert_eq!(display.chars().nth(index), Some('X'));
    }

    #[test]
    fn a_label_has_an_accelerator_exactly_when_a_marker_survives_the_rules() {
        // Upstream asks this with a regular expression while stripping with a
        // loop; this port derives it from the loop, so what is worth pinning
        // is the *answer* for each shape of label rather than an agreement
        // between two implementations.
        for (label, expected) in [
            ("&Save", true),
            ("Save &As", true),
            ("Save", false),
            ("Search && Replace", false),
            ("Save & Quit", false),
            ("Save&", false),
            ("&", false),
            ("&& &X", true),
        ] {
            assert_eq!(
                MenuAcceleratorLabel::new(label).has_accelerator(),
                expected,
                "{label:?}"
            );
        }
    }

    #[test]
    fn the_display_label_is_what_a_reader_sees() {
        let label = MenuAcceleratorLabel::new("&Open Recent");
        assert_eq!(label.display_label(), "Open Recent");
        assert_eq!(label.label, "&Open Recent", "and the original is kept");
        assert!(label.has_accelerator());
    }

    #[test]
    fn a_marker_on_a_multi_byte_letter_still_indexes_by_character() {
        // Upstream uses `characters` so as not to split a surrogate pair. The
        // same care in Rust means indexing by char rather than by byte.
        let (display, index) = stripped("Ré&sumé");
        assert_eq!(display, "Résumé");
        let index = index.expect("there is one");
        assert_eq!(display.chars().nth(index), Some('s'));
    }

    #[test]
    fn a_submenu_and_an_item_do_different_things_with_their_letter() {
        // Which is why the binding carries hasSubmenu: the first is invoked
        // and the menu closes, the second opens its submenu and stays.
        let item = MenuAcceleratorCallbackBinding::new(true, false);
        let submenu = SubmenuButton::new().accelerator_binding();
        assert!(!item.has_submenu);
        assert!(submenu.has_submenu);
        assert!(submenu.has_on_invoke);
        assert!(item.update_should_notify(&submenu));
    }

    #[test]
    fn the_binding_notifies_only_on_a_real_change() {
        let binding = MenuAcceleratorCallbackBinding::new(true, false);
        assert!(!binding.update_should_notify(&MenuAcceleratorCallbackBinding::new(true, false)));
        assert!(binding.update_should_notify(&MenuAcceleratorCallbackBinding::new(false, false)));
        assert!(binding.update_should_notify(&MenuAcceleratorCallbackBinding::new(true, true)));
    }

    #[test]
    fn a_bars_menus_are_meant_to_hang_below_it() {
        // Upstream's MenuBar defaults to no clipping where MenuAnchor defaults
        // to hardEdge, and the difference is the whole point.
        assert!(!MenuBar::new().clip);
        assert!(MenuBar::new().with_clip(true).clip);
    }

    #[test]
    fn a_menu_item_closes_the_menu_when_pressed_unless_told_not_to() {
        // Pressing an item is normally the end of the interaction.
        assert!(MenuItemButton::new().close_on_activate);
        assert!(
            !MenuItemButton::new()
                .with_close_on_activate(false)
                .close_on_activate
        );
        assert!(MenuItemButton::new().enabled);
    }

    #[test]
    fn a_submenu_is_allowed_to_be_wider_than_the_space_beside_its_parent() {
        // A menu item wrapped onto two lines is worse than one that overhangs.
        assert!(MenuAnchor::new().cross_axis_unconstrained);
        assert!(
            !MenuAnchor::new()
                .with_cross_axis_unconstrained(false)
                .cross_axis_unconstrained
        );

        // And a tap that dismisses a menu does not by default also press what
        // was under it.
        assert!(!MenuAnchor::new().consume_outside_tap);
        assert!(
            MenuAnchor::new()
                .with_consume_outside_tap(true)
                .consume_outside_tap
        );
        assert_eq!(MenuAnchor::new().alignment_offset, Offset::ZERO);
    }

    #[test]
    fn a_checkbox_menu_item_cycles_the_way_a_checkbox_does() {
        let plain = CheckboxMenuButton::new(Some(false));
        assert_eq!(plain.next_value(), Some(true));
        assert_eq!(
            CheckboxMenuButton::new(Some(true)).next_value(),
            Some(false),
            "two states without tristate"
        );

        let tri = CheckboxMenuButton::new(Some(true)).with_tristate(true);
        assert_eq!(tri.next_value(), None, "and a real third state with it");
        assert_eq!(
            CheckboxMenuButton::new(None)
                .with_tristate(true)
                .next_value(),
            Some(false)
        );
    }

    #[test]
    fn a_radio_in_a_group_cannot_normally_be_turned_off_by_pressing_it_again() {
        // The group is meant to have an answer, which is what keeps a required
        // choice required.
        let selected = RadioMenuButton::new(2).with_group_value(2);
        assert!(selected.is_selected());
        assert_eq!(selected.next_group_value(), Some(2), "it stays");

        let toggleable = RadioMenuButton::new(2)
            .with_group_value(2)
            .with_toggleable(true);
        assert_eq!(toggleable.next_group_value(), None, "unless told otherwise");

        // Pressing one that is not selected always selects it.
        let other = RadioMenuButton::new(3).with_group_value(2);
        assert!(!other.is_selected());
        assert_eq!(other.next_group_value(), Some(3));
        assert_eq!(
            RadioMenuButton::new(3)
                .with_group_value(2)
                .with_toggleable(true)
                .next_group_value(),
            Some(3),
            "toggleable or not"
        );
    }
}
