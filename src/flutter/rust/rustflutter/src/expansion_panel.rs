// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `material/expansion_panel.dart`: a list of panels that open, built
//! on [`crate::mergeable_material`].
//!
//! The reason it is built on that and not on a column of cards is the whole
//! design: a closed list is **one** card, and opening a panel *tears* it. The
//! panels above the opened one stay merged into one piece, the opened one
//! separates, and the panels below merge into another -- which is what tells
//! the reader that the thing they opened came out of the list rather than
//! being a new thing on top of it.
//!
//! # The gap rule, and why it is two conditions
//!
//! Upstream inserts a [`MaterialGap`] *before* panel `i` when `i` is expanded,
//! is not the first, and its predecessor is **not** expanded; and *after*
//! panel `i` when `i` is expanded and is not the last. Neither condition is
//! arbitrary -- together they are exactly what keeps
//! [`MergeableMaterial`]'s two invariants:
//!
//! * Two panels expanded in a row would otherwise get a gap after the first
//!   and another before the second: two gaps in a row, which
//!   `MergeableMaterial` forbids. The `!expanded(i - 1)` clause is what stops
//!   the second one.
//! * An expanded panel at either end would otherwise put a gap at that end,
//!   which `MergeableMaterial` also forbids. The `i != 0` and `i != last`
//!   clauses are what stop those.
//!
//! There is a test that checks this exhaustively over every pattern of open
//! and closed panels, because "the invariants hold for the cases I thought
//! of" is a weaker claim than the code makes.
//!
//! # What is not ported
//!
//! * **`ExpandIcon`** -- the chevron that turns. It is its own upstream class
//!   waiting on the icon system, so the header here carries no chevron.
//! * **`AnimatedCrossFade`** on the body, which upstream uses to fade the old
//!   size out over the first 60% and the new content in over the last 60%
//!   (overlapping in the middle, which is what keeps the panel from looking
//!   empty mid-open). This crate has no cross-fade, so the body is present or
//!   absent.
//! * The **`Semantics` hint** on the icon, which needs `MaterialLocalizations`.

use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext, Component, component, leaf};
use crate::mergeable_material::{
    MaterialGap, MaterialSlice, MergeableMaterial, MergeableMaterialItem,
};

/// Upstream's `_kPanelHeaderCollapsedHeight`, which is
/// `kMinInteractiveDimension`: a header is at least a comfortable target
/// however short its text is.
pub const PANEL_HEADER_COLLAPSED_HEIGHT: f32 = 48.0;

/// Upstream's `_kPanelHeaderExpandedDefaultPadding`, which is written as
/// `64 - kMinInteractiveDimension` rather than as 16.
///
/// Spelt the same way here, because the arithmetic is the point: an expanded
/// header is 64 tall, and the padding is whatever is left over once the
/// minimum target height is accounted for. Writing 16 would leave two numbers
/// to keep in step by hand.
pub const PANEL_HEADER_EXPANDED_VERTICAL_PADDING: f32 = 64.0 - PANEL_HEADER_COLLAPSED_HEIGHT;

/// Upstream `ExpansionPanel`: one panel of the list.
pub struct ExpansionPanel {
    /// Upstream's `headerBuilder`, which is handed whether the panel is open
    /// -- so a header can say "Details" when shut and show the chosen value
    /// when open.
    pub header_builder: Box<dyn Fn(bool) -> AnyWidget>,
    pub body: std::cell::RefCell<Option<AnyWidget>>,
    pub is_expanded: bool,
    /// Upstream's `canTapOnHeader`. Off by default, which means only the
    /// chevron opens the panel: a header that is itself interactive -- a row
    /// of controls -- would otherwise swallow every press.
    pub can_tap_on_header: bool,
    pub background_color: Option<Color>,
    pub splash_color: Option<Color>,
    pub highlight_color: Option<Color>,
}

impl ExpansionPanel {
    pub fn new(
        header_builder: impl Fn(bool) -> AnyWidget + 'static,
        body: AnyWidget,
    ) -> ExpansionPanel {
        ExpansionPanel {
            header_builder: Box::new(header_builder),
            body: std::cell::RefCell::new(Some(body)),
            is_expanded: false,
            can_tap_on_header: false,
            background_color: None,
            splash_color: None,
            highlight_color: None,
        }
    }

