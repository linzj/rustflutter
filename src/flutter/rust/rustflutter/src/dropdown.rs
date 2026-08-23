//! Ports of `material/dropdown.dart`, `material/dropdown_menu.dart` and
//! `material/dropdown_menu_form_field.dart`.
//!
//! Choosing one thing from a list, twice: the Material 2 `DropdownButton` and
//! the Material 3 `DropdownMenu`. Upstream's own migration notes say the
//! visuals differ "a little bit" and then give the change that actually
//! matters -- **`DropdownButton` makes the application hold the current value
//! and `DropdownMenu` holds it itself.** One is a controlled widget and the
//! other is not, and everything else is decoration.

use crate::direction::TextDirection;

/// Upstream's `_kMenuItemHeight`, which is `kMinInteractiveDimension`.
pub const MENU_ITEM_HEIGHT: f32 = 48.0;
/// Upstream's `_kDenseButtonHeight`.
pub const DENSE_BUTTON_HEIGHT: f32 = 24.0;
/// Upstream's `kMaterialListPadding.vertical`.
pub const LIST_PADDING_VERTICAL: f32 = 8.0;

/// Upstream `DropdownMenuItem`: one row of a [`DropdownButton`]'s menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropdownMenuItem {
    pub value: i32,
    /// Defaults to true. A disabled item is still **laid out and shown** -- it
    /// is part of the list the reader is choosing from, and hiding it would
    /// change what they think the options are.
    pub enabled: bool,
    pub child: u64,
}

