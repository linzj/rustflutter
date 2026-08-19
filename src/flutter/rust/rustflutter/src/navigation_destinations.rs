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