    pub fn with_expanded(mut self, is_expanded: bool) -> Self {
        self.is_expanded = is_expanded;
        self
    }

    pub fn with_tap_on_header(mut self, can_tap: bool) -> Self {
        self.can_tap_on_header = can_tap;
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }
}

/// Upstream `ExpansionPanelRadio`: a panel that knows its own identity.
///
/// Upstream it is a subclass adding one field; here it holds the panel,
/// because Rust has no inheritance and the relationship really is
/// composition -- a radio panel *is* a panel plus a value.
///
/// The value exists because a radio list tracks *which* panel is open rather
/// than a flag per panel, and it has to survive the list being rebuilt with
/// the panels in a different order. An index would not.
pub struct ExpansionPanelRadio {
    pub panel: ExpansionPanel,
    /// Upstream's `value`, an `Object` compared by equality. A `u64` here,
    /// which is what this crate uses for identity everywhere else.
    pub value: u64,
}

impl ExpansionPanelRadio {
    pub fn new(
        value: u64,
        header_builder: impl Fn(bool) -> AnyWidget + 'static,
        body: AnyWidget,
    ) -> ExpansionPanelRadio {
        ExpansionPanelRadio {
            panel: ExpansionPanel::new(header_builder, body),
            value,
        }
    }
}

/// Which panels a list may have open at once.
enum PanelMode {
    /// Upstream's default constructor: each panel carries its own
    /// `isExpanded` and any number may be open.
    Any,
    /// Upstream's `ExpansionPanelList.radio`: one at a time, tracked by value.
    OnlyOne { values: Vec<u64>, open: Option<u64> },
}

/// Upstream `ExpansionPanelList`.
pub struct ExpansionPanelList {
    panels: std::cell::RefCell<Vec<ExpansionPanel>>,
    mode: PanelMode,
    pub elevation: u32,
    pub divider_color: Option<Color>,
    /// Upstream's `materialGapSize`, which it passes straight to the gaps.
    pub material_gap_size: f32,
    #[allow(clippy::type_complexity)]
    expansion_callback: Option<std::rc::Rc<dyn Fn(usize, bool)>>,
    /// The first hit-test id the headers may use, one per panel from there.
    ///
    /// Ids are the caller's to allocate in this crate -- see
    /// [`crate::components::IdSource`] and the `id` every other interactive
    /// control takes -- so a list that wants tappable headers has to say
    /// which range is free. Unset, the headers are inert, which is also what
    /// upstream gives a panel whose `canTapOnHeader` is false.
    header_tap_ids: Option<u64>,
}

impl ExpansionPanelList {
    pub fn new(panels: Vec<ExpansionPanel>) -> ExpansionPanelList {
        ExpansionPanelList {
            panels: std::cell::RefCell::new(panels),
            mode: PanelMode::Any,
            elevation: 2,
            divider_color: None,
            material_gap_size: MaterialGap::DEFAULT_SIZE,
            expansion_callback: None,
            header_tap_ids: None,
        }
    }

    /// Upstream's `ExpansionPanelList.radio`.
    pub fn radio(
        panels: Vec<ExpansionPanelRadio>,
        initial_open: Option<u64>,
    ) -> ExpansionPanelList {
        let values: Vec<u64> = panels.iter().map(|radio| radio.value).collect();
        let panels: Vec<ExpansionPanel> = panels.into_iter().map(|radio| radio.panel).collect();
        // Upstream's `_currentOpenPanel = searchPanelByValue(...)`: an initial
        // value naming no panel opens nothing rather than opening the first,
        // because a caller who named a panel that is not there has a bug and
        // guessing would hide it.
        let open = initial_open.filter(|value| values.contains(value));
        ExpansionPanelList {
            panels: std::cell::RefCell::new(panels),
            mode: PanelMode::OnlyOne { values, open },
            elevation: 2,
            divider_color: None,
            material_gap_size: MaterialGap::DEFAULT_SIZE,
            expansion_callback: None,
            header_tap_ids: None,
        }
    }

    /// See [`ExpansionPanelList::header_tap_ids`].
    pub fn with_header_tap_ids(mut self, base: u64) -> Self {
        self.header_tap_ids = Some(base);
        self
    }

    pub fn with_elevation(mut self, elevation: u32) -> Self {
        self.elevation = elevation;
        self
    }

    pub fn with_divider_color(mut self, color: Color) -> Self {
        self.divider_color = Some(color);
        self
    }

