// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The three places Material 3 puts navigation destinations, and the pill
//! that marks the chosen one.
//!
//! Upstream spreads these across three files -- `navigation_bar.dart`,
//! `navigation_drawer.dart` and `navigation_rail.dart` -- because each sits
//! beside the surface that holds it. They are together here because what is
//! worth knowing about them is how they *differ*, and that only shows when
//! they are side by side:
//!
//! * A **bar** destination's label is a `String`; the other two take a widget.
//!   A bar's label is always one short word under an icon, and upstream types
//!   it accordingly.
//! * A **rail** destination's `selectedIcon` defaults to its `icon`; the other
//!   two leave it null. A rail is narrow and always visible, so a destination
//!   with no selected icon still has to draw something when chosen -- the
//!   other two have room to fall back on the label.
//! * A **drawer** destination is a full row with a background colour of its
//!   own, because a drawer's destinations are a list and a list row can be
//!   tinted; a bar's cannot.
//!
//! The [`NavigationIndicator`] is the same pill in all three.

use crate::borders::BorderRadius;
use crate::engine::Color;
use crate::framework::AnyWidget;
use crate::render::EdgeInsets;

/// Upstream `NavigationDestination` (`material/navigation_bar.dart`): one
/// destination of a bottom navigation bar.
pub struct NavigationDestination {
    pub icon: std::cell::RefCell<Option<AnyWidget>>,
    /// Shown instead of `icon` while this destination is the chosen one --
    /// usually the filled version of the same glyph. `None` means the same
    /// icon serves both, which is what a bar can afford because the label and
    /// the indicator pill already say which is chosen.
    pub selected_icon: std::cell::RefCell<Option<AnyWidget>>,
    /// A `String`, not a widget: a bar's label is one short word under an
    /// icon, and upstream types it that way.
    pub label: String,
    pub tooltip: Option<String>,
    pub enabled: bool,
}

impl NavigationDestination {
    pub fn new(icon: AnyWidget, label: impl Into<String>) -> NavigationDestination {
        NavigationDestination {
            icon: std::cell::RefCell::new(Some(icon)),
            selected_icon: std::cell::RefCell::new(None),
            label: label.into(),
            tooltip: None,
            enabled: true,
        }
    }

    pub fn with_selected_icon(self, icon: AnyWidget) -> Self {
        *self.selected_icon.borrow_mut() = Some(icon);
        self
    }

    /// Upstream's `tooltip`. Unset, a bar shows the *label* as the tooltip;
    /// an empty string is how a caller says "no tooltip at all", which is a
    /// third state a bare `Option` would lose.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// What the bar shows on hover: the tooltip if one was given, the label
    /// otherwise, and nothing at all for an empty tooltip.
    pub fn effective_tooltip(&self) -> Option<String> {
        match &self.tooltip {
            Some(tooltip) if tooltip.is_empty() => None,
            Some(tooltip) => Some(tooltip.clone()),
            None => Some(self.label.clone()),
        }
    }
}

/// Upstream `NavigationDrawerDestination`: one row of a navigation drawer.
pub struct NavigationDrawerDestination {
    pub icon: std::cell::RefCell<Option<AnyWidget>>,
    pub selected_icon: std::cell::RefCell<Option<AnyWidget>>,
    /// A widget rather than a string, unlike the bar's: a drawer row is wide
    /// enough for a label that is more than one word, and often for a badge
    /// beside it.
    pub label: std::cell::RefCell<Option<AnyWidget>>,
    /// The row's own colour. A drawer's destinations are a list and a list row
    /// can be tinted; a bar's destinations share one surface and cannot.
    pub background_color: Option<Color>,
    pub enabled: bool,
}

impl NavigationDrawerDestination {
    pub fn new(icon: AnyWidget, label: AnyWidget) -> NavigationDrawerDestination {
        NavigationDrawerDestination {
            icon: std::cell::RefCell::new(Some(icon)),
            selected_icon: std::cell::RefCell::new(None),
            label: std::cell::RefCell::new(Some(label)),
            background_color: None,
            enabled: true,
        }
    }