impl DropdownMenuItem {
    pub fn new(value: i32, child: u64) -> DropdownMenuItem {
        DropdownMenuItem {
            value,
            enabled: true,
            child,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Upstream `DropdownButtonHideUnderline`.
///
/// An inherited widget with **no data at all**: its presence is the whole
/// message. `at(context)` is a null check on the lookup, and
/// `updateShouldNotify` returns false because there is nothing that could have
/// changed -- appearing and disappearing are changes of tree shape, which the
/// framework already handles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DropdownButtonHideUnderline;

impl DropdownButtonHideUnderline {
    /// Upstream's static `at`.
    pub fn at(ancestor_present: bool) -> bool {
        ancestor_present
    }

    pub fn update_should_notify() -> bool {
        false
    }
}

/// Where the menu ended up. Upstream's `_MenuLimits`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuLimits {
    pub top: f32,
    pub bottom: f32,
    pub height: f32,
    /// How far the menu is scrolled so the selected item still lines up when
    /// the whole list does not fit.
    pub scroll_offset: f32,
}

/// Upstream `DropdownButton`, and the layout that makes it feel native.
#[derive(Clone, Debug, PartialEq)]
pub struct DropdownButton {
    /// The current value. **The application holds this**, which is what makes
    /// this a controlled widget: it does not change until the app says so.
    pub value: Option<i32>,
    pub items: Vec<DropdownMenuItem>,
    pub item_heights: Vec<f32>,
    pub menu_max_height: Option<f32>,
    pub is_dense: bool,
}

impl DropdownButton {
    pub fn new(items: Vec<DropdownMenuItem>) -> DropdownButton {
        let item_heights = vec![MENU_ITEM_HEIGHT; items.len()];
        DropdownButton {
            value: None,
            items,
            item_heights,
            menu_max_height: None,
            is_dense: false,
        }
    }

    pub fn with_value(mut self, value: i32) -> Self {
        self.value = Some(value);
        self
    }

    /// Where this dropdown's insets sit, which the ambient `ButtonTheme`
    /// decides -- see [`crate::component_themes::DropdownAlignment`].
    ///
    /// `in_input_decorator` is upstream's `widget._inputDecoration == null`,
    /// and it conditions only half the answer.
    pub fn alignment(
        &self,
        context: &mut crate::framework::BuildContext,
        in_input_decorator: bool,
    ) -> crate::component_themes::DropdownAlignment {
        crate::component_themes::DropdownAlignment::from_theme(context, in_input_decorator)
    }

    pub fn selected_index(&self) -> usize {
        self.value
            .and_then(|value| self.items.iter().position(|item| item.value == value))
            .unwrap_or(0)
    }

    /// Where an item starts, measured from the top of the menu's content.
    pub fn item_offset(&self, index: usize) -> f32 {
        let mut offset = LIST_PADDING_VERTICAL / 2.0;
        for height in self.item_heights.iter().take(index) {
            offset += height;
        }
        offset
    }

    /// Upstream's `getConstraintsForChild` maximum.
    ///
    /// The menu is capped at the viewport height less **two** item heights, and
    /// upstream cites the Material spec for why: *"This ensures a tappable area
    /// outside of the simple menu with which to dismiss the menu."* A menu
    /// filling the screen would have nowhere left to tap to get out of it.
    pub fn max_menu_height(&self, available_height: f32) -> f32 {
        let computed = (available_height - 2.0 * MENU_ITEM_HEIGHT).max(0.0);
        match self.menu_max_height {
            Some(requested) if requested <= computed => requested,
            _ => computed,
        }
    }

    /// Upstream `getMenuLimits`, which is the whole reason this widget feels
    /// like the platform's own control: **the menu is placed so the currently
    /// selected item lands over the button.** Press "Medium" and "Medium" is
    /// under your finger, so choosing again is a small movement rather than a
    /// hunt.
    pub fn menu_limits(
        &self,
        button_top: f32,
        button_height: f32,
        available_height: f32,
    ) -> MenuLimits {
        let index = self.selected_index();
        let computed_max_height = self.max_menu_height(available_height);
        let button_bottom = (button_top + button_height).min(available_height);
        let selected_offset = self.item_offset(index);
        let selected_height = self
            .item_heights
            .get(index)
            .copied()
            .unwrap_or(MENU_ITEM_HEIGHT);

        // Normally a menu item's height of margin at each edge -- but if the
        // button is nearer the edge than that, the button's own edge. **The
        // margin is a preference that yields to the button.**
        let top_limit = MENU_ITEM_HEIGHT.min(button_top);
        let bottom_limit = (available_height - MENU_ITEM_HEIGHT).max(button_bottom);

        // Centre the selected item on the button.
        let mut menu_top = (button_top - selected_offset) - (selected_height - button_height) / 2.0;
        let preferred = LIST_PADDING_VERTICAL + self.item_heights.iter().sum::<f32>();
        let menu_height = computed_max_height.min(preferred);
        let mut menu_bottom = menu_top + menu_height;

        // Three corrections, in order.
        if menu_top < top_limit {
            menu_top = button_top.min(top_limit);
            menu_bottom = menu_top + menu_height;
        }
        if menu_bottom > bottom_limit {
            menu_bottom = button_bottom.max(bottom_limit);
            menu_top = menu_bottom - menu_height;
        }
        // And a third that undoes the damage of the first two: if clamping
        // pushed the selected item's centre above the button's, pull it back.
        if menu_bottom - selected_height / 2.0 < button_bottom - button_height / 2.0 {
            menu_bottom = button_bottom - button_height / 2.0 + selected_height / 2.0;
            menu_top = menu_bottom - menu_height;
        }

        // When the list does not fit, the selected item is lined up by
        // scrolling instead. Upstream notes two limits on this honestly: it is
        // done **only when the menu is first shown** -- afterwards the reader's
        // own scroll position is left alone -- and it is **only accurate for
        // fixed-height items**, which is the default and not a guarantee.
        let scroll_offset = if preferred > computed_max_height {
            (selected_offset - (button_top - menu_top))
                .max(0.0)
                .min(preferred - menu_height)
        } else {
            0.0
        };

        MenuLimits {
            top: menu_top,
            bottom: menu_bottom,
            height: menu_height,
            scroll_offset,
        }
    }

    /// Upstream's horizontal placement, which reads from the far edge in
    /// right-to-left.
    pub fn menu_left(
        button_left: f32,
        button_right: f32,
        child_width: f32,
        available_width: f32,
        direction: TextDirection,
    ) -> f32 {
        match direction {
            TextDirection::Rtl => button_right.clamp(0.0, available_width) - child_width,
            TextDirection::Ltr => button_left.clamp(0.0, available_width - child_width),
        }
    }

    /// Upstream's assert is guarded on the button being fully on screen, with
    /// the comment saying so: *"If the button was a bit off-screen, then, oh
    /// well."* An invariant that only holds in the case anybody can reason
    /// about, said out loud rather than quietly assumed.
    pub fn menu_is_on_screen_check_applies(button_fully_on_screen: bool) -> bool {
        button_fully_on_screen
    }
}

/// Upstream `DropdownButtonFormField`: the same button inside a `FormField`, so
/// it validates and saves with everything else on the form.
#[derive(Clone, Debug, PartialEq)]
pub struct DropdownButtonFormField {
    pub button: DropdownButton,
    pub has_validator: bool,
}

impl DropdownButtonFormField {
    pub fn new(button: DropdownButton) -> DropdownButtonFormField {
        DropdownButtonFormField {
            button,
            has_validator: false,
        }
    }

    /// A form field's value and the widget's value are the same thing, which is
    /// why this wrapper exists at all rather than a caller wiring the two
    /// together and getting it subtly wrong.
    pub fn value(&self) -> Option<i32> {
        self.button.value
    }
}

/// Upstream `DropdownMenuEntry`: one row of a [`DropdownMenu`].
///
/// Where a [`DropdownMenuItem`] carries a **widget**, this carries a **label**
/// -- a string. That is what lets `DropdownMenu` filter as the reader types:
/// you cannot search a widget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropdownMenuEntry {
    pub value: i32,
    pub label: String,
    pub enabled: bool,
    pub has_leading_icon: bool,
    pub has_trailing_icon: bool,
}

impl DropdownMenuEntry {
    pub fn new(value: i32, label: impl Into<String>) -> DropdownMenuEntry {
        DropdownMenuEntry {
            value,
            label: label.into(),
            enabled: true,
            has_leading_icon: false,
            has_trailing_icon: false,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Upstream `DropdownMenuCloseBehavior`: what shuts when an entry is chosen.
///
/// # Three values over two separate mechanisms
///
/// Upstream does not switch on this once. It reads it twice, in two places
/// that close different things:
///
/// ```dart
/// closeOnActivate: widget.closeBehavior == DropdownMenuCloseBehavior.all,
/// ...
/// if (widget.closeBehavior == DropdownMenuCloseBehavior.self) {
///   _controller.close();
/// }
/// ```
///
/// The first hands the job to the menu system, which walks up and shuts
/// everything it finds. The second closes **this** menu's controller and
/// nothing above it. So the two questions are *does the menu system close
/// everything* and *does this menu close itself*, and the three values are
/// three of their four combinations.
///
/// The fourth -- doing both -- is not a value, and would not want to be:
/// telling the menu system to close everything and then closing yourself as
/// well is asking twice for something that has already happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DropdownMenuCloseBehavior {
    /// Every open menu in the tree. Upstream's default, and the only value
    /// that reaches menus this one did not open.
    #[default]
    All,
    /// This menu only, leaving an enclosing menu open.
    SelfOnly,
    /// Nothing closes; the menu stays up after a choice.
    None,
}

impl DropdownMenuCloseBehavior {
    pub const ALL: [DropdownMenuCloseBehavior; 3] = [
        DropdownMenuCloseBehavior::All,
        DropdownMenuCloseBehavior::SelfOnly,
        DropdownMenuCloseBehavior::None,
    ];

    /// Upstream's `closeOnActivate` on the item button: the menu system's own
    /// closing, which does not stop at this menu.
    pub fn closes_the_whole_tree(self) -> bool {
        matches!(self, DropdownMenuCloseBehavior::All)
    }

    /// Upstream's explicit `_controller.close()` in `onPressed`.
    pub fn closes_this_menu_itself(self) -> bool {
        matches!(self, DropdownMenuCloseBehavior::SelfOnly)
    }

    /// Whether this menu ends up shut, by either route.
    pub fn leaves_this_menu_open(self) -> bool {
        !self.closes_the_whole_tree() && !self.closes_this_menu_itself()
    }
}

/// Upstream `DropdownMenu`, the Material 3 one.
///
/// It is a text field with a menu attached, and the difference from
/// [`DropdownButton`] that matters is not how it looks: **it keeps the
/// selection itself.** The application gives an `initial_selection` and is told
/// about changes; it does not have to hold the value and hand it back.
#[derive(Clone, Debug, PartialEq)]
pub struct DropdownMenu {
    /// Where it starts. Compare [`DropdownButton::value`], which is where it
    /// **is**.
    pub initial_selection: Option<i32>,
    pub entries: Vec<DropdownMenuEntry>,
    /// Whether the reader can type to filter the entries. Being a text field is
    /// what makes this possible at all.
    pub enable_filter: bool,
    /// Whether the field can be typed in freely, or only chosen from.
    pub enable_search: bool,
    /// Upstream's `closeBehavior`.
    pub close_behavior: DropdownMenuCloseBehavior,
    selection: Option<i32>,
}

impl DropdownMenu {
    pub fn new(entries: Vec<DropdownMenuEntry>) -> DropdownMenu {
        DropdownMenu {
            initial_selection: None,
            entries,
            enable_filter: false,
            enable_search: true,
            close_behavior: DropdownMenuCloseBehavior::All,
            selection: None,
        }
    }

    /// This menu's appearance -- see
    /// [`crate::component_themes::ResolvedDropdownMenu`], most of which is
    /// other components' themes.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        enabled: bool,
    ) -> crate::component_themes::ResolvedDropdownMenu {
        crate::component_themes::ResolvedDropdownMenu::of(context, enabled)
    }

    pub fn with_initial_selection(mut self, value: i32) -> Self {
        self.initial_selection = Some(value);
        self.selection = Some(value);
        self
    }

    pub fn with_filter(mut self) -> Self {
        self.enable_filter = true;
        self
    }

    /// What is selected now, which this widget knows without being told.
    pub fn selection(&self) -> Option<i32> {
        self.selection
    }

    pub fn select(&mut self, value: Option<i32>) {
        self.selection = value;
    }

    /// Upstream's filter: the entries whose label contains what has been typed.
    /// With filtering off, every entry stays -- the text is a search cursor
    /// rather than a sieve.
    pub fn filtered(&self, typed: &str) -> Vec<&DropdownMenuEntry> {
        if !self.enable_filter || typed.is_empty() {
            return self.entries.iter().collect();
        }
        let needle = typed.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| entry.label.to_lowercase().contains(&needle))
            .collect()
    }

