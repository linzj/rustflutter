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

    /// This anchor's panel, resolved. An anchored menu is the vertical case,
    /// which is what makes `MenuTheme` the one consulted.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        style: Option<&crate::component_themes::MenuStyle>,
    ) -> crate::component_themes::ResolvedMenuPanel {
        crate::component_themes::ResolvedMenuPanel::of(
            context,
            crate::component_themes::MenuPanelAxis::Vertical,
            style,
        )
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

    /// This bar's panel, resolved. A bar is the horizontal case, which is what
    /// makes `MenuBarTheme` the one consulted -- see
    /// [`crate::component_themes::ResolvedMenuPanel`].
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        style: Option<&crate::component_themes::MenuStyle>,
    ) -> crate::component_themes::ResolvedMenuPanel {
        crate::component_themes::ResolvedMenuPanel::of(
            context,
            crate::component_themes::MenuPanelAxis::Horizontal,
            style,
        )
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
    /// This line's appearance, with `MenuButtonTheme` and the M3 defaults
    /// folded in -- see [`crate::component_themes::ResolvedMenuButton`].
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        states: crate::widget_state::WidgetStates,
    ) -> crate::component_themes::ResolvedMenuButton {
        crate::component_themes::ResolvedMenuButton::of(context, states)
    }

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
    /// This line's appearance, with `MenuButtonTheme` and the M3 defaults
    /// folded in -- see [`crate::component_themes::ResolvedMenuButton`].
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        states: crate::widget_state::WidgetStates,
    ) -> crate::component_themes::ResolvedMenuButton {
        crate::component_themes::ResolvedMenuButton::of(context, states)
    }

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

