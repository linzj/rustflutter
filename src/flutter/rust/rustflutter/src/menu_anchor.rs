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

// -- Opening and closing, and the three ways to read the same four states -----

/// Upstream `_MenuAnchorState.isClosing`: the menu is **running its close
/// animation right now**.
///
/// Only `reverse`. A menu that has finished closing is not closing -- it is
/// closed, and that is a different answer to a different question. See
/// [`is_closing_or_closed`] for the other one.
pub fn is_closing(status: crate::animation::AnimationStatus) -> bool {
    status == crate::animation::AnimationStatus::Reverse
}

/// Upstream `_MenuAnchorState.isClosingOrClosed`: `dismissed` or `reverse`.
///
/// This is exactly the complement of
/// [`crate::animation::AnimationStatus::is_forward_or_completed`], written out
/// upstream as its own switch because the menu code reads better asking it
/// this way round.
pub fn is_closing_or_closed(status: crate::animation::AnimationStatus) -> bool {
    !status.is_forward_or_completed()
}

/// What an open request does, from upstream's `_handleMenuOpenRequest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuOpenRequest {
    /// Whether the overlay is put up. Upstream calls `showOverlay()`
    /// **before** looking at the animation at all.
    pub shows_overlay: bool,
    /// Whether the open animation is started. Skipped for a menu already open
    /// or already opening.
    pub starts_animation: bool,
}

/// Upstream's `_handleMenuOpenRequest`.
///
/// # A parent that is *closing* blocks the child; a parent that is *closed*
/// does not
///
/// The guard is `_parent?.isClosing ?? false`, and it is the narrow predicate
/// -- `reverse` alone, not [`is_closing_or_closed`]. That reads backwards for
/// a moment: surely a parent that is entirely shut is worse than one still
/// half on screen? But the comment says what it is for -- "if this menu's
/// parent is closing, submenus should not open. This prevents a submenu
/// calling `MenuController.open()` after a parent menu has started closing."
/// It is a **race**, not a state check. A closing parent is on its way to
/// taking the child down with it, so a child opening now would flash and
/// vanish. A dismissed parent is just a menu, and whatever is opening the
/// child will open it too.
///
/// # The overlay goes up even when the animation does not
///
/// `showOverlay()` runs unconditionally, then the animation is skipped for a
/// menu that is already forward or completed. Folding the two together --
/// returning early before showing the overlay -- would be the natural
/// simplification and would lose the case where the entry was taken down
/// while the animation stayed at its end.
///
/// # A closing menu re-opens rather than counting as open
///
/// `reverse` is not forward-or-completed, so a menu caught mid-close is sent
/// `forward()` from wherever it got to. Asking "is it visible?" instead would
/// have said yes and left it closing.
pub fn menu_open_request(
    parent_status: Option<crate::animation::AnimationStatus>,
    status: crate::animation::AnimationStatus,
) -> MenuOpenRequest {
    if parent_status.is_some_and(is_closing) {
        return MenuOpenRequest {
            shows_overlay: false,
            starts_animation: false,
        };
    }
    MenuOpenRequest {
        shows_overlay: true,
        starts_animation: !status.is_forward_or_completed(),
    }
}