    /// Upstream's search, which highlights rather than removes: the first entry
    /// whose label starts with what was typed.
    pub fn search(&self, typed: &str) -> Option<usize> {
        if !self.enable_search || typed.is_empty() {
            return None;
        }
        let needle = typed.to_lowercase();
        self.entries
            .iter()
            .position(|entry| entry.label.to_lowercase().starts_with(&needle))
    }
}

/// Upstream `DropdownMenuFormField`: [`DropdownMenu`] inside a `FormField`.
#[derive(Clone, Debug, PartialEq)]
pub struct DropdownMenuFormField {
    pub menu: DropdownMenu,
    pub has_validator: bool,
}

impl DropdownMenuFormField {
    pub fn new(menu: DropdownMenu) -> DropdownMenuFormField {
        DropdownMenuFormField {
            menu,
            has_validator: false,
        }
    }

    /// The form field's value is the menu's own selection, which is the point:
    /// the menu already knows, so the field does not keep a second copy to
    /// disagree with it.
    pub fn value(&self) -> Option<i32> {
        self.menu.selection()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button() -> DropdownButton {
        DropdownButton::new(vec![
            DropdownMenuItem::new(0, 10),
            DropdownMenuItem::new(1, 11),
            DropdownMenuItem::new(2, 12),
        ])
    }

    // -- The layout that makes it feel native ---------------------------------

    #[test]
    fn the_selected_item_lands_over_the_button() {
        // Press "Medium" and "Medium" is under your finger, so choosing again
        // is a small movement rather than a hunt.
        let middle = button().with_value(1);
        let limits = middle.menu_limits(300.0, 48.0, 800.0);
        // The selected item's top is menu_top + its offset.
        let selected_top = limits.top + middle.item_offset(1);
        assert!(
            (selected_top - 300.0).abs() < 0.01,
            "selected item at {selected_top}, button at 300"
        );
    }

    #[test]
    fn a_different_selection_moves_the_whole_menu_not_the_item() {
        let first = button().with_value(0).menu_limits(300.0, 48.0, 800.0);
        let last = button().with_value(2).menu_limits(300.0, 48.0, 800.0);
        assert!(
            last.top < first.top,
            "the menu slid up to bring item 2 down"
        );
        assert_eq!(first.height, last.height);
    }

    #[test]
    fn a_menu_never_fills_the_screen_so_there_is_somewhere_to_tap_to_dismiss_it() {
        // Upstream cites the Material spec for this.
        let many = DropdownButton::new(
            (0..40)
                .map(|i| DropdownMenuItem::new(i, i as u64))
                .collect(),
        );
        assert_eq!(many.max_menu_height(800.0), 800.0 - 96.0);
        assert!(many.menu_limits(300.0, 48.0, 800.0).height <= 704.0);
    }

    #[test]
    fn a_caller_may_ask_for_less_but_not_for_more() {
        let mut capped = button();
        capped.menu_max_height = Some(200.0);
        assert_eq!(capped.max_menu_height(800.0), 200.0);

        capped.menu_max_height = Some(5000.0);
        assert_eq!(
            capped.max_menu_height(800.0),
            704.0,
            "an over-large request is ignored rather than honoured"
        );
    }

    #[test]
    fn the_edge_margin_yields_to_a_button_that_is_nearer_the_edge() {
        // Normally an item's height of margin; but a button ten pixels from
        // the top gets a menu ten pixels from the top.
        let near_top = button().with_value(0).menu_limits(10.0, 48.0, 800.0);
        assert!(near_top.top >= 0.0);
        assert!(near_top.top <= 10.0);
    }

    #[test]
    fn a_menu_at_the_bottom_of_the_screen_stays_on_it() {
        let near_bottom = button().with_value(2).menu_limits(760.0, 48.0, 800.0);
        assert!(near_bottom.bottom <= 800.0 + 0.01, "{}", near_bottom.bottom);
        assert!(near_bottom.top >= 0.0);
    }

    #[test]
    fn a_menu_too_long_to_fit_scrolls_to_the_selection_instead_of_moving_to_it() {
        let many = DropdownButton::new(
            (0..40)
                .map(|i| DropdownMenuItem::new(i, i as u64))
                .collect(),
        )
        .with_value(30);
        let limits = many.menu_limits(300.0, 48.0, 800.0);
        assert!(limits.scroll_offset > 0.0);
        assert!(
            limits.scroll_offset <= LIST_PADDING_VERTICAL + 40.0 * MENU_ITEM_HEIGHT - limits.height,
            "and never past the end of the list"
        );
    }

    #[test]
    fn a_menu_that_fits_does_not_scroll_at_all() {
        assert_eq!(
            button()
                .with_value(2)
                .menu_limits(300.0, 48.0, 800.0)
                .scroll_offset,
            0.0
        );
    }

    #[test]
    fn the_menu_reads_from_the_far_edge_in_right_to_left() {
        assert_eq!(
            DropdownButton::menu_left(100.0, 300.0, 200.0, 400.0, TextDirection::Ltr),
            100.0
        );
        assert_eq!(
            DropdownButton::menu_left(100.0, 300.0, 200.0, 400.0, TextDirection::Rtl),
            100.0,
            "the same here, because the button is exactly the menu's width"
        );
        assert_eq!(
            DropdownButton::menu_left(100.0, 300.0, 250.0, 400.0, TextDirection::Rtl),
            50.0,
            "a wider menu grows leftwards from the button's right edge"
        );
    }

    #[test]
    fn the_on_screen_check_only_applies_where_it_can_be_reasoned_about() {
        // Upstream says so out loud: if the button was a bit off-screen, oh
        // well.
        assert!(DropdownButton::menu_is_on_screen_check_applies(true));
        assert!(!DropdownButton::menu_is_on_screen_check_applies(false));
    }

    // -- The hide-underline trick -----------------------------------------------

    #[test]
    fn an_inherited_widget_whose_payload_is_its_own_existence() {
        assert!(DropdownButtonHideUnderline::at(true));
        assert!(!DropdownButtonHideUnderline::at(false));
        assert!(
            !DropdownButtonHideUnderline::update_should_notify(),
            "there is nothing that could have changed"
        );
    }

    // -- Items -------------------------------------------------------------------

    #[test]
    fn a_disabled_item_is_still_shown_because_it_is_one_of_the_options() {
        let item = DropdownMenuItem::new(1, 11).disabled();
        assert!(!item.enabled);
        assert_eq!(item.value, 1);
    }

    #[test]
    fn nothing_selected_falls_back_to_the_first_item() {
        assert_eq!(button().selected_index(), 0);
        assert_eq!(button().with_value(2).selected_index(), 2);
    }

    // -- The Material 3 one ---------------------------------------------------------

    fn menu() -> DropdownMenu {
        DropdownMenu::new(vec![
            DropdownMenuEntry::new(0, "Small"),
            DropdownMenuEntry::new(1, "Medium"),
            DropdownMenuEntry::new(2, "Large"),
        ])
    }

    #[test]
    fn the_button_is_told_its_value_and_the_menu_knows_its_own() {
        // Which is the change that actually matters between the two, not the
        // visuals.
        let controlled = button().with_value(1);
        assert_eq!(controlled.value, Some(1));

        let mut uncontrolled = menu().with_initial_selection(1);
        assert_eq!(uncontrolled.selection(), Some(1));
        uncontrolled.select(Some(2));
        assert_eq!(
            uncontrolled.selection(),
            Some(2),
            "and it changed without the application handing it back"
        );
        assert_eq!(
            uncontrolled.initial_selection,
            Some(1),
            "while where it started is still where it started"
        );
    }

    #[test]
    fn an_entry_carries_a_label_and_an_item_carries_a_widget() {
        // Which is what lets the menu filter as the reader types: you cannot
        // search a widget.
        let entry = DropdownMenuEntry::new(1, "Medium");
        assert_eq!(entry.label, "Medium");
    }

    #[test]
    fn filtering_removes_and_searching_only_points() {
        let filtering = menu().with_filter();
        assert_eq!(filtering.filtered("me").len(), 1, "only Medium");
        assert_eq!(filtering.filtered("l").len(), 2, "Small and Large");
        assert_eq!(
            filtering.filtered("m").len(),
            2,
            "Small has one in the middle -- filtering is contains, not starts_with"
        );
        assert_eq!(
            filtering.search("m"),
            Some(1),
            "while searching is starts_with, so it points at Medium alone"
        );

        let searching = menu();
        assert_eq!(
            searching.filtered("m").len(),
            3,
            "with filtering off nothing is removed"
        );
        assert_eq!(searching.search("me"), Some(1));
        assert_eq!(searching.search("z"), None);
    }

    #[test]
    fn an_empty_query_leaves_everything_where_it_was() {
        assert_eq!(menu().with_filter().filtered("").len(), 3);
        assert_eq!(menu().search(""), None);
    }

    // -- The form fields --------------------------------------------------------------

    #[test]
    fn a_form_field_reads_the_value_rather_than_keeping_a_second_copy() {
        let field = DropdownButtonFormField::new(button().with_value(1));
        assert_eq!(field.value(), Some(1));

        let menu_field = DropdownMenuFormField::new(menu().with_initial_selection(2));
        assert_eq!(menu_field.value(), Some(2));
    }
}

#[cfg(test)]
mod aligned_dropdown_tests {
    use super::*;
    use crate::component_themes::{ButtonTheme, ButtonThemeData, DropdownAlignment};
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, leaf};
    use crate::render::EdgeInsetsDirectional;
    use crate::widgets::SizedBox;

