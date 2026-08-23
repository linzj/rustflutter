//! Ports of `material/bottom_navigation_bar.dart`, `material/navigation_bar.dart`
//! and `material/bottom_app_bar.dart`.
//!
//! The bar along the bottom, three ways: the Material 2 navigation bar, the
//! Material 3 one that replaced it, and the plain bar that holds actions rather
//! than destinations. Kept in one module because the interesting thing is the
//! contrast between them.

/// Upstream `BottomNavigationBarType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomNavigationBarType {
    /// Every item the same width, every label always shown.
    Fixed,
    /// Items move and labels fade in when tapped -- only the selected one is
    /// labelled.
    Shifting,
}

/// Upstream `BottomNavigationBarLandscapeLayout`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BottomNavigationBarLandscapeLayout {
    /// Spread evenly across the whole width.
    #[default]
    Spread,
    /// Keep the width they would have had in portrait and centre the row.
    ///
    /// Five items stretched across a landscape phone end up absurdly far
    /// apart, and a thumb that reached the first one cannot reach the last.
    Centered,
    /// Each item's label beside its icon rather than under it, which is what
    /// makes a short bar work in landscape.
    Linear,
}

/// Upstream `BottomNavigationBar`.
#[derive(Clone, Debug, PartialEq)]
pub struct BottomNavigationBar {
    pub item_count: usize,
    /// Whether every item was given a label. Upstream asserts it.
    pub all_items_labelled: bool,
    pub current_index: usize,
    /// `None` means "work it out from the count".
    pub bar_type: Option<BottomNavigationBarType>,
    pub has_selected_item_color: bool,
    pub has_fixed_color: bool,
    /// `None` defers to the theme, then to `true`.
    pub show_selected_labels: Option<bool>,
    /// `None` defers to the theme, then to a default computed from the
    /// *resolved* type -- see
    /// [`crate::component_themes::ResolvedBottomNavigationBar`].
    pub show_unselected_labels: Option<bool>,
}

impl BottomNavigationBar {
    pub fn new(item_count: usize, current_index: usize) -> BottomNavigationBar {
        BottomNavigationBar {
            item_count,
            all_items_labelled: true,
            current_index,
            bar_type: None,
            has_selected_item_color: false,
            has_fixed_color: false,
            show_selected_labels: None,
            show_unselected_labels: None,
        }
    }

    /// This bar's appearance, with the theme and the defaults folded in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedBottomNavigationBar {
        crate::component_themes::ResolvedBottomNavigationBar::of(context, self)
    }

    /// Upstream's constructor asserts.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.item_count < 2 {
            // A bar with one destination is not navigation.
            return Err("items.length must be at least two");
        }
        if !self.all_items_labelled {
            // Even in shifting mode, where labels are hidden. **Hiding a label
            // is not removing it** -- it is still what a screen reader reads
            // out, and an item with none would be an unnamed button.
            return Err("Every item must have a non-null label");
        }
        if self.current_index >= self.item_count {
            return Err("currentIndex must be a valid index into items");
        }
        if self.has_selected_item_color && self.has_fixed_color {
            // The same slot under two names, one of them older.
            return Err("Either selectedItemColor or fixedColor can be specified, but not both");
        }
        Ok(())
    }

    /// Upstream `_effectiveType`, whose default is the interesting part:
    /// **fixed for three items or fewer, shifting for four or more.**
    ///
    /// The layout changes because the room ran out, not because anybody chose.
    /// With four labels across a phone there is not width for all of them, so
    /// only the selected one is written and the items slide to make space for
    /// it.
    pub fn effective_type(
        &self,
        theme_type: Option<BottomNavigationBarType>,
    ) -> BottomNavigationBarType {
        self.bar_type.or(theme_type).unwrap_or({
            if self.item_count <= 3 {
                BottomNavigationBarType::Fixed
            } else {
                BottomNavigationBarType::Shifting
            }
        })
    }

    /// Whether an item's label is drawn.
    pub fn shows_label(&self, index: usize, effective_type: BottomNavigationBarType) -> bool {
        match effective_type {
            BottomNavigationBarType::Fixed => true,
            BottomNavigationBarType::Shifting => index == self.current_index,
        }
    }
}