    pub fn with_material_gap_size(mut self, size: f32) -> Self {
        self.material_gap_size = size;
        self
    }

    pub fn with_expansion_callback(mut self, callback: impl Fn(usize, bool) + 'static) -> Self {
        self.expansion_callback = Some(std::rc::Rc::new(callback));
        self
    }

    /// Upstream's `_allIdentifiersUnique`, which it asserts on in a radio
    /// list.
    ///
    /// Two panels with the same value are one panel as far as the list is
    /// concerned: opening either opens both, and there is no press that can
    /// tell them apart.
    pub fn all_identifiers_unique(&self) -> bool {
        match &self.mode {
            PanelMode::Any => true,
            PanelMode::OnlyOne { values, .. } => {
                let mut seen = values.clone();
                seen.sort_unstable();
                seen.dedup();
                seen.len() == values.len()
            }
        }
    }

    /// Upstream's `_isChildExpanded`.
    pub fn is_child_expanded(&self, index: usize) -> bool {
        match &self.mode {
            PanelMode::Any => self
                .panels
                .borrow()
                .get(index)
                .is_some_and(|panel| panel.is_expanded),
            PanelMode::OnlyOne { values, open } => {
                open.is_some() && values.get(index).copied() == *open
            }
        }
    }

    /// Upstream's `_handlePressed`, as the list of callbacks it would fire.
    ///
    /// Returned rather than fired so the rule is testable, and because two of
    /// its details are easy to get wrong:
    ///
    /// * **The panel being closed is reported first.** In a radio list,
    ///   opening one closes another, and upstream calls the callback for the
    ///   *other* panel with `false` before the one for this panel. A caller
    ///   keeping its own record would otherwise see them out of order and end
    ///   up with two panels marked open.
    /// * **The value reported is `!isExpanded`, not the panel's current
    ///   state.** Upstream's own comment says why: at the moment of the press
    ///   the panel has not flipped yet, so the callback carries where it is
    ///   *going*.
    pub fn presses_for(&self, index: usize) -> Vec<(usize, bool)> {
        let was_expanded = self.is_child_expanded(index);
        let mut calls = Vec::new();
        if let PanelMode::OnlyOne { values, open } = &self.mode {
            for (other, value) in values.iter().enumerate() {
                if other != index && Some(*value) == *open {
                    calls.push((other, false));
                }
            }
        }
        calls.push((index, !was_expanded));
        calls
    }

    /// The state a radio list moves to when panel `index` is pressed.
    /// Upstream's `_currentOpenPanel = isExpanded ? null : pressedChild` --
    /// pressing the open panel closes it rather than doing nothing.
    pub fn opened_after_press(&self, index: usize) -> Option<u64> {
        match &self.mode {
            PanelMode::Any => None,
            PanelMode::OnlyOne { values, .. } => {
                if self.is_child_expanded(index) {
                    None
                } else {
                    values.get(index).copied()
                }
            }
        }
    }

    /// Which gaps and slices this list becomes. Upstream's `build` loop,
    /// without the widgets -- see the module docs for why the two conditions
    /// are what they are.
    ///
    /// `slice` is called for each panel to build its content, so the caller
    /// decides what a panel looks like and this decides where the tears go.
    pub fn items(
        &self,
        mut slice: impl FnMut(usize, bool) -> AnyWidget,
    ) -> Vec<MergeableMaterialItem> {
        let count = self.panels.borrow().len();
        let mut items = Vec::new();
        for index in 0..count {
            let expanded = self.is_child_expanded(index);
            if expanded && index != 0 && !self.is_child_expanded(index - 1) {
                items.push(MergeableMaterialItem::Gap(
                    // Upstream's `_SaltedKey(context, index * 2 - 1)`: the odd
                    // numbers are gaps and the even ones slices, so the two
                    // series cannot collide.
                    MaterialGap::new((index * 2 - 1) as u64).with_size(self.material_gap_size),
                ));
            }
            let content = slice(index, expanded);
            let mut piece = MaterialSlice::new((index * 2) as u64, content);
            if let Some(colour) = self.panels.borrow()[index].background_color {
                piece = piece.with_color(colour);
            }
            items.push(MergeableMaterialItem::Slice(piece));
            if expanded && index + 1 != count {
                items.push(MergeableMaterialItem::Gap(
                    MaterialGap::new((index * 2 + 1) as u64).with_size(self.material_gap_size),
                ));
            }
        }
        items
    }
}