    struct Reader {
        in_input_decorator: bool,
        seen: std::rc::Rc<std::cell::RefCell<Option<DropdownAlignment>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() =
                Some(DropdownButton::new(Vec::new()).alignment(context, self.in_input_decorator));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn under_theme(aligned: bool, in_input_decorator: bool) -> DropdownAlignment {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        let mut data = ButtonThemeData::new();
        data.aligned_dropdown = aligned;
        tree.rebuild(ButtonTheme::new(
            data,
            component(Reader {
                in_input_decorator,
                seen: std::rc::Rc::clone(&seen),
            }),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn the_start_inset_changes_hands_between_the_button_and_the_menu() {
        // Exactly one of the two carries it, and it is the same 16 either way.
        let aligned = DropdownAlignment::of(true, false);
        let unaligned = DropdownAlignment::of(false, false);

        assert_eq!(aligned.button_padding.start, 16.0);
        assert_eq!(aligned.menu_margin.start, 0.0);
        assert_eq!(unaligned.button_padding.start, 0.0);
        assert_eq!(unaligned.menu_margin.start, 16.0);

        assert_eq!(
            aligned.button_padding.start + aligned.menu_margin.start,
            unaligned.button_padding.start + unaligned.menu_margin.start,
            "the same inset, on whichever of the two is carrying it"
        );
    }

    #[test]
    fn but_the_end_inset_does_not_transfer_at_all() {
        // 4 against 24. Reading the flag as "move the insets across" would get
        // the start right and the end wrong by twenty pixels: the aligned
        // button's 4 is room beside the arrow, and the unaligned menu's 24 is
        // clearance from what it is not lined up with. Different jobs.
        let aligned = DropdownAlignment::of(true, false);
        let unaligned = DropdownAlignment::of(false, false);
        assert_eq!(aligned.button_padding.end, 4.0);
        assert_eq!(unaligned.menu_margin.end, 24.0);
        assert_ne!(aligned.button_padding.end, unaligned.menu_margin.end);
    }

    #[test]
    fn a_dropdown_in_a_decorator_still_moves_its_menu_but_not_its_padding() {
        // The flag half-applies, and which half depends on something the flag
        // has never heard of: upstream picks the menu margin on
        // `alignedDropdown` alone and the button padding on
        // `alignedDropdown && _inputDecoration == null`.
        let bare = DropdownAlignment::of(true, false);
        let decorated = DropdownAlignment::of(true, true);

        assert_eq!(
            decorated.menu_margin, bare.menu_margin,
            "the menu does not care about the decoration"
        );
        assert_ne!(decorated.button_padding, bare.button_padding);
        assert_eq!(
            decorated.button_padding,
            EdgeInsetsDirectional::ZERO,
            "the decoration's own padding is what applies instead"
        );
    }

    #[test]
    fn and_a_decorator_changes_nothing_when_the_dropdown_is_not_aligned() {
        // The second condition is an `&&`, so it can only take away something
        // the first was giving.
        assert_eq!(
            DropdownAlignment::of(false, true),
            DropdownAlignment::of(false, false)
        );
    }

    #[test]
    fn the_flag_comes_from_the_ambient_button_theme() {
        // Which is why `ButtonTheme` is still alive: `ButtonTheme.of` is read
        // three times upstream, and two of them are this, in a widget that is
        // not a button.
        assert_eq!(under_theme(true, false), DropdownAlignment::of(true, false));
        assert_eq!(
            under_theme(false, false),
            DropdownAlignment::of(false, false)
        );
        assert_ne!(under_theme(true, false), under_theme(false, false));
    }

    #[test]
    fn nothing_is_inset_on_the_cross_axis_either_way() {
        // Both constants are `EdgeInsetsDirectional.only(start:, end:)`: the
        // flag moves a horizontal inset and never touches the vertical, which
        // is the row height's business.
        for (aligned, decorated) in [(true, false), (false, false), (true, true)] {
            let resolved = DropdownAlignment::of(aligned, decorated);
            assert_eq!(resolved.button_padding.top, 0.0);
            assert_eq!(resolved.button_padding.bottom, 0.0);
            assert_eq!(resolved.menu_margin.top, 0.0);
            assert_eq!(resolved.menu_margin.bottom, 0.0);
        }
    }
}

#[cfg(test)]
mod dropdown_menu_theme_tests {
    use super::*;
    use crate::component_themes::{
        DropdownMenuTheme, DropdownMenuThemeData, InputDecorationThemeData, MenuStyle,
        ResolvedDropdownMenu,
    };
    use crate::engine::{Color, TextStyle};
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, leaf};
    use crate::render::Size;
    use crate::theme::ThemeData;
    use crate::widget_state::{StateProperty, WidgetStates};
    use crate::widgets::SizedBox;

    struct Reader {
        enabled: bool,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedDropdownMenu>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() =
                Some(DropdownMenu::new(Vec::new()).resolved(context, self.enabled));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn resolve(data: DropdownMenuThemeData, enabled: bool) -> ResolvedDropdownMenu {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(DropdownMenuTheme::new(
            data,
            component(Reader {
                enabled,
                seen: std::rc::Rc::clone(&seen),
            }),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn plain(enabled: bool) -> ResolvedDropdownMenu {
        resolve(DropdownMenuThemeData::new(), enabled)
    }

    #[test]
    fn a_theme_style_replaces_the_defaults_whole_rather_than_field_by_field() {
        // The `??` end of the distinction `ResolvedIconButton` records the
        // other end of. Setting one thing discards everything the defaults
        // were carrying.
        let mut mine = MenuStyle::new();
        mine.visual_density = Some(crate::theme::VisualDensity::STANDARD);
        let resolved = resolve(
            DropdownMenuThemeData {
                menu_style: Some(mine),
                ..DropdownMenuThemeData::new()
            },
            true,
        );
        assert_eq!(
            resolved.menu_style.minimum_size, None,
            "the 112 floor went with it"
        );
        assert_eq!(resolved.menu_style.maximum_size, None);

        // Where the defaults do carry all three.
        let defaults = plain(true).menu_style;
        assert!(defaults.minimum_size.is_some());
        assert!(defaults.maximum_size.is_some());
        assert!(defaults.visual_density.is_some());
    }

    #[test]
    fn the_default_menu_style_carries_the_width_floor() {
        let style = ResolvedDropdownMenu::default_menu_style();
        assert_eq!(
            style
                .minimum_size
                .as_ref()
                .and_then(|p| p.resolve(WidgetStates::NONE))
                .map(|size| size.width),
            Some(112.0)
        );
        assert_eq!(
            style
                .minimum_size
                .as_ref()
                .and_then(|p| p.resolve(WidgetStates::NONE))
                .map(|size| size.height),
            Some(0.0),
            "a width floor and no height floor -- the entries decide that"
        );
    }

    // -- The width, in the order the reads happen ------------------------------

    #[test]
    fn the_menu_is_at_least_as_wide_as_the_field_that_opened_it() {
        // A dropdown narrower than its own field would look like a different
        // control opening.
        assert_eq!(
            ResolvedDropdownMenu::minimum_width(None, Some(300.0), None, None),
            Some(300.0)
        );
    }

    #[test]
    fn a_given_width_beats_the_anchors() {
        assert_eq!(
            ResolvedDropdownMenu::minimum_width(Some(200.0), Some(300.0), None, None),
            Some(200.0)
        );
    }

    #[test]
    fn and_either_is_clamped_by_the_maximum() {
        assert_eq!(
            ResolvedDropdownMenu::minimum_width(None, Some(900.0), None, Some(400.0)),
            Some(400.0)
        );
        assert_eq!(
            ResolvedDropdownMenu::minimum_width(Some(900.0), None, None, Some(400.0)),
            Some(400.0)
        );
    }

    #[test]
    fn a_menu_height_silently_removes_that_clamp() {
        // The resolver closes over the *variable*, and the maximum is
        // reassigned after it is written, so the clamp reads the maximum that
        // ends up final -- `Size(infinity, height)`, which has no width in it.
        assert_eq!(
            ResolvedDropdownMenu::minimum_width(None, Some(900.0), Some(240.0), Some(400.0)),
            Some(900.0),
            "the 400 cap is gone because a height replaced the whole maximum"
        );
        assert_ne!(
            ResolvedDropdownMenu::minimum_width(None, Some(900.0), Some(240.0), Some(400.0)),
            ResolvedDropdownMenu::minimum_width(None, Some(900.0), None, Some(400.0)),
            "which is only visible because the two differ"
        );
    }

    #[test]
    fn with_neither_a_width_nor_an_anchor_there_is_nothing_to_say() {
        // Before the anchor has been measured there is no width to floor to,
        // and the style's own minimum stands.
        assert_eq!(
            ResolvedDropdownMenu::minimum_width(None, None, None, Some(400.0)),
            None
        );
    }

    // -- The other three fields -------------------------------------------------

    #[test]
    fn a_disabled_menu_gets_a_text_style_even_with_no_base_to_recolour() {
        // `baseTextStyle?.copyWith(...) ?? TextStyle(color: disabledColor)`.
        let grey = Color(0xFF888888);
        let from_nothing = ResolvedDropdownMenu::text_style_for(None, false, grey);
        assert_eq!(from_nothing.map(|style| style.color), Some(grey));

        assert_eq!(
            ResolvedDropdownMenu::text_style_for(None, true, grey),
            None,
            "where an enabled one with no base has no style at all"
        );
    }

    #[test]
    fn and_a_disabled_one_keeps_the_rest_of_the_style_it_recoloured() {
        let grey = Color(0xFF888888);
        let base = TextStyle {
            font_size: 31.0,
            color: Color(0xFF000000),
            ..TextStyle::default()
        };
        let recoloured =
            ResolvedDropdownMenu::text_style_for(Some(base.clone()), false, grey).unwrap();
        assert_eq!(recoloured.color, grey);
        assert_eq!(recoloured.font_size, 31.0, "the size survived");

        assert_eq!(
            ResolvedDropdownMenu::text_style_for(Some(base.clone()), true, grey),
            Some(base),
            "and an enabled one is left alone"
        );
    }

    #[test]
    fn the_disabled_colour_is_the_same_fade_as_everywhere_else() {
        let scheme = ThemeData::fallback().color_scheme;
        assert_eq!(
            plain(true).disabled_color,
            crate::elevation_overlay::with_opacity(scheme.on_surface, 0.38)
        );
        assert_eq!(
            plain(false).text_style.map(|style| style.color),
            Some(plain(true).disabled_color)
        );
    }

    #[test]
    fn a_dropdowns_field_is_outlined_where_a_bare_ones_is_not() {
        // `defaults.inputDecorationTheme` is `border: OutlineInputBorder()`,
        // against `_getDefaultBorder`'s `?? const UnderlineInputBorder()`.
        assert!(plain(true).input_border_is_outline);
        assert!(
            !resolve(
                DropdownMenuThemeData {
                    input_decoration_theme: Some(InputDecorationThemeData::new()),
                    ..DropdownMenuThemeData::new()
                },
                true
            )
            .input_border_is_outline,
            "and a theme that supplies one takes that default with it, whole"
        );
    }

    #[test]
    fn the_theme_supplies_the_text_style_over_the_typography() {
        let mine = TextStyle {
            font_size: 41.0,
            ..TextStyle::default()
        };
        let resolved = resolve(
            DropdownMenuThemeData {
                text_style: Some(mine),
                ..DropdownMenuThemeData::new()
            },
            true,
        );
        assert_eq!(resolved.text_style.map(|style| style.font_size), Some(41.0));
        assert_eq!(
            plain(true).text_style.map(|style| style.font_size),
            ThemeData::fallback()
                .text_theme
                .body_large
                .map(|style| style.font_size),
            "and with none, the typography's bodyLarge"
        );
    }

    #[test]
    fn a_menu_style_from_the_theme_reaches_the_panel_as_the_widget_step() {
        // Which is why a `MenuTheme` around a dropdown cannot move these: the
        // dropdown hands them in above it.
        let mut mine = MenuStyle::new();
        mine.minimum_size = Some(StateProperty::all(Some(Size::new(500.0, 0.0))));
        let resolved = resolve(
            DropdownMenuThemeData {
                menu_style: Some(mine),
                ..DropdownMenuThemeData::new()
            },
            true,
        );
        assert_eq!(
            resolved
                .menu_style
                .minimum_size
                .as_ref()
                .and_then(|p| p.resolve(WidgetStates::NONE))
                .map(|size| size.width),
            Some(500.0)
        );
    }
}

#[cfg(test)]
mod close_behavior_tests {
    use super::{DropdownMenu, DropdownMenuCloseBehavior};

    #[test]
    fn only_all_reaches_menus_this_one_did_not_open() {
        // closeOnActivate hands the job to the menu system, which walks up.
        assert!(DropdownMenuCloseBehavior::All.closes_the_whole_tree());
        assert!(!DropdownMenuCloseBehavior::SelfOnly.closes_the_whole_tree());
        assert!(!DropdownMenuCloseBehavior::None.closes_the_whole_tree());
    }

    #[test]
    fn and_only_self_closes_this_controller_by_hand() {
        // The explicit _controller.close() in onPressed, which stops here.
        assert!(DropdownMenuCloseBehavior::SelfOnly.closes_this_menu_itself());
        assert!(!DropdownMenuCloseBehavior::All.closes_this_menu_itself());
        assert!(!DropdownMenuCloseBehavior::None.closes_this_menu_itself());
    }

    #[test]
    fn the_two_mechanisms_are_never_both_used() {
        // Three values over two booleans, and the fourth combination is not a
        // value: telling the menu system to shut everything and then shutting
        // yourself as well asks twice for what has already happened.
        for behavior in DropdownMenuCloseBehavior::ALL {
            assert!(
                !(behavior.closes_the_whole_tree() && behavior.closes_this_menu_itself()),
                "{behavior:?} uses both"
            );
        }
        // And the three values really are three different pairs of answers.
        let mut answers: Vec<(bool, bool)> = DropdownMenuCloseBehavior::ALL
            .iter()
            .map(|b| (b.closes_the_whole_tree(), b.closes_this_menu_itself()))
            .collect();
        answers.sort();
        answers.dedup();
        assert_eq!(answers.len(), 3);
    }

    #[test]
    fn two_of_the_three_leave_this_menu_shut() {
        // Different routes, same outcome for this menu -- which is why the
        // difference between all and self is only visible from an enclosing
        // menu.
        assert!(!DropdownMenuCloseBehavior::All.leaves_this_menu_open());
        assert!(!DropdownMenuCloseBehavior::SelfOnly.leaves_this_menu_open());
        assert!(DropdownMenuCloseBehavior::None.leaves_this_menu_open());
    }

    #[test]
    fn and_none_is_the_only_one_that_leaves_a_choice_showing() {
        let staying: Vec<DropdownMenuCloseBehavior> = DropdownMenuCloseBehavior::ALL
            .into_iter()
            .filter(|b| b.leaves_this_menu_open())
            .collect();
        assert_eq!(staying, vec![DropdownMenuCloseBehavior::None]);
    }

    #[test]
    fn a_dropdown_shuts_everything_unless_told_otherwise() {
        assert_eq!(
            DropdownMenu::new(Vec::new()).close_behavior,
            DropdownMenuCloseBehavior::All
        );
        assert_eq!(
            DropdownMenuCloseBehavior::default(),
            DropdownMenuCloseBehavior::All
        );
    }
}
