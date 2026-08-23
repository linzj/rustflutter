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
#[derive(Clone, Debug, PartialEq)]
pub struct BottomAppBar {
    /// The outline the notch is cut from, or `None` to defer.
    ///
    /// `None` does **not** mean "no notch": under Material 3 the chain
    /// continues to `_BottomAppBarDefaultsM3.shape`, which is an
    /// `AutomaticNotchedShape`, so a bar nobody configured still carries one.
    /// Whether a hole is actually cut needs a floating action button as well --
    /// see [`crate::component_themes::ResolvedBottomAppBar::cuts_a_notch`].
    ///
    /// This used to be a `bool` defaulting to false, with a `cuts_a_notch`
    /// that answered from it alone. That was wrong twice over: it said a
    /// default Material 3 bar never notches, where upstream's always has a
    /// shape, and it never looked for the button, where upstream cuts nothing
    /// without one.
    pub shape: Option<crate::borders::NotchedShape>,
    /// The gap left between the button and the edge of the hole, so the two do
    /// not touch.
    pub notch_margin: f32,
}

impl BottomAppBar {
    pub const DEFAULT_NOTCH_MARGIN: f32 = 4.0;
    /// Upstream's Material 3 default padding, as `(horizontal, vertical)`.
    pub const M3_PADDING: (f32, f32) = (16.0, 12.0);

    pub fn new() -> BottomAppBar {
        BottomAppBar {
            shape: None,
            notch_margin: BottomAppBar::DEFAULT_NOTCH_MARGIN,
        }
    }

    /// Upstream's usual shape, `CircularNotchedRectangle`.
    pub fn with_notch(mut self) -> Self {
        self.shape = Some(crate::borders::NotchedShape::Circular { inverted: false });
        self
    }