/// Upstream's `_handleMenuCloseRequest`: whether to run the close animation
/// and, when it finishes, take the overlay down.
///
/// The mirror of the open guard, and the mirror matters. A menu **already
/// closing** is left alone: restarting the reverse would jump it back to full
/// size, and `whenComplete(hideOverlay)` would be armed a second time. A menu
/// already **closed** is likewise left alone -- there is no overlay left to
/// hide, and reversing from zero animates nothing.
pub fn menu_close_request(status: crate::animation::AnimationStatus) -> bool {
    status.is_forward_or_completed()
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
    /// Upstream's `requestFocusOnHover`, **true** by default.
    ///
    /// This port had it false. A pointer moving down a menu carries the focus
    /// with it upstream, so the item under the cursor is the one a keyboard
    /// would act on -- and with the default inverted, moving the mouse and
    /// then pressing Enter acted on whatever the keyboard had left behind.
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
            request_focus_on_hover: true,
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

    /// The line this item lays out: upstream's `_MenuItemLabel` with
    /// `hasSubmenu: false`.
    ///
    /// `horizontal` is the anchor's orientation, which upstream reads off the
    /// enclosing anchor rather than off the item -- a line does not know
    /// whether it is in a bar until it is in one.
    pub fn label(
        &self,
        leading: bool,
        trailing: bool,
        shortcut: bool,
        horizontal: bool,
    ) -> MenuItemLabel {
        MenuItemLabel::new()
            .with_leading_icon(leading)
            .with_trailing_icon(trailing)
            .with_shortcut(shortcut)
            .in_a_horizontal_bar(horizontal)
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

    /// The line this submenu lays out: the same `_MenuItemLabel` an item
    /// builds, with `hasSubmenu: true`.
    ///
    /// So the arrow is a **trailing part like any other** and takes the same
    /// gap -- and in a horizontal bar it is suppressed with the shortcut,
    /// which is why a menu bar's top-level entries are bare words even though
    /// every one of them opens a submenu.
    pub fn label(
        &self,
        leading: bool,
        trailing: bool,
        shortcut: bool,
        horizontal: bool,
    ) -> MenuItemLabel {
        let mut label = MenuItemLabel::new()
            .with_leading_icon(leading)
            .with_trailing_icon(trailing)
            .with_shortcut(shortcut)
            .in_a_horizontal_bar(horizontal);
        label.has_submenu = self.has_submenu_icon;
        label
    }

    /// The binding a submenu publishes to its label: it has a submenu, so its
    /// accelerator opens rather than invokes.
    pub fn accelerator_binding(&self) -> MenuAcceleratorCallbackBinding {
        MenuAcceleratorCallbackBinding::new(self.enabled, true)
    }
}

/// How a menu line lays its parts out: upstream's `_MenuItemLabel`.
///
/// Kept apart from the buttons for the reason every other rule in this crate
/// is: it can be asked without building anything, and what it answers is a
/// number a test can hold. Both [`MenuItemButton`] and [`SubmenuButton`] build
/// one -- upstream's `_MenuItemLabel` is shared by them in exactly the same
/// way `_MenuButtonDefaultsM3` is.
///
/// # One spacing, and only where two things meet
///
/// Upstream computes a single `horizontalPadding` and spends it in four
/// places: before the label (**only when there is a leading icon**), before
/// the trailing icon, before the shortcut, and before the submenu arrow. There
/// is none at the outer edges -- the button's own padding does that -- so a
/// line with no leading icon starts its text exactly where a line with one
/// starts its icon, and a column of menu items has one left edge rather than
/// two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuItemLabel {
    pub has_leading_icon: bool,
    pub has_trailing_icon: bool,
    pub has_shortcut: bool,
    pub has_submenu: bool,
    /// Upstream's `showDecoration`, false for a line in a horizontal menu bar:
    /// a bar's items show neither their shortcut nor a submenu arrow, because
    /// a bar is a row of words and either would turn it into a table.
    pub show_decoration: bool,
}

impl MenuItemLabel {
    /// Upstream's `_kLabelItemDefaultSpacing`.
    pub const DEFAULT_SPACING: f32 = 12.0;
    /// Upstream's `_kLabelItemMinSpacing`, the floor a negative density cannot
    /// push through.
    pub const MIN_SPACING: f32 = 4.0;

    pub fn new() -> MenuItemLabel {
        MenuItemLabel {
            has_leading_icon: false,
            has_trailing_icon: false,
            has_shortcut: false,
            has_submenu: false,
            show_decoration: true,
        }
    }

    pub fn with_leading_icon(mut self, has: bool) -> Self {
        self.has_leading_icon = has;
        self
    }

    pub fn with_trailing_icon(mut self, has: bool) -> Self {
        self.has_trailing_icon = has;
        self
    }

    pub fn with_shortcut(mut self, has: bool) -> Self {
        self.has_shortcut = has;
        self
    }

    /// Upstream's `showDecoration`, which
    /// [`MenuItemLabel::in_a_horizontal_bar`] names the case for.
    pub fn with_decoration(mut self, show: bool) -> Self {
        self.show_decoration = show;
        self
    }