/// Upstream `NavigationBar`, the Material 3 replacement.
///
/// The same two asserts and **no type at all**: it does not shift, so the
/// count-based default disappears. Every destination keeps its label whatever
/// the count, which is the design deciding one way rather than adapting.
#[derive(Clone, Debug, PartialEq)]
pub struct NavigationBar {
    pub destination_count: usize,
    pub selected_index: usize,
    /// `None` is 500ms.
    ///
    /// **Not from the theme.** Upstream reads
    /// `animationDuration ?? const Duration(milliseconds: 500)` and
    /// `NavigationBarThemeData` has no duration field, so there is nothing in
    /// between to consult. This used to claim the theme supplied it and gave a
    /// default for a step that exists on neither side.
    pub animation_duration_ms: Option<u32>,
}

impl NavigationBar {
    /// This bar's appearance, with the theme and the defaults folded in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedNavigationBar {
        crate::component_themes::ResolvedNavigationBar::of(context, self)
    }

    pub const DEFAULT_ANIMATION_MS: u32 = 500;

    pub fn new(destination_count: usize, selected_index: usize) -> NavigationBar {
        NavigationBar {
            destination_count,
            selected_index,
            animation_duration_ms: None,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.destination_count < 2 {
            return Err("destinations.length must be at least two");
        }
        if self.selected_index >= self.destination_count {
            return Err("selectedIndex must be a valid index into destinations");
        }
        Ok(())
    }

    /// Whether a destination's label is drawn. Always -- there is no shifting
    /// mode to hide it in.
    pub fn shows_label(&self, _index: usize) -> bool {
        true
    }
}

/// Upstream `BottomAppBar`.
///
/// Not navigation: a bar of **actions**, and the reason it is a separate class
/// is the notch. A floating action button docked over this bar has a hole cut
/// for it, and the hole is the bar's job because only the bar knows its own
/// outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BottomAppBar {
    /// `None` gives a rectangle with no notch. A bar with no floating button
    /// over it has nothing to cut around.
    pub has_notched_shape: bool,
    /// The gap left between the button and the edge of the hole, so the two do
    /// not touch.
    pub notch_margin: f32,
    /// Material 3 pads its child; Material 2 does not.
    pub material3: bool,
}

impl BottomAppBar {
    pub const DEFAULT_NOTCH_MARGIN: f32 = 4.0;
    /// Upstream's Material 3 default padding, as `(horizontal, vertical)`.
    pub const M3_PADDING: (f32, f32) = (16.0, 12.0);

    pub fn new() -> BottomAppBar {
        BottomAppBar {
            has_notched_shape: false,
            notch_margin: BottomAppBar::DEFAULT_NOTCH_MARGIN,
            material3: true,
        }
    }

    pub fn with_notch(mut self) -> Self {
        self.has_notched_shape = true;
        self
    }

    /// Whether a hole is cut in the bar's outline.
    pub fn cuts_a_notch(&self) -> bool {
        self.has_notched_shape
    }

    pub fn default_padding(&self) -> (f32, f32) {
        if self.material3 {
            BottomAppBar::M3_PADDING
        } else {
            (0.0, 0.0)
        }
    }
}