    pub fn with_selected_icon(self, icon: AnyWidget) -> Self {
        *self.selected_icon.borrow_mut() = Some(icon);
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Upstream `NavigationRailDestination` (`material/navigation_rail.dart`).
pub struct NavigationRailDestination {
    pub icon: std::cell::RefCell<Option<AnyWidget>>,
    /// **Not optional here, unlike the other two.** Upstream's constructor is
    /// `selectedIcon = selectedIcon ?? icon`, so a rail destination always has
    /// one. A rail is narrow and always on screen, so a chosen destination
    /// still has to draw *something* -- where a bar or a drawer has room to
    /// let the label and the pill carry it.
    pub selected_icon: std::cell::RefCell<Option<AnyWidget>>,
    pub label: std::cell::RefCell<Option<AnyWidget>>,
    pub indicator_color: Option<Color>,
    pub padding: Option<EdgeInsets>,
    /// Upstream calls this `disabled`, not `enabled` -- the one of the three
    /// that is negative, and the default is therefore `false`. Kept as
    /// upstream spells it so a reader comparing the two files is not left
    /// wondering which way round it is.
    pub disabled: bool,
}

impl NavigationRailDestination {
    /// `selected_icon` defaults to `icon`, which is upstream's constructor
    /// doing the work rather than the builder.
    pub fn new(icon: AnyWidget, label: AnyWidget) -> NavigationRailDestination {
        NavigationRailDestination {
            icon: std::cell::RefCell::new(Some(icon)),
            selected_icon: std::cell::RefCell::new(None),
            label: std::cell::RefCell::new(Some(label)),
            indicator_color: None,
            padding: None,
            disabled: false,
        }
    }

    pub fn with_selected_icon(self, icon: AnyWidget) -> Self {
        *self.selected_icon.borrow_mut() = Some(icon);
        self
    }

    pub fn with_indicator_color(mut self, color: Color) -> Self {
        self.indicator_color = Some(color);
        self
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Whether a distinct selected icon was given. Upstream's field is never
    /// null, so this is the question its constructor answers and then throws
    /// away; kept because "does this destination change glyph when chosen"
    /// is a real question and the field alone can no longer answer it.
    pub fn has_distinct_selected_icon(&self) -> bool {
        self.selected_icon.borrow().is_some()
    }
}

/// Upstream `NavigationIndicator`: the pill behind the chosen destination's
/// icon.
///
/// The same shape in all three surfaces, which is the point -- a reader who
/// learns what the pill means in the bottom bar knows it in the drawer. Its
/// defaults are a 64 by 32 rounded rectangle with a 16 radius, which is to
/// say a **stadium**: the radius is exactly half the height, so the ends are
/// semicircles. Upstream writes 16 rather than naming the stadium, and the
/// two stay equal only by hand -- so a change to the height that forgot the
/// radius would quietly square the ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationIndicator {
    pub color: Option<Color>,
    pub width: f32,
    pub height: f32,
    pub border_radius: BorderRadius,
}

impl NavigationIndicator {
    /// Upstream's `_kIndicatorWidth`.
    pub const DEFAULT_WIDTH: f32 = 64.0;
    /// Upstream's `_kIndicatorHeight`.
    pub const DEFAULT_HEIGHT: f32 = 32.0;
    /// Upstream's default corner radius, which is half the default height.
    pub const DEFAULT_RADIUS: f32 = 16.0;

    pub fn new() -> NavigationIndicator {
        NavigationIndicator {
            color: None,
            width: NavigationIndicator::DEFAULT_WIDTH,
            height: NavigationIndicator::DEFAULT_HEIGHT,
            border_radius: BorderRadius::circular(NavigationIndicator::DEFAULT_RADIUS),
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn with_border_radius(mut self, radius: BorderRadius) -> Self {
        self.border_radius = radius;
        self
    }

    /// Whether the ends are semicircles -- true for the defaults, and the
    /// thing a caller resizing the pill has to keep true by hand.
    pub fn is_stadium(&self) -> bool {
        let radius = self.border_radius.top_left.x;
        (radius - self.height / 2.0).abs() < f32::EPSILON
            && self.border_radius.top_right.x == radius
            && self.border_radius.bottom_left.x == radius
            && self.border_radius.bottom_right.x == radius
    }
}

impl Default for NavigationIndicator {
    fn default() -> NavigationIndicator {
        NavigationIndicator::new()
    }
}

/// One entry of a [`NavigationDrawer`]: either a destination or something
/// else the caller put in the list.
pub enum NavigationDrawerChild {
    Destination(NavigationDrawerDestination),
    /// A header, a divider, a heading -- anything that is not a destination.
    Other(AnyWidget),
}

impl NavigationDrawerChild {
    pub fn is_destination(&self) -> bool {
        matches!(self, NavigationDrawerChild::Destination(_))
    }
}

/// Upstream `NavigationDrawer`.
///
/// # Only destinations are numbered
///
/// The rule that matters: `selectedIndex` counts **destinations**, not
/// children. Upstream walks the children and increments its index only for a
/// `NavigationDrawerDestination`, passing everything else through untouched.
/// So a divider or a heading dropped between two destinations does not shift
/// which one is highlighted -- which is what lets a caller group destinations
/// under headings without renumbering anything.
pub struct NavigationDrawer {
    pub children: std::cell::RefCell<Vec<NavigationDrawerChild>>,
    pub selected_index: Option<usize>,
    pub background_color: Option<Color>,
    pub indicator_color: Option<Color>,
    /// Upstream's default `EdgeInsets.symmetric(horizontal: 12)`: the rows are
    /// inset from the drawer's edges so the indicator pill has somewhere to
    /// sit without touching them.
    pub tile_padding: EdgeInsets,
    #[allow(clippy::type_complexity)]
    pub on_destination_selected: Option<std::rc::Rc<dyn Fn(usize)>>,
}

impl NavigationDrawer {
    /// Upstream's default `tilePadding`.
    pub const TILE_PADDING: EdgeInsets = EdgeInsets::symmetric(12.0, 0.0);

    pub fn new(children: Vec<NavigationDrawerChild>) -> NavigationDrawer {
        NavigationDrawer {
            children: std::cell::RefCell::new(children),
            // Upstream's default is 0, not null: a drawer with no selection
            // named still highlights its first destination, because a
            // navigation surface with nothing chosen does not say where the
            // reader is.
            selected_index: Some(0),
            background_color: None,
            indicator_color: None,
            tile_padding: NavigationDrawer::TILE_PADDING,
            on_destination_selected: None,
        }
    }

    pub fn with_selected_index(mut self, index: Option<usize>) -> Self {
        self.selected_index = index;
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_indicator_color(mut self, color: Color) -> Self {
        self.indicator_color = Some(color);
        self
    }

    pub fn with_tile_padding(mut self, padding: EdgeInsets) -> Self {
        self.tile_padding = padding;
        self
    }

    pub fn with_on_destination_selected(mut self, callback: impl Fn(usize) + 'static) -> Self {
        self.on_destination_selected = Some(std::rc::Rc::new(callback));
        self
    }

    /// This drawer's appearance, with the theme and the M3 defaults folded in
    /// -- except for the surface fields, which have only two steps here; see
    /// [`crate::component_themes::ResolvedNavigationDrawer`].
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedNavigationDrawer {
        crate::component_themes::ResolvedNavigationDrawer::of(context, self)
    }

    /// Upstream's `totalNumberOfDestinations`: how many of the children are
    /// destinations. Passed to each of them upstream so a screen reader can
    /// say "3 of 5".
    pub fn destination_count(&self) -> usize {
        self.children
            .borrow()
            .iter()
            .filter(|child| child.is_destination())
            .count()
    }

    /// The destination index each child carries, or `None` for a child that is
    /// not a destination. Upstream's `destinationIndex++` inside the loop.
    pub fn destination_indices(&self) -> Vec<Option<usize>> {
        let mut next = 0;
        self.children
            .borrow()
            .iter()
            .map(|child| {
                if child.is_destination() {
                    let index = next;
                    next += 1;
                    Some(index)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Which child, by position in the list, is the selected one.
    pub fn selected_child(&self) -> Option<usize> {
        let selected = self.selected_index?;
        self.destination_indices()
            .into_iter()
            .position(|index| index == Some(selected))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::leaf;
    use crate::widgets::Empty;

    fn widget() -> AnyWidget {
        leaf(|| Empty)
    }

    fn destination() -> NavigationDrawerChild {
        NavigationDrawerChild::Destination(NavigationDrawerDestination::new(widget(), widget()))
    }

    fn heading() -> NavigationDrawerChild {
        NavigationDrawerChild::Other(widget())
    }

    #[test]
    fn only_destinations_are_numbered() {
        // The rule that matters. A divider or a heading between two
        // destinations must not shift which one is highlighted, or a caller
        // could not group destinations under headings without renumbering.
        let drawer = NavigationDrawer::new(vec![
            heading(),
            destination(),
            destination(),
            heading(),
            destination(),
        ]);
        assert_eq!(
            drawer.destination_indices(),
            vec![None, Some(0), Some(1), None, Some(2)]
        );
        assert_eq!(drawer.destination_count(), 3);
    }

    #[test]
    fn the_selected_index_finds_its_child_past_the_headings() {
        let drawer =
            NavigationDrawer::new(vec![heading(), destination(), heading(), destination()])
                .with_selected_index(Some(1));
        // The second *destination* is the fourth child.
        assert_eq!(drawer.selected_child(), Some(3));
    }

    #[test]
    fn a_selection_past_the_end_finds_nothing_rather_than_the_last() {
        // Clamping would silently highlight a destination the caller did not
        // name, which is worse than highlighting none: it would look like the
        // drawer worked.
        let drawer =
            NavigationDrawer::new(vec![destination(), destination()]).with_selected_index(Some(9));
        assert_eq!(drawer.selected_child(), None);
    }

    #[test]
    fn a_drawer_highlights_its_first_destination_unless_told_otherwise() {
        // Upstream's default `selectedIndex` is 0 rather than null, because a
        // navigation surface with nothing chosen does not say where the reader
        // is.
        let drawer = NavigationDrawer::new(vec![destination(), destination()]);
        assert_eq!(drawer.selected_index, Some(0));
        assert_eq!(drawer.selected_child(), Some(0));
        // And a caller can still say "none".
        let none = NavigationDrawer::new(vec![destination()]).with_selected_index(None);
        assert_eq!(none.selected_child(), None);
    }

    #[test]
    fn only_the_rail_gives_a_destination_a_selected_icon_by_default() {
        // A rail is narrow and always on screen, so a chosen destination still
        // has to draw something; a bar or a drawer can let the label and the
        // pill carry it. Upstream expresses that in the rail's constructor
        // (`selectedIcon ?? icon`) and nowhere else.
        let bar = NavigationDestination::new(widget(), "Home");
        assert!(bar.selected_icon.borrow().is_none());

        let drawer = NavigationDrawerDestination::new(widget(), widget());
        assert!(drawer.selected_icon.borrow().is_none());

        let rail = NavigationRailDestination::new(widget(), widget());
        assert!(
            !rail.has_distinct_selected_icon(),
            "none was given, so the icon serves both"
        );
        assert!(
            NavigationRailDestination::new(widget(), widget())
                .with_selected_icon(widget())
                .has_distinct_selected_icon()
        );
    }

    #[test]
    fn a_bars_tooltip_falls_back_to_its_label_and_an_empty_one_means_none() {
        // Three states out of one nullable string, which a bare `Option` in
        // the port would have lost: unset means "use the label", empty means
        // "no tooltip", and anything else is itself.
        let plain = NavigationDestination::new(widget(), "Home");
        assert_eq!(plain.effective_tooltip(), Some("Home".to_string()));

        let named = NavigationDestination::new(widget(), "Home").with_tooltip("Go home");
        assert_eq!(named.effective_tooltip(), Some("Go home".to_string()));

        let silent = NavigationDestination::new(widget(), "Home").with_tooltip("");
        assert_eq!(silent.effective_tooltip(), None);
    }

    #[test]
    fn the_rail_spells_its_flag_the_other_way_round() {
        // Upstream calls it `disabled` on the rail and `enabled` on the other
        // two. Kept as upstream spells it, so a reader comparing the files is
        // not left guessing which way round each is -- and pinned, because a
        // port that quietly normalised them would invert one of the defaults.
        assert!(NavigationDestination::new(widget(), "Home").enabled);
        assert!(NavigationDrawerDestination::new(widget(), widget()).enabled);
        assert!(!NavigationRailDestination::new(widget(), widget()).disabled);
    }

    #[test]
    fn the_indicator_is_a_stadium_by_default() {
        // 64 by 32 with a 16 radius: the radius is exactly half the height, so
        // the ends are semicircles. Upstream writes 16 rather than naming the
        // stadium, and the two stay equal only by hand.
        let pill = NavigationIndicator::new();
        assert_eq!(pill.width, 64.0);
        assert_eq!(pill.height, 32.0);
        assert!(pill.is_stadium());

        // Which is what a caller resizing it has to keep true: a taller pill
        // with the old radius has square-ish ends.
        let taller = NavigationIndicator::new().with_size(64.0, 48.0);
        assert!(!taller.is_stadium(), "the radius no longer matches");
        assert!(
            taller
                .with_border_radius(BorderRadius::circular(24.0))
                .is_stadium()
        );
    }
}

#[cfg(test)]
mod navigation_drawer_theme_tests {
    use super::*;
    use crate::component_themes::{
        DrawerTheme, DrawerThemeData, NavigationDrawerTheme, NavigationDrawerThemeData,
        ResolvedDrawer, ResolvedNavigationDrawer,
    };
    use crate::framework::{BuildContext, Component, ElementTree, component, leaf};
    use crate::render::Size;
    use crate::theme::ThemeData;
    use crate::widget_state::{WidgetState, WidgetStates};
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

    /// Build `wrap(reader)` and hand back what the reader saw.
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

    fn drawer() -> NavigationDrawer {
        NavigationDrawer::new(vec![])
    }

    fn resolve(
        drawer: NavigationDrawer,
        data: NavigationDrawerThemeData,
    ) -> ResolvedNavigationDrawer {
        let drawer = std::rc::Rc::new(drawer);
        read_under(
            |child| NavigationDrawerTheme::new(data, child),
            move |context| drawer.resolved(context),
        )
    }

    #[test]
    fn the_surface_fields_stop_after_two_steps() {
        // Upstream writes `backgroundColor ?? theme.backgroundColor` and hands
        // the result -- null included -- to a plain `Drawer`. Filling these in
        // here would be resolving a step that happens in another widget.
        let plain = resolve(drawer(), NavigationDrawerThemeData::new());
        assert_eq!(plain.background_color, None);
        assert_eq!(plain.shadow_color, None);
        assert_eq!(plain.surface_tint_color, None);
        assert_eq!(
            plain.elevation, None,
            "`_NavigationDrawerDefaultsM3` declares elevation 1, and \
             navigation_drawer.dart never reads it"
        );
    }

    #[test]
    fn a_drawer_theme_moves_a_navigation_drawers_background() {
        // The consequence of the two-step chain, and the only way to see it:
        // the third step lives in `DrawerThemeData`, so this is the theme that
        // decides -- even though the widget is a `NavigationDrawer`.
        let mine = Color(0xFF00FF00);
        let mut drawer_theme = DrawerThemeData::default();
        drawer_theme.background_color = Some(mine);

        let surface = read_under(
            move |child| DrawerTheme::new(drawer_theme.clone(), child),
            move |context| {
                std::rc::Rc::new(NavigationDrawer::new(vec![]))
                    .resolved(context)
                    .surface(context)
            },
        );
        assert_eq!(surface.background, mine);
    }

    #[test]
    fn and_a_navigation_drawer_theme_does_not_move_a_plain_drawers() {
        // The other half of the asymmetry. If this passed, the two themes
        // would be interchangeable and the finding would be nothing.
        let mut data = NavigationDrawerThemeData::new();
        data.background_color = Some(Color(0xFF00FF00));
        let surface = read_under(
            move |child| NavigationDrawerTheme::new(data.clone(), child),
            ResolvedDrawer::of,
        );
        assert_ne!(surface.background, Color(0xFF00FF00));
    }

    #[test]
    fn the_widgets_own_background_still_wins_over_the_drawers_theme() {
        // The two-step chain starts at the widget, so a `NavigationDrawer`
        // given a colour keeps it even where a `DrawerTheme` would otherwise
        // have the last word.
        let mine = Color(0xFF0000FF);
        let mut drawer_theme = DrawerThemeData::default();
        drawer_theme.background_color = Some(Color(0xFF00FF00));
        let surface = read_under(
            move |child| DrawerTheme::new(drawer_theme.clone(), child),
            move |context| {
                std::rc::Rc::new(NavigationDrawer::new(vec![]).with_background_color(mine))
                    .resolved(context)
                    .surface(context)
            },
        );
        assert_eq!(surface.background, mine);
    }

    #[test]
    fn the_widget_is_the_first_step_and_the_navigation_drawer_theme_the_second() {
        // `order_sweep.py` found this one: every other test set at most one
        // side of `drawer.background_color.or(data.background_color)`, so
        // swapping the two went unnoticed. Both set, and disagreeing.
        let mine = Color(0xFF112233);
        let mut data = NavigationDrawerThemeData::new();
        data.background_color = Some(Color(0xFF445566));
        assert_eq!(
            resolve(drawer().with_background_color(mine), data.clone()).background_color,
            Some(mine)
        );
        assert_eq!(
            resolve(drawer(), data).background_color,
            Some(Color(0xFF445566)),
            "and with the widget silent the theme is what carries on to the Drawer"
        );
    }

    #[test]
    fn the_indicator_starts_at_the_drawer_and_not_at_the_destination() {
        // `info.indicatorColor` is what the *drawer* was given; a destination
        // has no indicator field to offer.
        let mine = Color(0xFFFF0000);
        let mut data = NavigationDrawerThemeData::new();
        data.indicator_color = Some(Color(0xFF00FF00));
        assert_eq!(
            resolve(drawer().with_indicator_color(mine), data.clone()).indicator_color,
            mine
        );
        assert_eq!(
            resolve(drawer(), data).indicator_color,
            Color(0xFF00FF00),
            "and the theme is the step below it"
        );
    }

    #[test]
    fn a_disabled_destination_is_not_also_a_selected_one() {
        // Upstream's `disabledState` is `{disabled}` alone -- the selection is
        // dropped rather than added to.
        let states = ResolvedNavigationDrawer::states(false, true);
        assert!(states.contains(WidgetState::Disabled));
        assert!(!states.contains(WidgetState::Selected));
        assert_eq!(states, ResolvedNavigationDrawer::states(false, false));
    }

    #[test]
    fn so_a_disabled_destinations_two_icons_have_nothing_to_fade_between() {
        // The consequence, which is the reason the state set is worth pinning:
        // selected and unselected resolve to one colour, so the crossfade has
        // nothing to show.
        let resolved = resolve(drawer(), NavigationDrawerThemeData::new());
        let selected = resolved.foreground(ResolvedNavigationDrawer::states(false, true));
        let unselected = resolved.foreground(ResolvedNavigationDrawer::states(false, false));
        assert_eq!(selected, unselected);

        // And while enabled they do differ, or the fade would be pointless
        // everywhere rather than only where it is meant to be.
        assert_ne!(
            resolved.foreground(ResolvedNavigationDrawer::states(true, true)),
            resolved.foreground(ResolvedNavigationDrawer::states(true, false))
        );
    }

    #[test]
    fn the_selected_foreground_is_the_indicators_partner_and_not_a_brighter_variant() {
        let resolved = resolve(drawer(), NavigationDrawerThemeData::new());
        let scheme = ThemeData::fallback().color_scheme;
        assert_eq!(
            resolved.foreground(ResolvedNavigationDrawer::states(true, true)),
            scheme.on_secondary_container()
        );
        assert_eq!(
            resolved.foreground(ResolvedNavigationDrawer::states(true, false)),
            scheme.on_surface_variant()
        );
        assert_eq!(
            resolved.foreground(ResolvedNavigationDrawer::states(false, false)),
            crate::elevation_overlay::with_opacity(scheme.on_surface_variant(), 0.38)
        );
    }

    #[test]
    fn the_tile_padding_has_no_theme_step_at_all() {
        // `NavigationDrawerThemeData` has no `tilePadding` field, so the
        // widget's own default is the only source there is.
        let plain = resolve(drawer(), NavigationDrawerThemeData::new());
        assert_eq!(plain.tile_padding, NavigationDrawer::TILE_PADDING);
        let mine = EdgeInsets::symmetric(4.0, 0.0);
        assert_eq!(
            resolve(
                drawer().with_tile_padding(mine),
                NavigationDrawerThemeData::new()
            )
            .tile_padding,
            mine
        );
    }

    #[test]
    fn the_destination_defaults_are_the_m3_ones() {
        let plain = resolve(drawer(), NavigationDrawerThemeData::new());
        assert_eq!(plain.tile_height, 56.0);
        assert_eq!(plain.indicator_size, Size::new(336.0, 56.0));
        assert!(matches!(
            plain.indicator_shape,
            crate::borders::ShapeBorder::Stadium(_)
        ));
        assert_eq!(plain.icon_theme(WidgetStates::NONE).size, Some(24.0));
    }

    #[test]
    fn a_theme_that_supplies_a_property_is_asked_instead_of_the_default() {
        let mine = Color(0xFFABCDEF);
        let mut data = NavigationDrawerThemeData::new();
        data.icon_theme = Some(crate::widget_state::StateProperty::all(Some(
            crate::component_themes::IconThemeData::new()
                .with_size(11.0)
                .with_color(mine),
        )));
        let resolved = resolve(drawer(), data);
        let icons = resolved.icon_theme(WidgetStates::NONE);
        assert_eq!(icons.size, Some(11.0));
        assert_eq!(icons.color, Some(mine));
    }
}