    /// This bar's appearance, with the theme and the defaults folded in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedBottomAppBar {
        crate::component_themes::ResolvedBottomAppBar::of(context, self)
    }

    /// Upstream's inline padding default, which reads
    /// [`crate::theme::ThemeData::use_material3`] rather than anything on the
    /// bar.
    pub fn default_padding(use_material3: bool) -> (f32, f32) {
        if use_material3 {
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
    fn its_animation_falls_back_to_a_half_second_with_no_theme_in_between() {
        assert_eq!(NavigationBar::new(3, 0).animation_duration_ms, None);
        assert_eq!(NavigationBar::DEFAULT_ANIMATION_MS, 500);
    }

    // -- The bar that holds actions ---------------------------------------------------

    #[test]
    fn asking_for_a_notch_names_upstreams_usual_shape() {
        // What the widget can say on its own. Whether a hole is cut is a
        // question for the resolution and the Scaffold -- this test used to be
        // called "a bar with no floating button over it has nothing to cut
        // around" while checking a flag that had never heard of a button.
        assert_eq!(BottomAppBar::new().shape, None);
        assert_eq!(
            BottomAppBar::new().with_notch().shape,
            Some(crate::borders::NotchedShape::Circular { inverted: false })
        );
    }

    #[test]
    fn the_notch_margin_keeps_the_button_off_the_edge_of_its_own_hole() {
        assert_eq!(BottomAppBar::new().notch_margin, 4.0);
        assert!(BottomAppBar::new().notch_margin > 0.0);
    }

    #[test]
    fn material_three_pads_the_child_and_material_two_does_not() {
        assert_eq!(BottomAppBar::default_padding(true), (16.0, 12.0));
        assert_eq!(BottomAppBar::default_padding(false), (0.0, 0.0));
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
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component};

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
        // No `Theme` above it: `BottomNavigationBarTheme::of` falls back to
        // `ThemeData::of`, which has its own fallback. Wrapping one here would
        // suggest it took part in the answer.
        tree.rebuild(BottomNavigationBarTheme::new(
            data,
            component(Reader {
                bar,
                seen: std::rc::Rc::clone(&seen),
            }),
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
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component};

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
        // No `Theme` above it: `NavigationBarTheme::of` falls back to
        // `ThemeData::of`, which has its own fallback. Wrapping one here would
        // suggest it took part in the answer.
        tree.rebuild(NavigationBarTheme::new(
            data,
            component(Reader {
                bar,
                seen: std::rc::Rc::clone(&seen),
            }),
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

#[cfg(test)]
mod bottom_app_bar_theme_tests {
    use super::*;
    use crate::EdgeInsetsGeometry;
    use crate::borders::NotchedShape;
    use crate::component_themes::{BottomAppBarTheme, BottomAppBarThemeData, ResolvedBottomAppBar};
    use crate::engine::Color;
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, leaf};
    use crate::render::EdgeInsets;
    use crate::theme::ThemeData;
    use crate::widgets::SizedBox;

    struct Reader {
        bar: BottomAppBar,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedBottomAppBar>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.bar.resolved(context));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn resolve(bar: BottomAppBar, data: BottomAppBarThemeData) -> ResolvedBottomAppBar {
        resolve_under(ThemeData::fallback(), bar, data)
    }

    fn resolve_under(
        theme: ThemeData,
        bar: BottomAppBar,
        data: BottomAppBarThemeData,
    ) -> ResolvedBottomAppBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::provide(
            theme,
            BottomAppBarTheme::new(
                data,
                component(Reader {
                    bar,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// A theme in the Material 2 mode, which is where the branch lives now.
    fn m2_theme() -> ThemeData {
        ThemeData {
            use_material3: false,
            ..ThemeData::fallback()
        }
    }

    #[test]
    fn the_elevation_is_an_input_to_the_colour_and_not_only_to_the_shadow() {
        // `effectiveColor` is `applySurfaceTint(color, tint, elevation)`. The
        // resolved colour is never what gets painted.
        let tint = Color(0xFF00FF00);
        let mut data = BottomAppBarThemeData::new();
        data.surface_tint_color = Some(tint);
        data.color = Some(Color(0xFF000000));

        let mut low = data.clone();
        low.elevation = Some(0.0);
        let mut high = data.clone();
        high.elevation = Some(24.0);

        let scheme = ThemeData::fallback().color_scheme;
        let painted = |data: BottomAppBarThemeData| {
            let resolved = resolve(BottomAppBar::new(), data);
            resolved.effective_color(false, scheme.surface, scheme.on_surface)
        };
        assert_ne!(
            painted(low.clone()),
            painted(high.clone()),
            "same colour, same tint, different elevation -- different paint"
        );
        assert_eq!(
            resolve(BottomAppBar::new(), low).color,
            resolve(BottomAppBar::new(), high).color,
            "and the resolved colour itself did not move, which is the point"
        );
    }

    #[test]
    fn neither_default_tints_anything_and_they_fail_to_for_opposite_reasons() {
        let scheme = ThemeData::fallback().color_scheme;

        // Material 3 consults the tint and defaults it to transparent.
        let m3 = resolve(BottomAppBar::new(), BottomAppBarThemeData::new());
        assert_eq!(m3.surface_tint_color, Color::TRANSPARENT);
        assert_eq!(
            m3.effective_color(false, scheme.surface, scheme.on_surface),
            m3.color,
            "a transparent tint is short-circuited"
        );

        // Material 2 resolves a real scheme colour and takes the branch that
        // never looks at it.
        let two = resolve_under(
            m2_theme(),
            BottomAppBar::new(),
            BottomAppBarThemeData::new(),
        );
        assert_eq!(two.surface_tint_color, scheme.surface_tint());
        assert_ne!(two.surface_tint_color, Color::TRANSPARENT);
        let mut tinted = BottomAppBarThemeData::new();
        tinted.surface_tint_color = Some(Color(0xFFFF0000));
        assert_eq!(
            resolve_under(m2_theme(), BottomAppBar::new(), tinted).effective_color(
                false,
                scheme.surface,
                scheme.on_surface
            ),
            two.effective_color(false, scheme.surface, scheme.on_surface),
            "a different tint entirely, and Material 2 paints the same"
        );
    }

    #[test]
    fn material_two_leaves_the_height_to_the_child_and_material_three_pins_it() {
        assert_eq!(
            resolve(BottomAppBar::new(), BottomAppBarThemeData::new()).height,
            Some(80.0)
        );
        assert_eq!(
            resolve_under(
                m2_theme(),
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .height,
            None,
            "`SizedBox(height: null)` is as tall as what is in it"
        );
    }

    #[test]
    fn a_material_three_bar_carries_a_notch_nobody_asked_for() {
        // The finding that corrected this port: the widget's shape defaults to
        // null, and the chain does not stop there.
        let plain = resolve(BottomAppBar::new(), BottomAppBarThemeData::new());
        assert!(matches!(plain.shape, Some(NotchedShape::Automatic { .. })));
        assert_eq!(
            resolve_under(
                m2_theme(),
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .shape,
            None,
            "and a Material 2 bar does not"
        );
    }

    #[test]
    fn carrying_a_shape_is_not_cutting_a_hole() {
        // Upstream's `notchedShape != null && hasFab`. With no floating action
        // button the clipper is a plain rounded rectangle, whatever shape
        // resolved.
        let plain = resolve(BottomAppBar::new(), BottomAppBarThemeData::new());
        assert!(plain.shape.is_some());
        assert!(!plain.cuts_a_notch(false));
        assert!(plain.cuts_a_notch(true));

        // And a button with nothing to cut into is equally not a notch.
        let two = resolve_under(
            m2_theme(),
            BottomAppBar::new(),
            BottomAppBarThemeData::new(),
        );
        assert!(!two.cuts_a_notch(true));
    }

    #[test]
    fn the_widget_is_the_first_step_for_the_shape_and_the_theme_the_second() {
        let mine = NotchedShape::Circular { inverted: true };
        let mut data = BottomAppBarThemeData::new();
        data.shape = Some(NotchedShape::Circular { inverted: false });
        assert_eq!(
            resolve(
                BottomAppBar {
                    shape: Some(mine.clone()),
                    ..BottomAppBar::new()
                },
                data.clone()
            )
            .shape,
            Some(mine)
        );
        assert_eq!(
            resolve(BottomAppBar::new(), data).shape,
            Some(NotchedShape::Circular { inverted: false })
        );
    }

    #[test]
    fn the_two_elevations_and_the_two_colours_are_not_the_same_numbers() {
        let scheme = ThemeData::fallback().color_scheme;
        let three = resolve(BottomAppBar::new(), BottomAppBarThemeData::new());
        let two = resolve_under(
            m2_theme(),
            BottomAppBar::new(),
            BottomAppBarThemeData::new(),
        );
        assert_eq!(three.elevation, 3.0);
        assert_eq!(two.elevation, 8.0);
        assert_eq!(three.color, scheme.surface_container());
        assert_eq!(
            two.color,
            ResolvedBottomAppBar::M2_LIGHT,
            "Material 2 is plain white in the light, from before the scheme"
        );
        assert_eq!(three.shadow_color, Color::TRANSPARENT);
        assert_eq!(two.shadow_color, Color(0xFF000000));
    }

    #[test]
    fn material_twos_colour_is_the_only_thing_here_that_reads_the_brightness() {
        // A mutation deleting this branch survived: nothing built under a dark
        // theme, so the arm was unreachable and the test suite could not tell
        // it from an empty one.
        assert_eq!(
            resolve_under(
                ThemeData {
                    use_material3: false,
                    ..ThemeData::dark()
                },
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .color,
            ResolvedBottomAppBar::M2_DARK,
            "`Colors.grey[800]`"
        );
        assert_eq!(
            resolve_under(
                ThemeData {
                    use_material3: false,
                    ..ThemeData::light()
                },
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .color,
            ResolvedBottomAppBar::M2_LIGHT
        );

        // Material 3 takes its colour from the scheme, which the brightness
        // has already moved -- it does not look at the brightness itself.
        let dark = resolve_under(
            ThemeData::dark(),
            BottomAppBar::new(),
            BottomAppBarThemeData::new(),
        );
        assert_eq!(
            dark.color,
            ThemeData::dark().color_scheme.surface_container()
        );
        assert_ne!(dark.color, ResolvedBottomAppBar::M2_DARK);
    }

    #[test]
    fn the_padding_default_lives_at_the_use_site_and_still_has_a_theme_step() {
        assert_eq!(
            resolve(BottomAppBar::new(), BottomAppBarThemeData::new()).padding,
            EdgeInsets::symmetric(16.0, 12.0)
        );
        assert_eq!(
            resolve_under(
                m2_theme(),
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .padding,
            EdgeInsets::ZERO
        );
        let mut data = BottomAppBarThemeData::new();
        data.padding = Some(EdgeInsetsGeometry::Absolute(EdgeInsets::all(7.0)));
        assert_eq!(
            resolve(BottomAppBar::new(), data).padding,
            EdgeInsets::all(7.0),
            "a theme still gets its say even though the default is written elsewhere"
        );
    }

    #[test]
    fn material_twos_overlay_only_fires_in_the_dark_and_only_on_the_surface() {
        // The two transforms are not two spellings of one idea.
        let scheme = ThemeData::fallback().color_scheme;
        let mut data = BottomAppBarThemeData::new();
        data.color = Some(scheme.surface);
        let two = resolve_under(m2_theme(), BottomAppBar::new(), data.clone());
        assert_eq!(
            two.effective_color(false, scheme.surface, scheme.on_surface),
            two.color,
            "in the light it does nothing at all"
        );
        assert_ne!(
            two.effective_color(true, scheme.surface, scheme.on_surface),
            two.color,
            "and in the dark it lightens the surface by its elevation"
        );

        let mut mine = BottomAppBarThemeData::new();
        mine.color = Some(Color(0xFF123456));
        let hand_coloured = resolve_under(m2_theme(), BottomAppBar::new(), mine);
        assert_eq!(
            hand_coloured.effective_color(true, scheme.surface, scheme.on_surface),
            hand_coloured.color,
            "a colour someone chose is left alone even in the dark"
        );
    }
}