    /// A line in a menu bar: upstream passes `showDecoration: _orientation ==
    /// Axis.vertical`, so a horizontal bar suppresses both decorations.
    pub fn in_a_horizontal_bar(mut self, horizontal: bool) -> Self {
        self.show_decoration = !horizontal;
        self
    }

    /// Upstream's `horizontalPadding`:
    /// `math.max(_kLabelItemMinSpacing, _kLabelItemDefaultSpacing + density.horizontal * 2)`.
    ///
    /// **Twice the density**, not once. A denser menu closes the gaps between
    /// a line's parts at twice the rate the density itself moves, which is how
    /// a compact menu stays readable while getting smaller: the vertical
    /// squeeze comes from the button's minimum size and the horizontal one
    /// from here.
    ///
    /// The floor is the half worth stating. At the minimum density of -4 the
    /// arithmetic gives `12 - 8 = 4`, exactly the floor -- so the floor is not
    /// reachable from below by any legal density, and it is there to stop the
    /// gap going negative if either constant ever moves.
    pub fn spacing(density: crate::theme::VisualDensity) -> f32 {
        (MenuItemLabel::DEFAULT_SPACING + density.horizontal * 2.0).max(MenuItemLabel::MIN_SPACING)
    }

    /// The gap before the label, which exists only when something is in front
    /// of it.
    pub fn leading_gap(&self, density: crate::theme::VisualDensity) -> f32 {
        if self.has_leading_icon {
            MenuItemLabel::spacing(density)
        } else {
            0.0
        }
    }

    /// What follows the label, in the order upstream builds it: the trailing
    /// icon, then the shortcut, then the submenu arrow, each preceded by the
    /// same gap.
    ///
    /// The two decorations are **suppressed together** and the trailing icon is
    /// not: a caller who put an icon there asked for it, where the shortcut and
    /// the arrow are the menu's own furniture.
    pub fn trailing_parts(&self) -> Vec<MenuItemPart> {
        let mut parts = Vec::new();
        if self.has_trailing_icon {
            parts.push(MenuItemPart::TrailingIcon);
        }
        if self.show_decoration && self.has_shortcut {
            parts.push(MenuItemPart::Shortcut);
        }
        if self.show_decoration && self.has_submenu {
            parts.push(MenuItemPart::SubmenuIcon);
        }
        parts
    }

    /// How wide the line's gaps come to altogether: one before the label when
    /// there is a leading icon, and one before each trailing part.
    ///
    /// A line's own width is its parts plus this, which is what a menu panel
    /// needs in order to be as wide as its widest line.
    pub fn total_gaps(&self, density: crate::theme::VisualDensity) -> f32 {
        let spacing = MenuItemLabel::spacing(density);
        self.leading_gap(density) + spacing * self.trailing_parts().len() as f32
    }
}

impl Default for MenuItemLabel {
    fn default() -> MenuItemLabel {
        MenuItemLabel::new()
    }
}

