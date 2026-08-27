// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `material/mergeable_material.dart`: a run of slices that read as
//! one card until something splits them.
//!
//! The idea is a list where adjacent items are *the same surface*. Two slices
//! with nothing between them share one shadow and one outline, and their
//! touching corners are square -- so they look like one card with a rule
//! across it, not two cards stacked. Put a [`MaterialGap`] between them and
//! the two halves round their new edges and separate.
//!
//! That is the whole point, and it is what an expansion panel list is built
//! on: opening a panel does not add a card, it *tears* the card it was part
//! of. The rounding is what sells it -- a corner that grows from square to
//! rounded as the gap opens reads as a tear, where two cards fading in reads
//! as a replacement.
//!
//! # The two invariants
//!
//! Upstream asserts both in `_debugGapsAreValid`, and both are structural
//! rather than cosmetic:
//!
//! * **No two gaps in a row.** Two adjacent gaps are one wider gap that
//!   nothing can tell apart, so the second is unreachable state -- and a
//!   caller who wrote one meant something else.
//! * **Neither end may be a gap.** A gap's job is to separate two slices; a
//!   gap at the end separates a slice from nothing, which is padding wearing
//!   a gap's clothes.
//!
//! # What is not ported
//!
//! Upstream keeps an `AnimationController` per gap, keyed by the gap's
//! `LocalKey`, so that a gap inserted into the list *grows* from nothing and
//! a removed one shrinks -- with the slice corners lerping from square to
//! rounded on the same clock. That needs animations that survive a rebuild
//! keyed by item identity, which this crate has no facility for yet; the
//! per-frame model in [`crate::implicit`] keys state by position in the tree,
//! not by a key carried in a list. So gaps here are their full size at once.
//! The arithmetic that animation drives -- [`MergeableMaterial::border_radius`]
//! -- is written to take the gap's openness as a fraction, so wiring a clock
//! to it later is a change at one call site.

use crate::borders::{BorderRadius, Radius};
use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext, Component, Key, leaf, many};
use crate::material::MaterialType;
use crate::render::Axis;

/// Upstream `MergeableMaterialItem`: one entry in a [`MergeableMaterial`].
///
/// Upstream this is an abstract base with a `key` and two subclasses; here it
/// is an enum wrapping them, the shape this crate takes for a closed set (see
/// [`crate::borders::ShapeBorder`]). The key is on the variants because it is
/// what upstream's base holds, and what a future keyed animation would look
/// the gap's controller up by.
pub enum MergeableMaterialItem {
    Slice(MaterialSlice),
    Gap(MaterialGap),
}

impl MergeableMaterialItem {
    /// Upstream's `MergeableMaterialItem.key`.
    pub fn key(&self) -> Key {
        match self {
            MergeableMaterialItem::Slice(slice) => slice.key,
            MergeableMaterialItem::Gap(gap) => gap.key,
        }
    }

    pub fn is_gap(&self) -> bool {
        matches!(self, MergeableMaterialItem::Gap(_))
    }
}

/// Upstream `MaterialSlice`: a piece of the surface, with something on it.
pub struct MaterialSlice {
    pub key: Key,
    pub child: std::cell::RefCell<Option<AnyWidget>>,
    /// Upstream's `color`, which overrides the material's own for this slice
    /// alone -- so one slice of a merged run can be highlighted without
    /// breaking the run apart.
    pub color: Option<Color>,
}