impl Default for BottomAppBar {
    fn default() -> Self {
        BottomAppBar::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The count decides the layout ------------------------------------------

    #[test]
    fn the_layout_changes_because_the_room_ran_out() {
        // Fixed for three or fewer, shifting for four or more. Nobody chose;
        // there is simply not width for four labels across a phone.
        for count in 2..=3 {
            assert_eq!(
                BottomNavigationBar::new(count, 0).effective_type(None),
                BottomNavigationBarType::Fixed,
                "{count} items"
            );
        }
        for count in 4..=6 {
            assert_eq!(
                BottomNavigationBar::new(count, 0).effective_type(None),
                BottomNavigationBarType::Shifting,
                "{count} items"
            );
        }
    }

    #[test]
    fn saying_so_outright_or_theming_it_overrules_the_count() {
        let five = BottomNavigationBar {
            bar_type: Some(BottomNavigationBarType::Fixed),
            ..BottomNavigationBar::new(5, 0)
        };
        assert_eq!(five.effective_type(None), BottomNavigationBarType::Fixed);

        let themed = BottomNavigationBar::new(2, 0);
        assert_eq!(
            themed.effective_type(Some(BottomNavigationBarType::Shifting)),
            BottomNavigationBarType::Shifting
        );
        assert_eq!(
            five.effective_type(Some(BottomNavigationBarType::Shifting)),
            BottomNavigationBarType::Fixed,
            "and the widget beats the theme"
        );
    }

    #[test]
    fn shifting_writes_only_the_selected_label_and_fixed_writes_them_all() {
        let bar = BottomNavigationBar::new(5, 2);
        for index in 0..5 {
            assert!(bar.shows_label(index, BottomNavigationBarType::Fixed));
        }
        assert!(bar.shows_label(2, BottomNavigationBarType::Shifting));
        assert!(!bar.shows_label(0, BottomNavigationBarType::Shifting));
    }

    // -- What the constructor refuses --------------------------------------------

    #[test]
    fn a_bar_with_one_destination_is_not_navigation() {
        assert!(BottomNavigationBar::new(1, 0).validate().is_err());
        assert_eq!(BottomNavigationBar::new(2, 0).validate(), Ok(()));
        assert!(NavigationBar::new(1, 0).validate().is_err());
    }

    #[test]
    fn hiding_a_label_is_not_removing_it() {
        // Even in shifting mode, where it is not drawn, it is still what a
        // screen reader reads out. An item with none would be an unnamed
        // button.
        let mut bar = BottomNavigationBar::new(5, 0);
        assert_eq!(bar.validate(), Ok(()));
        bar.all_items_labelled = false;
        assert!(bar.validate().is_err());
    }

    #[test]
    fn the_selected_index_has_to_point_at_something() {
        assert!(BottomNavigationBar::new(3, 3).validate().is_err());
        assert_eq!(BottomNavigationBar::new(3, 2).validate(), Ok(()));
        assert!(NavigationBar::new(3, 3).validate().is_err());
    }

    #[test]
    fn two_names_for_one_colour_cannot_both_be_given() {
        let mut bar = BottomNavigationBar::new(3, 0);
        bar.has_selected_item_color = true;
        assert_eq!(bar.validate(), Ok(()));
        bar.has_fixed_color = true;
        assert!(bar.validate().is_err());
    }

    // -- What Material 3 dropped ---------------------------------------------------

    #[test]
    fn the_material_three_bar_has_no_shifting_mode_to_hide_a_label_in() {
        // The design decided one way rather than adapting to the count.
        let crowded = NavigationBar::new(6, 0);
        assert_eq!(crowded.validate(), Ok(()));
        for index in 0..6 {
            assert!(crowded.shows_label(index));
        }
    }

    #[test]
    fn its_animation_falls_back_to_the_themes_half_second() {
        assert_eq!(NavigationBar::new(3, 0).animation_duration_ms, None);
        assert_eq!(NavigationBar::DEFAULT_ANIMATION_MS, 500);
    }

    // -- The bar that holds actions ---------------------------------------------------

    #[test]
    fn a_bar_with_no_floating_button_over_it_has_nothing_to_cut_around() {
        let plain = BottomAppBar::new();
        assert!(!plain.cuts_a_notch());
        assert!(BottomAppBar::new().with_notch().cuts_a_notch());
    }

    #[test]
    fn the_notch_margin_keeps_the_button_off_the_edge_of_its_own_hole() {
        assert_eq!(BottomAppBar::new().notch_margin, 4.0);
        assert!(BottomAppBar::new().notch_margin > 0.0);
    }

    #[test]
    fn material_three_pads_the_child_and_material_two_does_not() {
        assert_eq!(BottomAppBar::new().default_padding(), (16.0, 12.0));

        let mut m2 = BottomAppBar::new();
        m2.material3 = false;
        assert_eq!(m2.default_padding(), (0.0, 0.0));
    }

    // -- Landscape --------------------------------------------------------------------

    #[test]
    fn spreading_five_items_across_a_landscape_phone_puts_them_out_of_reach() {
        // Which is what the centred layout is for.
        assert_eq!(
            BottomNavigationBarLandscapeLayout::default(),
            BottomNavigationBarLandscapeLayout::Spread
        );
        assert_ne!(
            BottomNavigationBarLandscapeLayout::Centered,
            BottomNavigationBarLandscapeLayout::Spread
        );
        assert_ne!(
            BottomNavigationBarLandscapeLayout::Linear,
            BottomNavigationBarLandscapeLayout::Centered
        );
    }
}

#[cfg(test)]
mod bottom_bar_theme_tests {
    use super::*;
    use crate::component_themes::{
        BottomNavigationBarTheme, BottomNavigationBarThemeData, ResolvedBottomNavigationBar,
    };
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, provide};