impl Component for ExpansionPanelList {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        debug_assert!(
            self.all_identifiers_unique(),
            "all ExpansionPanelRadio values must be unique"
        );
        let panels = std::mem::take(&mut *self.panels.borrow_mut());
        let expanded: Vec<bool> = (0..panels.len())
            .map(|index| self.is_child_expanded(index))
            .collect();
        // Rebuilt with the panels moved out, so `items` still sees the same
        // count and colours it would have.
        let colours: Vec<Option<Color>> = panels.iter().map(|p| p.background_color).collect();
        let gap_size = self.material_gap_size;

        let mut bodies: Vec<Option<AnyWidget>> = Vec::new();
        let mut headers: Vec<AnyWidget> = Vec::new();
        for (index, panel) in panels.iter().enumerate() {
            let header = (panel.header_builder)(expanded[index]);
            // Upstream wraps the header in an `InkWell` when `canTapOnHeader`
            // is set, and leaves it inert otherwise so that a header which is
            // itself interactive does not swallow every press. The callback
            // carries `presses_for`'s answer, which is where the panel is
            // *going* -- see there.
            headers.push(
                match (
                    panel.can_tap_on_header,
                    &self.expansion_callback,
                    self.header_tap_ids,
                ) {
                    (true, Some(callback), Some(base)) => {
                        let calls = self.presses_for(index);
                        let callback = std::rc::Rc::clone(callback);
                        let id = base + index as u64;
                        crate::framework::single(header, move |inner| {
                            Box::new(
                                crate::render::RenderPointerRegion::new(id, inner).with_handlers(
                                    crate::gestures::PointerHandlers::new().with_tap({
                                        let callback = std::rc::Rc::clone(&callback);
                                        let calls = calls.clone();
                                        move |_| {
                                            for (panel, expanded) in &calls {
                                                callback(*panel, *expanded);
                                            }
                                        }
                                    }),
                                ),
                            )
                        })
                    }
                    _ => header,
                },
            );
            bodies.push(if expanded[index] {
                panel.body.borrow().clone()
            } else {
                None
            });
        }