impl MaterialSlice {
    pub fn new(key: u64, child: AnyWidget) -> MaterialSlice {
        MaterialSlice {
            key: Some(key),
            child: std::cell::RefCell::new(Some(child)),
            color: None,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

/// Upstream `MaterialGap`: a space that splits the run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialGap {
    pub key: Key,
    /// Upstream's default of 16.
    pub size: f32,
}

impl MaterialGap {
    /// Upstream's default `size`.
    pub const DEFAULT_SIZE: f32 = 16.0;

    pub fn new(key: u64) -> MaterialGap {
        MaterialGap {
            key: Some(key),
            size: MaterialGap::DEFAULT_SIZE,
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

/// Upstream `MergeableMaterial`: the run itself.
pub struct MergeableMaterial {
    pub children: std::cell::RefCell<Vec<MergeableMaterialItem>>,
    pub main_axis: Axis,
    /// Upstream's default of 2 -- one step above a card, because a merged run
    /// is a surface the reader is meant to act on.
    pub elevation: u32,
    pub has_dividers: bool,
    pub divider_color: Option<Color>,
}

impl MergeableMaterial {
    pub fn new(children: Vec<MergeableMaterialItem>) -> MergeableMaterial {
        MergeableMaterial {
            children: std::cell::RefCell::new(children),
            main_axis: Axis::Vertical,
            elevation: 2,
            has_dividers: false,
            divider_color: None,
        }
    }

    pub fn with_main_axis(mut self, axis: Axis) -> Self {
        self.main_axis = axis;
        self
    }

    pub fn with_elevation(mut self, elevation: u32) -> Self {
        self.elevation = elevation;
        self
    }

    /// Upstream's `hasDividers`: a rule between touching slices.
    ///
    /// Only between *touching* ones -- a gap already separates, and a rule
    /// inside a gap would be a line floating in space.
    pub fn with_dividers(mut self, has_dividers: bool) -> Self {
        self.has_dividers = has_dividers;
        self
    }

    pub fn with_divider_color(mut self, color: Color) -> Self {
        self.divider_color = Some(color);
        self
    }

    /// The radius a merged run's outer corners take. Upstream reads it out of
    /// `kMaterialEdges[MaterialType.card]`, asserting first that all four
    /// corners agree -- which they do, so this is that one number.
    pub fn card_radius() -> f32 {
        MaterialType::Card.border_radius().unwrap_or(0.0)
    }

    /// Upstream's `_debugGapsAreValid`. See the module docs for why each half
    /// is a rule rather than a preference.
    pub fn gaps_are_valid(children: &[MergeableMaterialItem]) -> bool {
        if children
            .windows(2)
            .any(|pair| pair[0].is_gap() && pair[1].is_gap())
        {
            return false;
        }
        match (children.first(), children.last()) {
            (Some(first), Some(last)) => !first.is_gap() && !last.is_gap(),
            // An empty run is valid: it has no gap to be wrong about.
            _ => true,
        }
    }

    /// Upstream's `_borderRadius`: how the slice at `index` rounds.
    ///
    /// `start` and `end` are upstream's, and mean "this slice is at the very
    /// start (or end) of the whole run". Those corners always take the full
    /// card radius, because they are the outside of the card whatever else is
    /// happening.
    ///
    /// The other two corners are the interesting ones. A slice's inner corner
    /// is **square where it touches its neighbour** -- that is what makes two
    /// slices read as one card -- and rounds only against a gap, in
    /// proportion to how open that gap is. `gap_openness` is upstream's
    /// `startAnimation.value`/`endAnimation.value`, at 1 for a fully open gap;
    /// it is a parameter rather than read from a clock because this crate has
    /// no per-gap clock yet (see the module docs).
    pub fn border_radius(
        &self,
        index: usize,
        start: bool,
        end: bool,
        gap_openness: impl Fn(usize) -> Option<f32>,
    ) -> BorderRadius {
        let card = Radius::circular(MergeableMaterial::card_radius());
        let zero = Radius::ZERO;
        let children = self.children.borrow();

        // The corner nearer the run's start rounds if what precedes this
        // slice is a gap.
        let start_radius = match index
            .checked_sub(1)
            .filter(|&before| children.get(before).is_some_and(|item| item.is_gap()))
            .and_then(|before| gap_openness(before))
        {
            Some(openness) => Radius::lerp(zero, card, openness),
            None => zero,
        };
        // And the far corner if what follows is one. Upstream's bound is
        // `index < children.length - 2`, not `- 1`: the item after this one
        // being the *last* item cannot be a gap anyway (the invariant), so
        // the shorter bound would only ever ask a question already answered.
        let end_radius = match Some(index + 1)
            .filter(|&after| after + 1 < children.len())
            .filter(|&after| children.get(after).is_some_and(|item| item.is_gap()))
            .and_then(gap_openness)
        {
            Some(openness) => Radius::lerp(zero, card, openness),
            None => zero,
        };

        match self.main_axis {
            Axis::Vertical => BorderRadius::vertical(
                if start { card } else { start_radius },
                if end { card } else { end_radius },
            ),
            Axis::Horizontal => BorderRadius::horizontal(
                if start { card } else { start_radius },
                if end { card } else { end_radius },
            ),
        }
    }
}

impl Component for MergeableMaterial {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let children = std::mem::take(&mut *self.children.borrow_mut());
        debug_assert!(
            MergeableMaterial::gaps_are_valid(&children),
            "a gap may not lead, trail, or follow another gap"
        );
        let theme = crate::components::theme_of(context);
        let divider = crate::component_themes::ResolvedDivider::of(context);
        let divider_color = self.divider_color.unwrap_or(divider.color);
        let surface = theme.surface;
        let elevation = self.elevation;
        let axis = self.main_axis;
        let has_dividers = self.has_dividers;
        let card = MergeableMaterial::card_radius();

        // Gaps are full-sized here, so a slice's inner corner is rounded
        // exactly when there is a gap beside it. See the module docs.
        let openness = |_: usize| Some(1.0);
        let count = children.len();
        let mut widgets = Vec::new();
        let mut plans: Vec<(f32, Option<Color>, bool, BorderRadius)> = Vec::new();
        for (index, item) in children.into_iter().enumerate() {
            match item {
                MergeableMaterialItem::Gap(gap) => {
                    plans.push((gap.size, None, true, BorderRadius::all(Radius::ZERO)));
                }
                MergeableMaterialItem::Slice(slice) => {
                    let radius =
                        self.border_radius(index, index == 0, index + 1 == count, openness);
                    let child = slice
                        .child
                        .borrow()
                        .clone()
                        .unwrap_or_else(|| leaf(|| crate::widgets::Empty));
                    plans.push((0.0, Some(slice.color.unwrap_or(surface)), false, radius));
                    widgets.push(child);
                }
            }
        }

        many(widgets, move |mut boxed| {
            let mut boxed = boxed.drain(..);
            let mut run = match axis {
                Axis::Vertical => crate::render::RenderFlex::column(),
                Axis::Horizontal => crate::render::RenderFlex::row(),
            }
            .with_main_axis_size(crate::render::MainAxisSize::Min)
            .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Stretch);

            let mut previous_was_slice = false;
            for (size, colour, is_gap, radius) in &plans {
                if *is_gap {
                    run = run.push(match axis {
                        Axis::Vertical => crate::widgets::SizedBox::new(0.0, *size),
                        Axis::Horizontal => crate::widgets::SizedBox::new(*size, 0.0),
                    });
                    previous_was_slice = false;
                    continue;
                }
                // A rule between *touching* slices only: a gap already
                // separates, and a rule inside one would float in space.
                if has_dividers && previous_was_slice {
                    run = run.push(
                        crate::widgets::Container::new()
                            .with_height(1.0)
                            .with_color(divider_color),
                    );
                }
                let child = boxed.next().expect("one box per slice");
                let mut surface = crate::widgets::Container::new().with_child(child);
                if let Some(colour) = colour {
                    surface = surface.with_color(*colour);
                }
                if elevation > 0 {
                    surface = surface.with_elevation(elevation);
                }
                // The crate's renderer takes one radius for all four corners
                // (the limitation `BottomSheet` documents), so a slice with
                // mixed corners takes the larger of the two -- which keeps a
                // torn edge rounded and a touching edge square in the common
                // case where only one end is against a gap.
                let corner = radius
                    .top_left
                    .x
                    .max(radius.top_right.x)
                    .max(radius.bottom_left.x)
                    .max(radius.bottom_right.x);
                if corner > 0.0 {
                    surface = surface.with_corner_radius(corner.min(card));
                }
                run = run.push(surface);
                previous_was_slice = true;
            }
            run
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::leaf;
    use crate::widgets::Empty;

    fn slice(key: u64) -> MergeableMaterialItem {
        MergeableMaterialItem::Slice(MaterialSlice::new(key, leaf(|| Empty)))
    }

    fn gap(key: u64) -> MergeableMaterialItem {
        MergeableMaterialItem::Gap(MaterialGap::new(key))
    }

    #[test]
    fn two_gaps_in_a_row_are_invalid() {
        // Two adjacent gaps are one wider gap that nothing can tell apart, so
        // the second is unreachable state -- and a caller who wrote one meant
        // something else.
        assert!(MergeableMaterial::gaps_are_valid(&[
            slice(1),
            gap(2),
            slice(3)
        ]));
        assert!(!MergeableMaterial::gaps_are_valid(&[
            slice(1),
            gap(2),
            gap(3),
            slice(4)
        ]));
    }

    #[test]
    fn a_gap_at_either_end_is_invalid() {
        // A gap's job is to separate two slices; one at the end separates a
        // slice from nothing, which is padding wearing a gap's clothes.
        assert!(!MergeableMaterial::gaps_are_valid(&[gap(1), slice(2)]));
        assert!(!MergeableMaterial::gaps_are_valid(&[slice(1), gap(2)]));
        assert!(MergeableMaterial::gaps_are_valid(&[slice(1)]));
        // And an empty run has no gap to be wrong about.
        assert!(MergeableMaterial::gaps_are_valid(&[]));
    }

    #[test]
    fn touching_slices_have_square_corners_where_they_meet() {
        // This is what makes two slices read as one card rather than two.
        let run = MergeableMaterial::new(vec![slice(1), slice(2), slice(3)]);
        let card = Radius::circular(MergeableMaterial::card_radius());
        let middle = run.border_radius(1, false, false, |_| Some(1.0));
        assert_eq!(middle.top_left, Radius::ZERO);
        assert_eq!(middle.bottom_left, Radius::ZERO);

        // The run's own two ends are rounded whatever else is happening:
        // they are the outside of the card.
        let first = run.border_radius(0, true, false, |_| Some(1.0));
        assert_eq!(first.top_left, card);
        assert_eq!(first.bottom_left, Radius::ZERO);
        let last = run.border_radius(2, false, true, |_| Some(1.0));
        assert_eq!(last.top_left, Radius::ZERO);
        assert_eq!(last.bottom_left, card);
    }

    #[test]
    fn a_slice_rounds_the_corner_it_turns_towards_a_gap() {
        let run = MergeableMaterial::new(vec![slice(1), gap(2), slice(3)]);
        let card = Radius::circular(MergeableMaterial::card_radius());
        // The slice before the gap rounds its far corner.
        let before = run.border_radius(0, true, false, |_| Some(1.0));
        assert_eq!(before.bottom_left, card, "against the gap");
        // The slice after it rounds its near corner.
        let after = run.border_radius(2, false, true, |_| Some(1.0));
        assert_eq!(after.top_left, card);
    }

    #[test]
    fn a_half_open_gap_gives_a_half_rounded_corner() {
        // The rounding is what makes a split read as a *tear* rather than as
        // two cards appearing: the corner grows from square as the gap opens.
        let run = MergeableMaterial::new(vec![slice(1), gap(2), slice(3)]);
        let card = MergeableMaterial::card_radius();
        let closed = run.border_radius(0, true, false, |_| Some(0.0));
        assert_eq!(closed.bottom_left, Radius::ZERO, "square while shut");
        let half = run.border_radius(0, true, false, |_| Some(0.5));
        assert!((half.bottom_left.x - card / 2.0).abs() < 0.001);
    }

    #[test]
    fn a_horizontal_run_rounds_across_where_a_vertical_one_rounds_down() {
        // The axis decides which *pair* of corners a radius applies to, so
        // the two must differ somewhere -- a middle slice touching on both
        // sides is where they do.
        let items = || vec![slice(1), slice(2), slice(3)];
        let vertical = MergeableMaterial::new(items());
        let horizontal = MergeableMaterial::new(items()).with_main_axis(Axis::Horizontal);
        let card = Radius::circular(MergeableMaterial::card_radius());

        // The first slice of a vertical run rounds its *top* two corners and
        // leaves the bottom two square, because down is where it touches.
        let top = vertical.border_radius(0, true, false, |_| Some(1.0));
        assert_eq!((top.top_left, top.top_right), (card, card));
        assert_eq!(
            (top.bottom_left, top.bottom_right),
            (Radius::ZERO, Radius::ZERO)
        );

        // The first slice of a horizontal run rounds its *left* two instead.
        let left = horizontal.border_radius(0, true, false, |_| Some(1.0));
        assert_eq!((left.top_left, left.bottom_left), (card, card));
        assert_eq!(
            (left.top_right, left.bottom_right),
            (Radius::ZERO, Radius::ZERO)
        );
        assert_ne!(top, left, "the axis has to change the answer");
    }

    #[test]
    fn the_card_radius_is_the_one_a_card_uses() {
        // Upstream reads it out of `kMaterialEdges[MaterialType.card]` after
        // asserting all four corners agree, so it must be that number and not
        // a second copy of it.
        assert_eq!(
            MergeableMaterial::card_radius(),
            MaterialType::Card.border_radius().expect("a card rounds")
        );
        assert_eq!(MergeableMaterial::card_radius(), 2.0);
    }

    #[test]
    fn a_slice_may_carry_its_own_colour_without_breaking_the_run() {
        // Which is the point of `MaterialSlice.color`: one slice of a merged
        // run can be highlighted, and the run stays one run.
        let highlighted = MaterialSlice::new(1, leaf(|| Empty)).with_color(Color::WHITE);
        assert_eq!(highlighted.color, Some(Color::WHITE));
        assert_eq!(MaterialSlice::new(1, leaf(|| Empty)).color, None);
    }

    #[test]
    fn an_items_key_survives_whichever_kind_it_is() {
        // Upstream's base holds the key, and it is what a keyed animation
        // would look a gap's controller up by.
        assert_eq!(slice(7).key(), Some(7));
        assert_eq!(gap(9).key(), Some(9));
        assert!(!slice(7).is_gap());
        assert!(gap(9).is_gap());
    }

    #[test]
    fn a_gap_is_sixteen_unless_it_says_otherwise() {
        assert_eq!(MaterialGap::new(1).size, 16.0);
        assert_eq!(MaterialGap::new(1).with_size(4.0).size, 4.0);
    }
}