#[cfg(test)]
mod menu_theme_tests {
    use super::*;
    use crate::component_themes::{
        ButtonStyle, MenuBarTheme, MenuBarThemeData, MenuButtonTheme, MenuButtonThemeData,
        MenuPanelAxis, MenuStyle, MenuTheme, MenuThemeData, ResolvedMenuButton, ResolvedMenuPanel,
    };
    use crate::engine::Color;
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, leaf};
    use crate::render::{AlignmentDirectional, AlignmentGeometry, EdgeInsets, Size};
    use crate::theme::ThemeData;
    use crate::widget_state::{StateProperty, WidgetState, WidgetStates};
    use crate::widgets::SizedBox;

    struct Reader<T> {
        read: std::rc::Rc<dyn Fn(&mut BuildContext) -> T>,
        seen: std::rc::Rc<std::cell::RefCell<Option<T>>>,
    }

    impl<T: 'static> Component for Reader<T> {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some((self.read)(context));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn read_under<T: 'static>(
        wrap: impl FnOnce(AnyWidget) -> AnyWidget,
        read: impl Fn(&mut BuildContext) -> T + 'static,
    ) -> T {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(wrap(component(Reader {
            read: std::rc::Rc::new(read),
            seen: std::rc::Rc::clone(&seen),
        })));
        seen.borrow_mut().take().expect("built once")
    }

    fn panel(axis: MenuPanelAxis) -> ResolvedMenuPanel {
        read_under(
            |child| child,
            move |context| ResolvedMenuPanel::of(context, axis, None),
        )
    }

    // -- The axis picks the theme ----------------------------------------------

    #[test]
    fn a_bar_theme_moves_the_horizontal_panel_and_not_the_vertical_one() {
        // Upstream switches on the orientation, so it never consults the theme
        // it is not using. Both wrapped at once, disagreeing, so the switch is
        // what decides and not which one happens to be present.
        let bar_colour = Color(0xFF110000);
        let menu_colour = Color(0xFF001100);
        let mut bar = MenuStyle::new();
        bar.background_color = Some(StateProperty::all(Some(bar_colour)));
        let mut menu = MenuStyle::new();
        menu.background_color = Some(StateProperty::all(Some(menu_colour)));

        let wrap = move |child: AnyWidget| {
            MenuBarTheme::new(
                MenuBarThemeData {
                    style: Some(bar.clone()),
                },
                MenuTheme::new(
                    MenuThemeData {
                        style: Some(menu.clone()),
                    },
                    child,
                ),
            )
        };
        assert_eq!(
            read_under(wrap.clone(), |context| ResolvedMenuPanel::of(
                context,
                MenuPanelAxis::Horizontal,
                None
            ))
            .background_color,
            Some(bar_colour)
        );
        assert_eq!(
            read_under(wrap, |context| ResolvedMenuPanel::of(
                context,
                MenuPanelAxis::Vertical,
                None
            ))
            .background_color,
            Some(menu_colour)
        );
    }

    #[test]
    fn the_two_defaults_differ_in_exactly_two_fields() {
        // The claim the type's docs make, checked field by field rather than
        // asserted in prose.
        let bar = panel(MenuPanelAxis::Horizontal);
        let menu = panel(MenuPanelAxis::Vertical);

        assert_eq!(bar.background_color, menu.background_color);
        assert_eq!(bar.shadow_color, menu.shadow_color);
        assert_eq!(bar.surface_tint_color, menu.surface_tint_color);
        assert_eq!(bar.elevation, menu.elevation);
        assert_eq!(bar.shape, menu.shape);
        assert_eq!(bar.visual_density, menu.visual_density);
        assert_eq!(bar.minimum_size, menu.minimum_size);
        assert_eq!(bar.fixed_size, menu.fixed_size);
        assert_eq!(bar.maximum_size, menu.maximum_size);
        assert_eq!(bar.side, menu.side);

        assert_ne!(bar.alignment, menu.alignment);
        assert_ne!(bar.padding, menu.padding);
    }

    #[test]
    fn and_both_differences_are_the_axis() {
        // A row is padded at the ends of a row; a column at the ends of a
        // column. A bar's submenu drops below it; a menu's flies out beside it.
        let bar = panel(MenuPanelAxis::Horizontal);
        let menu = panel(MenuPanelAxis::Vertical);

        assert_eq!(bar.padding, EdgeInsets::symmetric(4.0, 0.0));
        assert_eq!(bar.padding.top, 0.0, "a bar is not padded across its run");
        assert_eq!(menu.padding, EdgeInsets::symmetric(0.0, 8.0));
        assert_eq!(menu.padding.left, 0.0, "nor is a menu");

        assert_eq!(
            bar.alignment,
            AlignmentGeometry::Directional(AlignmentDirectional::BOTTOM_START)
        );
        assert_eq!(
            menu.alignment,
            AlignmentGeometry::Directional(AlignmentDirectional::TOP_END)
        );
    }

    #[test]
    fn a_panel_is_asked_as_though_nothing_were_happening() {
        // Upstream resolves with `<WidgetState>{}` unconditionally. A panel is
        // a surface: it is not hovered, its items are.
        let resting = Color(0xFF010101);
        let hovered = Color(0xFF020202);
        let mut style = MenuStyle::new();
        style.background_color = Some(StateProperty::resolve_with(move |states| {
            Some(if states.contains(WidgetState::Hovered) {
                hovered
            } else {
                resting
            })
        }));
        let resolved = read_under(
            move |child| {
                MenuTheme::new(
                    MenuThemeData {
                        style: Some(style.clone()),
                    },
                    child,
                )
            },
            |context| ResolvedMenuPanel::of(context, MenuPanelAxis::Vertical, None),
        );
        assert_eq!(resolved.background_color, Some(resting));
        assert_ne!(resolved.background_color, Some(hovered));
    }

    #[test]
    fn the_zero_after_the_elevation_chain_cannot_be_reached() {
        // `resolve(...elevation) ?? 0` is a fourth step the chain never falls
        // out of: the defaults supply 3, and a style whose elevation resolves
        // to null falls through to that rather than past it.
        let mut style = MenuStyle::new();
        style.elevation = Some(StateProperty::all(None));
        for axis in [MenuPanelAxis::Horizontal, MenuPanelAxis::Vertical] {
            let resolved = read_under(
                {
                    let style = style.clone();
                    move |child| MenuTheme::new(MenuThemeData { style: Some(style) }, child)
                },
                move |context| ResolvedMenuPanel::of(context, axis, None),
            );
            assert_eq!(resolved.elevation, ResolvedMenuPanel::ELEVATION);
            assert_ne!(resolved.elevation, ResolvedMenuPanel::UNREACHABLE_ELEVATION);
        }
    }

    #[test]
    fn the_widget_is_the_first_step_and_the_theme_the_second() {
        let mine = Color(0xFF123456);
        let theirs = Color(0xFF654321);
        let mut widget = MenuStyle::new();
        widget.background_color = Some(StateProperty::all(Some(mine)));
        let mut themed = MenuStyle::new();
        themed.background_color = Some(StateProperty::all(Some(theirs)));

        let wrap = move |child: AnyWidget| {
            MenuTheme::new(
                MenuThemeData {
                    style: Some(themed.clone()),
                },
                child,
            )
        };
        assert_eq!(
            read_under(wrap.clone(), move |context| {
                ResolvedMenuPanel::of(context, MenuPanelAxis::Vertical, Some(&widget))
            })
            .background_color,
            Some(mine)
        );
        assert_eq!(
            read_under(wrap, |context| ResolvedMenuPanel::of(
                context,
                MenuPanelAxis::Vertical,
                None
            ))
            .background_color,
            Some(theirs)
        );
    }

    // -- One line of a menu ----------------------------------------------------

    fn line(states: WidgetStates) -> ResolvedMenuButton {
        read_under(
            |child| child,
            move |context| ResolvedMenuButton::of(context, states),
        )
    }

    fn states(list: &[WidgetState]) -> WidgetStates {
        WidgetStates::of(list)
    }

    #[test]
    fn neither_the_label_nor_the_icon_reacts_to_anything_but_being_disabled() {
        // Four arms upstream, all returning the same colour. A menu line that
        // recoloured its text would flicker as the pointer crossed it.
        let resting = line(WidgetStates::NONE);
        for interaction in [
            states(&[WidgetState::Pressed]),
            states(&[WidgetState::Hovered]),
            states(&[WidgetState::Focused]),
            states(&[WidgetState::Hovered, WidgetState::Focused]),
        ] {
            let touched = line(interaction);
            assert_eq!(touched.foreground, resting.foreground);
            assert_eq!(touched.icon_color, resting.icon_color);
        }

        let off = line(states(&[WidgetState::Disabled]));
        assert_ne!(off.foreground, resting.foreground);
        assert_ne!(off.icon_color, resting.icon_color);
    }

    #[test]
    fn the_overlay_is_the_whole_of_the_feedback() {
        // And it does move -- otherwise the test above would only prove that
        // nothing anywhere reacts.
        let resting = line(WidgetStates::NONE);
        assert_eq!(resting.overlay, Color::TRANSPARENT);

        let scheme = ThemeData::fallback().color_scheme;
        let pressed = line(states(&[WidgetState::Pressed]));
        let hovered = line(states(&[WidgetState::Hovered]));
        let focused = line(states(&[WidgetState::Focused]));

        assert_eq!(
            pressed.overlay,
            crate::elevation_overlay::with_opacity(scheme.on_surface, 0.1)
        );
        assert_eq!(
            hovered.overlay,
            crate::elevation_overlay::with_opacity(scheme.on_surface, 0.08)
        );
        assert_ne!(
            hovered.overlay, pressed.overlay,
            "hovering is the lighter one"
        );
        assert_eq!(
            focused.overlay, pressed.overlay,
            "pressed and focused agree; only hovering is weaker"
        );
    }

    #[test]
    fn pressing_beats_hovering_when_both_are_true() {
        // The order of the arms, which is only visible where the values differ
        // -- and a pointer that presses is always also hovering.
        let both = line(states(&[WidgetState::Pressed, WidgetState::Hovered]));
        assert_eq!(both.overlay, line(states(&[WidgetState::Pressed])).overlay);
        assert_ne!(both.overlay, line(states(&[WidgetState::Hovered])).overlay);
    }

    #[test]
    fn hovering_beats_being_focused_when_both_are_true() {
        // The other order in the ladder. Pressed and focused agree, so this is
        // the only pair below the top that a swap could show.
        let both = line(states(&[WidgetState::Hovered, WidgetState::Focused]));
        assert_eq!(both.overlay, line(states(&[WidgetState::Hovered])).overlay);
        assert_ne!(both.overlay, line(states(&[WidgetState::Focused])).overlay);
    }

    #[test]
    fn the_label_is_stronger_than_the_icon() {
        let scheme = ThemeData::fallback().color_scheme;
        let resting = line(WidgetStates::NONE);
        assert_eq!(resting.foreground, scheme.on_surface);
        assert_eq!(resting.icon_color, scheme.on_surface_variant());
        assert_ne!(resting.foreground, resting.icon_color);
    }

    #[test]
    fn a_line_paints_no_background_of_its_own() {
        // It sits on the panel's; painting one would draw the panel twice.
        let resting = line(WidgetStates::NONE);
        assert_eq!(resting.background, Color::TRANSPARENT);
        assert_eq!(resting.elevation, 0.0);
        assert_eq!(resting.minimum_size, Size::new(64.0, 48.0));
        assert_eq!(resting.icon_size, 24.0);
    }

    #[test]
    fn both_kinds_of_line_read_the_one_theme() {
        // `MenuItemButton` and `SubmenuButton` share `MenuButtonTheme` and
        // `_MenuButtonDefaultsM3` -- two widgets, one theme, the mirror of the
        // panel's one widget and two themes.
        let mine = Color(0xFF00FFFF);
        let mut style = ButtonStyle::new();
        style.foreground_color = Some(StateProperty::all(Some(mine)));
        let data = MenuButtonThemeData { style: Some(style) };

        let item = read_under(
            {
                let data = data.clone();
                move |child| MenuButtonTheme::new(data, child)
            },
            |context| MenuItemButton::new().resolved(context, WidgetStates::NONE),
        );
        let submenu = read_under(
            move |child| MenuButtonTheme::new(data, child),
            |context| SubmenuButton::new().resolved(context, WidgetStates::NONE),
        );
        assert_eq!(item.foreground, mine);
        assert_eq!(submenu.foreground, mine);
    }
}