    struct Reader {
        bar: BottomNavigationBar,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedBottomNavigationBar>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.bar.resolved(context));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(
        bar: BottomNavigationBar,
        data: BottomNavigationBarThemeData,
    ) -> ResolvedBottomNavigationBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            BottomNavigationBarTheme::new(
                data,
                component(Reader {
                    bar,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn the_unselected_label_default_is_computed_from_the_type_and_the_selected_one_is_not() {
        // The asymmetry is the design: the selected label tells the reader
        // where they are and is never hidden; the unselected ones are hidden
        // exactly when there is no room, which is what shifting means.
        let three = resolve(
            BottomNavigationBar::new(3, 0),
            BottomNavigationBarThemeData::new(),
        );
        assert_eq!(three.bar_type, BottomNavigationBarType::Fixed);
        assert!(three.show_selected_labels);
        assert!(three.show_unselected_labels, "fixed: there is room");

        let four = resolve(
            BottomNavigationBar::new(4, 0),
            BottomNavigationBarThemeData::new(),
        );
        assert_eq!(four.bar_type, BottomNavigationBarType::Shifting);
        assert!(four.show_selected_labels, "still never hidden");
        assert!(!four.show_unselected_labels, "shifting: there is not");
    }

    #[test]
    fn a_theme_that_asks_for_shifting_changes_what_the_labels_do_without_touching_them() {
        // The default is computed from the *resolved* type, so a theme that
        // only set the type has moved the labels too.
        let mut data = BottomNavigationBarThemeData::new();
        data.bar_type = Some(BottomNavigationBarType::Shifting);
        let bar = resolve(BottomNavigationBar::new(3, 0), data);
        assert_eq!(bar.bar_type, BottomNavigationBarType::Shifting);
        assert!(
            !bar.show_unselected_labels,
            "three items, and still no unselected labels"
        );
    }

    #[test]
    fn the_widgets_own_type_beats_the_themes() {
        let mut data = BottomNavigationBarThemeData::new();
        data.bar_type = Some(BottomNavigationBarType::Shifting);
        let mut bar = BottomNavigationBar::new(4, 0);
        bar.bar_type = Some(BottomNavigationBarType::Fixed);
        let resolved = resolve(bar, data);
        assert_eq!(resolved.bar_type, BottomNavigationBarType::Fixed);
        assert!(
            resolved.show_unselected_labels,
            "and the labels follow the type that won"
        );
    }

    #[test]
    fn saying_so_outright_beats_the_computed_default() {
        let mut bar = BottomNavigationBar::new(4, 0);
        bar.show_unselected_labels = Some(true);
        assert!(
            resolve(bar, BottomNavigationBarThemeData::new()).show_unselected_labels,
            "shifting would have hidden them"
        );

        let mut bar = BottomNavigationBar::new(3, 0);
        bar.show_selected_labels = Some(false);
        assert!(!resolve(bar, BottomNavigationBarThemeData::new()).show_selected_labels);
    }

    #[test]
    fn the_theme_sits_between_the_widget_and_the_computed_default() {
        let mut data = BottomNavigationBarThemeData::new();
        data.show_unselected_labels = Some(true);
        // Four items would compute false; the theme says otherwise.
        assert!(resolve(BottomNavigationBar::new(4, 0), data.clone()).show_unselected_labels);

        let mut bar = BottomNavigationBar::new(4, 0);
        bar.show_unselected_labels = Some(false);
        assert!(
            !resolve(bar, data).show_unselected_labels,
            "and the widget over it"
        );
    }

    #[test]
    fn the_widget_beats_the_theme_on_the_selected_label_too() {
        // Both sides set and *disagreeing*: the theme's own tests set one side
        // at a time, which shows that something comes through and not which
        // side it came from. `tools/order_sweep.py` found this one.
        let mut data = BottomNavigationBarThemeData::new();
        data.show_selected_labels = Some(false);
        assert!(
            !resolve(BottomNavigationBar::new(3, 0), data.clone()).show_selected_labels,
            "the theme's, with the widget silent"
        );

        let mut bar = BottomNavigationBar::new(3, 0);
        bar.show_selected_labels = Some(true);
        assert!(
            resolve(bar, data).show_selected_labels,
            "and the widget over it"
        );
    }

    #[test]
    fn nothing_is_invented_for_the_colours_upstream_leaves_null() {
        // The widget falls back to the primary and to the caption colour;
        // a colour made up here is one it could not tell from an answer.
        let resolved = resolve(
            BottomNavigationBar::new(3, 0),
            BottomNavigationBarThemeData::new(),
        );
        assert_eq!(resolved.selected_item_color, None);
        assert_eq!(resolved.unselected_item_color, None);
        assert_eq!(resolved.background_color, None);
    }

    #[test]
    fn the_defaults_are_upstreams() {
        let resolved = resolve(
            BottomNavigationBar::new(3, 0),
            BottomNavigationBarThemeData::new(),
        );
        assert_eq!(resolved.elevation, 8.0);
        assert!(resolved.enable_feedback);
        assert_eq!(
            resolved.landscape_layout,
            BottomNavigationBarLandscapeLayout::Spread
        );
    }
}

#[cfg(test)]
mod navigation_bar_theme_tests {
    use super::*;
    use crate::component_themes::{
        NavigationBarTheme, NavigationBarThemeData, NavigationDestinationLabelBehavior,
        ResolvedNavigationBar,
    };
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, provide};

    struct Reader {
        bar: NavigationBar,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedNavigationBar>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.bar.resolved(context));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(bar: NavigationBar, data: NavigationBarThemeData) -> ResolvedNavigationBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            NavigationBarTheme::new(
                data,
                component(Reader {
                    bar,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn the_duration_is_the_one_field_the_theme_has_no_say_in() {
        // Upstream reads `animationDuration ?? 500ms` and the theme has no
        // duration field at all -- two steps where every other field has
        // three. This port's doc used to claim the theme supplied it.
        let plain = resolve(NavigationBar::new(3, 0), NavigationBarThemeData::new());
        assert_eq!(plain.animation_duration_ms, 500);

        let mut bar = NavigationBar::new(3, 0);
        bar.animation_duration_ms = Some(120);
        assert_eq!(
            resolve(bar, NavigationBarThemeData::new()).animation_duration_ms,
            120,
            "and the widget's own is the only thing that moves it"
        );
    }

    #[test]
    fn every_other_field_does_go_through_the_theme() {
        // The contrast that makes the duration worth remarking on.
        let mut data = NavigationBarThemeData::new();
        data.height = Some(64.0);
        data.elevation = Some(9.0);
        let resolved = resolve(NavigationBar::new(3, 0), data);
        assert_eq!(resolved.height, 64.0);
        assert_eq!(resolved.elevation, 9.0);
    }

    #[test]
    fn the_label_behaviour_is_a_constant_where_the_older_bars_was_computed() {
        // The M3 bar does not shift, so there is no count at which the labels
        // stop fitting -- `BottomNavigationBar` had to work its default out
        // from the item count and this one does not.
        for count in [2, 3, 4, 7] {
            assert_eq!(
                resolve(NavigationBar::new(count, 0), NavigationBarThemeData::new()).label_behavior,
                NavigationDestinationLabelBehavior::AlwaysShow
            );
        }
    }

    #[test]
    fn a_theme_can_still_ask_for_the_labels_to_come_and_go() {
        let mut data = NavigationBarThemeData::new();
        data.label_behavior = Some(NavigationDestinationLabelBehavior::OnlyShowSelected);
        assert_eq!(
            resolve(NavigationBar::new(3, 0), data).label_behavior,
            NavigationDestinationLabelBehavior::OnlyShowSelected
        );
    }

    #[test]
    fn the_default_height_is_the_indicators_height() {
        // A bar height chosen independently would leave the indicator floating
        // in it or clipped by it.
        assert_eq!(
            resolve(NavigationBar::new(3, 0), NavigationBarThemeData::new()).height,
            ResolvedNavigationBar::HEIGHT
        );
        assert_eq!(ResolvedNavigationBar::HEIGHT, 32.0);
    }

    #[test]
    fn nothing_is_invented_for_the_colours_upstream_leaves_null() {
        let resolved = resolve(NavigationBar::new(3, 0), NavigationBarThemeData::new());
        assert_eq!(resolved.background_color, None);
        assert_eq!(resolved.indicator_color, None);
        assert_eq!(resolved.shadow_color, None);
        assert_eq!(resolved.label_padding, crate::render::EdgeInsets::ZERO);
    }
}