/// One of the things that can sit after a menu line's label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItemPart {
    TrailingIcon,
    Shortcut,
    SubmenuIcon,
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The line's geometry ------------------------------------------------

    use crate::theme::VisualDensity;

    fn density(horizontal: f32) -> VisualDensity {
        VisualDensity {
            horizontal,
            vertical: 0.0,
        }
    }

    #[test]
    fn the_gap_moves_at_twice_the_density() {
        // `_kLabelItemDefaultSpacing + density.horizontal * 2`. Twice, not
        // once: the horizontal squeeze of a compact menu comes from here while
        // the vertical one comes from the button's minimum size, and a menu
        // that tightened at the same rate in both directions would run out of
        // room across long before it did down.
        assert_eq!(MenuItemLabel::spacing(density(0.0)), 12.0);
        assert_eq!(MenuItemLabel::spacing(density(1.0)), 14.0);
        assert_eq!(MenuItemLabel::spacing(density(-1.0)), 10.0);
    }

    #[test]
    fn the_gap_has_a_floor_that_the_densest_menu_lands_exactly_on() {
        // At the minimum density of -4 the arithmetic gives 12 - 8 = 4, which
        // is `_kLabelItemMinSpacing` to the pixel. So the floor is not
        // reachable from below by any legal density -- it is there to stop the
        // gap going negative if either constant moves, and the two numbers
        // being in that relationship is the fact worth pinning.
        assert_eq!(
            MenuItemLabel::spacing(density(VisualDensity::MINIMUM)),
            MenuItemLabel::MIN_SPACING
        );
        assert_eq!(
            MenuItemLabel::spacing(density(VisualDensity::MINIMUM - 1.0)),
            MenuItemLabel::MIN_SPACING,
            "and past it the floor holds"
        );
    }

    #[test]
    fn the_label_is_only_padded_when_something_is_in_front_of_it() {
        // The gap is between two things, not an inset. A line with no leading
        // icon starts its text exactly where a line with one starts its icon,
        // so a column of items has one left edge rather than two.
        let plain = MenuItemLabel::new();
        assert_eq!(plain.leading_gap(density(0.0)), 0.0);
        assert_eq!(
            plain.with_leading_icon(true).leading_gap(density(0.0)),
            MenuItemLabel::DEFAULT_SPACING
        );
    }

    #[test]
    fn what_follows_the_label_comes_in_upstreams_order() {
        let full = MenuItemLabel::new()
            .with_trailing_icon(true)
            .with_shortcut(true);
        let mut full = full;
        full.has_submenu = true;
        assert_eq!(
            full.trailing_parts(),
            vec![
                MenuItemPart::TrailingIcon,
                MenuItemPart::Shortcut,
                MenuItemPart::SubmenuIcon
            ]
        );
    }

    #[test]
    fn a_bar_hides_the_shortcut_and_the_arrow_and_keeps_the_icon() {
        // `showDecoration: _orientation == Axis.vertical`. The two decorations
        // go together because both are the menu's own furniture; a trailing
        // icon is the caller's and stays.
        let mut line = MenuItemLabel::new()
            .with_trailing_icon(true)
            .with_shortcut(true);
        line.has_submenu = true;

        assert_eq!(line.in_a_horizontal_bar(false).trailing_parts().len(), 3);
        assert_eq!(
            line.in_a_horizontal_bar(true).trailing_parts(),
            vec![MenuItemPart::TrailingIcon],
            "the icon stays and the furniture goes"
        );
    }

    #[test]
    fn the_gaps_add_up_to_one_per_join() {
        // What a panel needs in order to be as wide as its widest line: the
        // parts plus a gap at each place two of them meet.
        let bare = MenuItemLabel::new();
        assert_eq!(bare.total_gaps(density(0.0)), 0.0, "a label on its own");

        let mut busy = MenuItemLabel::new()
            .with_leading_icon(true)
            .with_trailing_icon(true)
            .with_shortcut(true);
        busy.has_submenu = true;
        assert_eq!(
            busy.total_gaps(density(0.0)),
            4.0 * MenuItemLabel::DEFAULT_SPACING,
            "one before the label and one before each of the three after it"
        );
        assert_eq!(
            busy.in_a_horizontal_bar(true).total_gaps(density(0.0)),
            2.0 * MenuItemLabel::DEFAULT_SPACING,
            "and in a bar, one before the label and one before the icon"
        );
    }

    #[test]
    fn an_item_never_has_a_submenu_and_a_submenu_button_does() {
        // The one difference between the two lines upstream builds from the
        // same `_MenuItemLabel`.
        assert!(
            !MenuItemButton::new()
                .label(false, false, false, false)
                .has_submenu
        );
        assert!(
            SubmenuButton::new()
                .label(false, false, false, false)
                .has_submenu
        );
        assert!(
            !SubmenuButton {
                has_submenu_icon: false,
                ..SubmenuButton::new()
            }
            .label(false, false, false, false)
            .has_submenu,
            "a submenu with no arrow slot has no arrow"
        );
    }

    #[test]
    fn a_submenus_arrow_takes_the_same_gap_as_anything_else_after_the_label() {
        // The arrow is a trailing part like the others, not a special case
        // with a spacing of its own.
        let submenu = SubmenuButton::new().label(false, false, false, false);
        assert_eq!(submenu.trailing_parts(), vec![MenuItemPart::SubmenuIcon]);
        assert_eq!(
            submenu.total_gaps(density(0.0)),
            MenuItemLabel::DEFAULT_SPACING
        );
        assert_eq!(
            SubmenuButton::new()
                .label(false, false, false, true)
                .total_gaps(density(0.0)),
            0.0,
            "and in a bar it is not there at all -- which is why a menu bar's \
             top-level entries are bare words though every one opens a submenu"
        );
    }

    fn stripped(label: &str) -> (String, Option<usize>) {
        MenuAcceleratorLabel::strip_accelerator_markers(label)
    }

    // -- Opening and closing, tick 319 -------------------------------------

    use crate::animation::AnimationStatus::{Completed, Dismissed, Forward, Reverse};

    const EVERY_STATE: [crate::animation::AnimationStatus; 4] =
        [Dismissed, Forward, Reverse, Completed];

    #[test]
    fn a_menu_that_has_finished_closing_is_not_closing() {
        // Three predicates over the same four states, and no two of them are
        // the same set.
        assert!(is_closing(Reverse));
        assert!(!is_closing(Dismissed), "closed, which is not closing");
        assert!(!is_closing(Forward) && !is_closing(Completed));

        assert!(is_closing_or_closed(Dismissed) && is_closing_or_closed(Reverse));
        assert!(!is_closing_or_closed(Forward) && !is_closing_or_closed(Completed));

        // The two differ on exactly one state, which is the one the parent
        // guard turns on.
        let differ: Vec<_> = EVERY_STATE
            .iter()
            .filter(|status| is_closing(**status) != is_closing_or_closed(**status))
            .collect();
        assert_eq!(differ, vec![&Dismissed]);
    }

    #[test]
    fn a_closing_parent_blocks_a_submenu_and_a_closed_one_does_not() {
        // It is a race, not a state check: a closing parent is on its way to
        // taking the child down with it. A dismissed parent is just a menu.
        let blocked = menu_open_request(Some(Reverse), Dismissed);
        assert!(!blocked.shows_overlay && !blocked.starts_animation);

        let allowed = menu_open_request(Some(Dismissed), Dismissed);
        assert!(
            allowed.shows_overlay && allowed.starts_animation,
            "a shut parent is not an obstacle"
        );

        for parent in [Forward, Completed] {
            assert!(menu_open_request(Some(parent), Dismissed).shows_overlay);
        }
        assert!(
            menu_open_request(None, Dismissed).shows_overlay,
            "no parent"
        );
    }

    #[test]
    fn the_overlay_goes_up_even_when_the_animation_is_skipped() {
        // showOverlay() runs before the animation is looked at. Folding the
        // two into one early return is the natural simplification and loses
        // the case where the entry was taken down while the animation stayed
        // at its end.
        let already = menu_open_request(None, Completed);
        assert!(already.shows_overlay, "still shown");
        assert!(!already.starts_animation, "but not animated again");
    }

    #[test]
    fn a_menu_caught_mid_close_re_opens_from_where_it_got_to() {
        // `reverse` is not forward-or-completed. Asking "is it visible?"
        // would have said yes and left it closing.
        let reopened = menu_open_request(None, Reverse);
        assert!(reopened.starts_animation);
        assert!(
            !menu_open_request(None, Forward).starts_animation,
            "and one already opening is left to finish"
        );
    }

    #[test]
    fn closing_something_already_closing_does_nothing_at_all() {
        // Restarting the reverse would jump it back to full size, and the
        // completion callback that hides the overlay would be armed twice.
        assert!(!menu_close_request(Reverse));
        assert!(!menu_close_request(Dismissed), "nothing left to hide");
        assert!(menu_close_request(Forward), "caught mid-open, so turn back");
        assert!(menu_close_request(Completed));
    }

    #[test]
    fn the_open_and_close_guards_are_exact_mirrors() {
        // Every state either starts an open or starts a close, never both and
        // never neither -- which is what makes the pair total.
        for status in EVERY_STATE {
            assert_ne!(
                menu_open_request(None, status).starts_animation,
                menu_close_request(status),
                "{status:?}"
            );
        }
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