        let mut items = Vec::new();
        let count = panels.len();
        for index in 0..count {
            if expanded[index] && index != 0 && !expanded[index - 1] {
                items.push(MergeableMaterialItem::Gap(
                    MaterialGap::new((index * 2 - 1) as u64).with_size(gap_size),
                ));
            }
            let header = std::mem::replace(&mut headers[index], leaf(|| crate::widgets::Empty));
            let body = bodies[index].take();
            let content = crate::framework::many(
                match body {
                    Some(body) => vec![header, body],
                    None => vec![header],
                },
                move |mut boxed| {
                    let mut column = crate::widgets::Column::new()
                        .with_main_axis_size(crate::render::MainAxisSize::Min)
                        .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Stretch);
                    // The header is at least a comfortable target however
                    // short its text is.
                    let header = boxed.remove(0);
                    column = column.push(
                        crate::render::RenderConstrainedBox::new(crate::render::BoxConstraints {
                            min_width: 0.0,
                            max_width: f32::INFINITY,
                            min_height: PANEL_HEADER_COLLAPSED_HEIGHT,
                            max_height: f32::INFINITY,
                        })
                        .with_child(header),
                    );
                    for rest in boxed.drain(..) {
                        column = column.push(rest);
                    }
                    column
                },
            );
            let mut piece = MaterialSlice::new((index * 2) as u64, content);
            if let Some(colour) = colours[index] {
                piece = piece.with_color(colour);
            }
            items.push(MergeableMaterialItem::Slice(piece));
            if expanded[index] && index + 1 != count {
                items.push(MergeableMaterialItem::Gap(
                    MaterialGap::new((index * 2 + 1) as u64).with_size(gap_size),
                ));
            }
        }

        let mut merged = MergeableMaterial::new(items)
            .with_dividers(true)
            .with_elevation(self.elevation);
        if let Some(colour) = self.divider_color {
            merged = merged.with_divider_color(colour);
        }
        component(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::leaf;
    use crate::widgets::Empty;

    fn panel(expanded: bool) -> ExpansionPanel {
        ExpansionPanel::new(|_| leaf(|| Empty), leaf(|| Empty)).with_expanded(expanded)
    }

    fn radio(value: u64) -> ExpansionPanelRadio {
        ExpansionPanelRadio::new(value, |_| leaf(|| Empty), leaf(|| Empty))
    }

    fn body() -> AnyWidget {
        leaf(|| Empty)
    }

    #[test]
    fn the_gap_rule_keeps_the_mergeable_invariants_for_every_pattern() {
        // Exhaustive over every combination of open and closed, because "the
        // invariants hold for the cases I thought of" is a weaker claim than
        // the code makes. Both of upstream's conditions exist precisely to
        // keep these two, and dropping either breaks a pattern in here.
        for count in 0..=6usize {
            for bits in 0..(1u32 << count) {
                let panels: Vec<ExpansionPanel> = (0..count)
                    .map(|index| panel(bits & (1 << index) != 0))
                    .collect();
                let list = ExpansionPanelList::new(panels);
                let items = list.items(|_, _| body());
                assert!(
                    MergeableMaterial::gaps_are_valid(&items),
                    "count {count}, pattern {bits:b}"
                );
            }
        }
    }

    #[test]
    fn an_open_panel_is_torn_out_of_the_run_on_both_sides() {
        // The middle one of three: a gap before it and a gap after it, so it
        // is its own piece and the two neighbours are theirs.
        let list = ExpansionPanelList::new(vec![panel(false), panel(true), panel(false)]);
        let items = list.items(|_, _| body());
        let kinds: Vec<bool> = items.iter().map(|item| item.is_gap()).collect();
        assert_eq!(kinds, vec![false, true, false, true, false]);
    }

    #[test]
    fn two_open_panels_in_a_row_share_one_gap_between_them() {
        // Not two. The `!expanded(i - 1)` clause is what stops the second,
        // and without it `MergeableMaterial` would reject the list.
        let list =
            ExpansionPanelList::new(vec![panel(false), panel(true), panel(true), panel(false)]);
        let items = list.items(|_, _| body());
        let kinds: Vec<bool> = items.iter().map(|item| item.is_gap()).collect();
        assert_eq!(kinds, vec![false, true, false, true, false, true, false]);
    }

    #[test]
    fn an_open_panel_at_either_end_puts_no_gap_off_the_end() {
        let first = ExpansionPanelList::new(vec![panel(true), panel(false)]);
        let kinds: Vec<bool> = first
            .items(|_, _| body())
            .iter()
            .map(|i| i.is_gap())
            .collect();
        assert_eq!(kinds, vec![false, true, false], "nothing before the first");

        let last = ExpansionPanelList::new(vec![panel(false), panel(true)]);
        let kinds: Vec<bool> = last
            .items(|_, _| body())
            .iter()
            .map(|i| i.is_gap())
            .collect();
        assert_eq!(kinds, vec![false, true, false], "nothing after the last");

        // And a lone open panel gets no gaps at all: it is already the whole
        // card.
        let lone = ExpansionPanelList::new(vec![panel(true)]);
        let kinds: Vec<bool> = lone
            .items(|_, _| body())
            .iter()
            .map(|i| i.is_gap())
            .collect();
        assert_eq!(kinds, vec![false]);
    }

    #[test]
    fn a_radio_list_tracks_which_panel_is_open_rather_than_a_flag_each() {
        let list = ExpansionPanelList::radio(vec![radio(10), radio(20), radio(30)], Some(20));
        assert!(!list.is_child_expanded(0));
        assert!(list.is_child_expanded(1));
        assert!(!list.is_child_expanded(2));
    }

    #[test]
    fn an_initial_value_naming_no_panel_opens_nothing() {
        // Rather than opening the first: a caller who named a panel that is
        // not there has a bug, and guessing would hide it.
        let list = ExpansionPanelList::radio(vec![radio(10), radio(20)], Some(99));
        assert!(!list.is_child_expanded(0));
        assert!(!list.is_child_expanded(1));
        assert_eq!(list.opened_after_press(0), Some(10), "and pressing works");
    }

    #[test]
    fn opening_one_radio_panel_reports_the_closing_one_first() {
        // A caller keeping its own record would otherwise see them out of
        // order and end up with two panels marked open.
        let list = ExpansionPanelList::radio(vec![radio(10), radio(20), radio(30)], Some(10));
        assert_eq!(list.presses_for(2), vec![(0, false), (2, true)]);
    }

    #[test]
    fn the_reported_value_is_where_the_panel_is_going_not_where_it_is() {
        // Upstream's own comment: at the moment of the press the panel has
        // not flipped yet, so the callback carries `!isExpanded`.
        let open = ExpansionPanelList::new(vec![panel(true)]);
        assert_eq!(
            open.presses_for(0),
            vec![(0, false)],
            "an open one is closing"
        );
        let shut = ExpansionPanelList::new(vec![panel(false)]);
        assert_eq!(
            shut.presses_for(0),
            vec![(0, true)],
            "a shut one is opening"
        );
    }

    #[test]
    fn pressing_the_open_radio_panel_closes_it() {
        // Upstream's `isExpanded ? null : pressedChild` -- pressing the open
        // one closes it rather than doing nothing, so a radio list can be
        // fully shut.
        let list = ExpansionPanelList::radio(vec![radio(10), radio(20)], Some(10));
        assert_eq!(list.opened_after_press(0), None);
        assert_eq!(list.opened_after_press(1), Some(20));
        assert_eq!(list.presses_for(0), vec![(0, false)], "no other to close");
    }

    #[test]
    fn two_radio_panels_with_the_same_value_are_rejected() {
        // They are one panel as far as the list is concerned: opening either
        // opens both, and no press can tell them apart.
        assert!(ExpansionPanelList::radio(vec![radio(1), radio(2)], None).all_identifiers_unique());
        assert!(
            !ExpansionPanelList::radio(vec![radio(1), radio(1)], None).all_identifiers_unique()
        );
        // A plain list has no identifiers to collide.
        assert!(ExpansionPanelList::new(vec![panel(false), panel(false)]).all_identifiers_unique());
    }

    #[test]
    fn the_expanded_header_padding_is_written_as_the_arithmetic_it_is() {
        // Upstream spells it `64 - kMinInteractiveDimension` rather than 16,
        // and the arithmetic is the point: an expanded header is 64 tall and
        // the padding is what is left once the minimum target is accounted
        // for. Two hand-kept numbers would drift.
        assert_eq!(PANEL_HEADER_COLLAPSED_HEIGHT, 48.0);
        assert_eq!(PANEL_HEADER_EXPANDED_VERTICAL_PADDING, 16.0);
        assert_eq!(
            PANEL_HEADER_COLLAPSED_HEIGHT + PANEL_HEADER_EXPANDED_VERTICAL_PADDING,
            64.0
        );
    }

    #[test]
    fn gaps_and_slices_take_keys_from_two_series_that_cannot_collide() {
        // Upstream salts them `index * 2` and `index * 2 +- 1`, so a gap's key
        // is odd and a slice's even. A collision would make the mergeable
        // material treat a gap and a slice as the same item across a rebuild.
        let list = ExpansionPanelList::new(vec![panel(false), panel(true), panel(false)]);
        let items = list.items(|_, _| body());
        for item in &items {
            let key = item.key().expect("every item is keyed");
            assert_eq!(item.is_gap(), key % 2 == 1, "key {key}");
        }
    }

    #[test]
    fn a_tappable_header_takes_the_press_and_an_untappable_one_does_not() {
        // Wired end to end rather than only through `presses_for`, so the
        // callback cannot be a field nothing reaches. `can_tap_on_header` is
        // off by default because a header that is itself interactive would
        // otherwise swallow every press.
        use crate::framework::{ElementTree, component, provide};
        use crate::render::{BoxConstraints, HitTestResult, Offset, RenderBox};

        let build = |can_tap: bool| {
            ExpansionPanelList::new(vec![
                ExpansionPanel::new(
                    |_| leaf(|| crate::widgets::Container::new().with_size(100.0, 50.0)),
                    leaf(|| Empty),
                )
                .with_tap_on_header(can_tap),
            ])
            .with_header_tap_ids(9000)
            .with_expansion_callback(|_, _| {})
        };

        let takes_the_press = |can_tap: bool| {
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                crate::components::Theme::dark(),
                component(build(can_tap)),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(300.0, 300.0));
            let mut result = HitTestResult::new();
            root.hit_test(Offset::new(50.0, 20.0), &mut result);
            result.path.iter().any(|entry| entry.target == 9000)
        };

        assert!(takes_the_press(true), "a tappable header takes the press");
        assert!(!takes_the_press(false), "an untappable one lets it through");
    }
}
