//! Upstream `widgets/overflow_bar.dart`: a row of children that gives up on
//! being a row.
//!
//! The problem it solves is a dialog's buttons. A row of them reads best, and
//! usually fits; at a large text scale, or in a language whose labels are
//! longer, it does not. A `Row` that does not fit overflows -- it paints past
//! its own edge and complains. This lays the children out in a row when they
//! fit and stacks them in a column when they do not, and that is its whole
//! job: there is no partial wrapping, no eliding, and no shrinking.
//!
//! Which is what makes it different from [`crate::render::RenderWrap`], the
//! other thing here that reflows. A wrap packs as many children as fit onto
//! each line and starts a new one; this is all-or-nothing. For buttons that
//! is the right answer -- three buttons across two lines look like a mistake,
//! where three buttons down one column look deliberate.

use crate::render::{
    BoxConstraints, BoxedRender, HitTestResult, MainAxisAlignment, Offset, PaintContext, RenderBox,
    RenderRef, Size, UpdateEffect, VerticalDirection,
};

use crate::direction::TextDirection;

/// Where the children sit once the bar has given up and stacked them.
///
/// Upstream `OverflowBarAlignment`. Named in reading order rather than by
/// edge: `Start` is the left edge in a left-to-right subtree and the right
/// edge in a right-to-left one, which is why this is not a
/// [`crate::render::CrossAxisAlignment`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverflowBarAlignment {
    #[default]
    Start,
    End,
    Center,
}

/// Upstream `_RenderOverflowBar`: the layout itself.
pub struct RenderOverflowBar {
    /// Between children while they are still a row. Upstream's `spacing`.
    spacing: f32,
    /// How the row distributes the width it was given.
    ///
    /// Upstream's `alignment` is *nullable*, and the null is not a synonym for
    /// `start`: with no alignment the bar takes only the width its children
    /// need, and with one it takes all the width it is offered so that there
    /// is something to distribute. So this stays an `Option`.
    alignment: Option<MainAxisAlignment>,
    /// Between children once they are a column. Upstream's `overflowSpacing`,
    /// separate from `spacing` because a vertical gap that reads right is not
    /// the horizontal one.
    overflow_spacing: f32,
    overflow_alignment: OverflowBarAlignment,
    /// Which end the column starts from. Upstream's `overflowDirection`:
    /// `Up` puts the *last* child on top, which is how a dialog keeps its
    /// confirming button nearest the thumb once the buttons stack.
    overflow_direction: VerticalDirection,
    text_direction: TextDirection,
    children: Vec<BoxedRender>,
    /// Where each child ended up, filled in by layout.
    offsets: Vec<Offset>,
    size: Size,
}

impl RenderOverflowBar {
    pub fn new() -> RenderOverflowBar {
        RenderOverflowBar {
            spacing: 0.0,
            alignment: None,
            overflow_spacing: 0.0,
            overflow_alignment: OverflowBarAlignment::Start,
            overflow_direction: VerticalDirection::Down,
            text_direction: crate::direction::current_direction(),
            children: Vec::new(),
            offsets: Vec::new(),
            size: Size::ZERO,
        }
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Sets the row's alignment, which also makes the bar take the whole
    /// width it is offered -- see the field.
    pub fn with_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    pub fn with_overflow_spacing(mut self, spacing: f32) -> Self {
        self.overflow_spacing = spacing;
        self
    }

    pub fn with_overflow_alignment(mut self, alignment: OverflowBarAlignment) -> Self {
        self.overflow_alignment = alignment;
        self
    }

    pub fn with_overflow_direction(mut self, direction: VerticalDirection) -> Self {
        self.overflow_direction = direction;
        self
    }

    pub fn with_text_direction(mut self, direction: TextDirection) -> Self {
        self.text_direction = direction;
        self
    }

    pub fn push(mut self, child: impl RenderBox + 'static) -> Self {
        self.children.push(RenderRef::new(child));
        self
    }

    pub fn push_boxed(mut self, child: BoxedRender) -> Self {
        self.children.push(child);
        self
    }

    pub fn child_offsets(&self) -> &[Offset] {
        &self.offsets
    }

    /// Whether the children, laid out as a row, would be wider than `limit` --
    /// the one question the whole class turns on.
    pub fn overflows(&self, widths: impl IntoIterator<Item = f32>, limit: f32) -> bool {
        self.row_width(widths) > limit
    }

    fn row_width(&self, widths: impl IntoIterator<Item = f32>) -> f32 {
        let total: f32 = widths.into_iter().sum();
        total + self.spacing * self.gaps()
    }

    /// How many gaps sit between the children. Upstream writes
    /// `spacing * (childCount - 1)` with `childCount` known to be at least one
    /// at every call site; this says the same without underflowing.
    fn gaps(&self) -> f32 {
        (self.children.len() as f32 - 1.0).max(0.0)
    }

    /// Where a stacked child's left edge goes.
    ///
    /// Upstream's switch, and the reason it is a switch rather than a factor:
    /// `Start` and `End` swap under a right-to-left direction, `Center` does
    /// not.
    fn overflow_x(&self, available: f32, child_width: f32) -> f32 {
        let rtl = self.text_direction == TextDirection::Rtl;
        match self.overflow_alignment {
            OverflowBarAlignment::Center => (available - child_width) / 2.0,
            OverflowBarAlignment::Start => {
                if rtl {
                    available - child_width
                } else {
                    0.0
                }
            }
            OverflowBarAlignment::End => {
                if rtl {
                    0.0
                } else {
                    available - child_width
                }
            }
        }
    }

    /// The order the children are stacked in: down the list, or up it.
    fn overflow_order(&self) -> Vec<usize> {
        match self.overflow_direction {
            VerticalDirection::Down => (0..self.children.len()).collect(),
            VerticalDirection::Up => (0..self.children.len()).rev().collect(),
        }
    }

    /// The row's starting x and the gap it puts between children, for a bar
    /// of `width` holding children totalling `children_width`.
    ///
    /// Upstream's alignment switch. The three `space*` alignments turn slack
    /// into gap rather than into a leading offset, which is why this answers
    /// both at once.
    fn row_placement(&self, width: f32, children_width: f32, first_width: f32) -> (f32, f32) {
        let rtl = self.text_direction == TextDirection::Rtl;
        let count = self.children.len() as f32;
        let actual_width = children_width + self.spacing * self.gaps();
        let start = |first: f32| if rtl { width - first } else { 0.0 };
        match self.alignment {
            // No alignment at all lays out from the start edge, exactly as
            // `Start` does. What differs is the *width* the bar took, decided
            // before this is called.
            None | Some(MainAxisAlignment::Start) => (start(first_width), self.spacing),
            Some(MainAxisAlignment::Center) => {
                let half = (width - actual_width) / 2.0;
                let x = if rtl {
                    width - half - first_width
                } else {
                    half
                };
                (x, self.spacing)
            }
            Some(MainAxisAlignment::End) => {
                let x = if rtl {
                    actual_width - first_width
                } else {
                    width - actual_width
                };
                (x, self.spacing)
            }
            Some(MainAxisAlignment::SpaceBetween) => {
                let gap = (width - children_width) / (count - 1.0).max(1.0);
                (start(first_width), gap)
            }
            Some(MainAxisAlignment::SpaceAround) => {
                let gap = if count > 0.0 {
                    (width - children_width) / count
                } else {
                    0.0
                };
                let x = if rtl {
                    width - gap / 2.0 - first_width
                } else {
                    gap / 2.0
                };
                (x, gap)
            }
            Some(MainAxisAlignment::SpaceEvenly) => {
                let gap = (width - children_width) / (count + 1.0);
                let x = if rtl { width - gap - first_width } else { gap };
                (x, gap)
            }
        }
    }

    /// The size and the offsets for children of the given sizes, which is all
    /// of layout that does not touch a child. Shared by
    /// [`RenderBox::layout`] and [`RenderBox::compute_dry_layout`] so the two
    /// cannot drift.
    fn place(&self, constraints: BoxConstraints, sizes: &[Size]) -> (Size, Vec<Offset>) {
        if sizes.is_empty() {
            return (constraints.smallest(), Vec::new());
        }
        let children_width: f32 = sizes.iter().map(|size| size.width).sum();
        let max_child_height = sizes.iter().map(|size| size.height).fold(0.0, f32::max);
        let actual_width = children_width + self.spacing * self.gaps();
        let mut offsets = vec![Offset::ZERO; sizes.len()];

        if actual_width > constraints.max_width {
            // Stacked. Each child gets the full width to place itself in --
            // note it is the *constraint's* width, not the widest child's, so
            // an `End` alignment reaches the bar's own right edge.
            let mut y = 0.0;
            for index in self.overflow_order() {
                let size = sizes[index];
                offsets[index] = Offset::new(self.overflow_x(constraints.max_width, size.width), y);
                y += size.height + self.overflow_spacing;
            }
            // One `overflow_spacing` too many was added, by the last child.
            let size =
                constraints.constrain(Size::new(constraints.max_width, y - self.overflow_spacing));
            return (size, offsets);
        }

        // A row. With no alignment the bar is exactly as wide as its children
        // need; with one it takes everything, because an alignment is an
        // answer to "what do I do with the slack" and there is no slack in a
        // shrink-wrapped bar.
        let overall_width = match self.alignment {
            None => actual_width,
            Some(_) => constraints.max_width,
        };
        let size = constraints.constrain(Size::new(overall_width, max_child_height));
        let (mut x, gap) = self.row_placement(size.width, children_width, sizes[0].width);
        let rtl = self.text_direction == TextDirection::Rtl;
        for index in 0..sizes.len() {
            // Children are centred against the tallest, which is what makes a
            // row of buttons of different heights read as one bar.
            offsets[index] = Offset::new(x, (max_child_height - sizes[index].height) / 2.0);
            // `x` is a left edge, so advancing it left-to-right adds *this*
            // child's width and advancing it right-to-left subtracts the
            // *next* one's. Upstream's asymmetry, and it is not a quirk: the
            // left edge of the next child right-to-left depends on how wide
            // that child is, which the current one cannot say.
            if !rtl {
                x += sizes[index].width + gap;
            } else if let Some(next) = sizes.get(index + 1) {
                x -= next.width + gap;
            }
        }
        (size, offsets)
    }

    /// Upstream's two `computeIntrinsicHeight` bodies, which differ only in
    /// which height they ask each child for.
    fn intrinsic_height(&self, width: f32, max: bool) -> f32 {
        if self.children.is_empty() {
            return 0.0;
        }
        // Note the *minimum* intrinsic widths, even when answering about
        // maximum heights: upstream asks `getMinIntrinsicWidth` in both. The
        // question being answered is "would this stack", and a bar stacks
        // only when it cannot fit even at the children's narrowest.
        let bar_width = self.row_width(
            self.children
                .iter()
                .map(|child| child.min_intrinsic_width(f32::INFINITY)),
        );
        let heights = self.children.iter().map(|child| {
            if max {
                child.max_intrinsic_height(width)
            } else {
                child.min_intrinsic_height(width)
            }
        });
        if bar_width > width {
            heights.sum::<f32>() + self.overflow_spacing * self.gaps()
        } else {
            heights.fold(0.0, f32::max)
        }
    }
}

impl Default for RenderOverflowBar {
    fn default() -> RenderOverflowBar {
        RenderOverflowBar::new()
    }
}

impl RenderBox for RenderOverflowBar {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderOverflowBar>()?;
        let same_children = self.children.len() == fresh.children.len()
            && self
                .children
                .iter()
                .zip(&fresh.children)
                .all(|(a, b)| a.is(b));
        let effect = UpdateEffect::relayout_if(
            self.spacing != fresh.spacing
                || self.alignment != fresh.alignment
                || self.overflow_spacing != fresh.overflow_spacing
                || self.overflow_alignment != fresh.overflow_alignment
                || self.overflow_direction != fresh.overflow_direction
                || self.text_direction != fresh.text_direction
                || !same_children,
        );
        self.spacing = fresh.spacing;
        self.alignment = fresh.alignment;
        self.overflow_spacing = fresh.overflow_spacing;
        self.overflow_alignment = fresh.overflow_alignment;
        self.overflow_direction = fresh.overflow_direction;
        self.text_direction = fresh.text_direction;
        self.children = std::mem::take(&mut fresh.children);
        Some(effect)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if self.children.is_empty() {
            self.offsets.clear();
            self.size = constraints.smallest();
            return self.size;
        }
        // Loosened: every child is measured at the size it wants, and only
        // then does the bar decide whether they all fit. Measuring them
        // against the bar's own width would let a wide child shrink itself
        // and hide the overflow the bar exists to notice.
        let child_constraints = constraints.loosen();
        let sizes: Vec<Size> = self
            .children
            .iter_mut()
            .map(|child| child.layout_child(child_constraints, true))
            .collect();
        let (size, offsets) = self.place(constraints, &sizes);
        self.offsets = offsets;
        self.size = size;
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        if self.children.is_empty() {
            return constraints.smallest();
        }
        let child_constraints = constraints.loosen();
        let sizes: Vec<Size> = self
            .children
            .iter()
            .map(|child| child.dry_layout(child_constraints))
            .collect();
        self.place(constraints, &sizes).0
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        for (child, child_offset) in self.children.iter().zip(&self.offsets) {
            context.paint_child(child, offset.plus(*child_offset));
        }
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        for (child, child_offset) in self.children.iter().zip(&self.offsets) {
            visit(child, *child_offset);
        }
    }

    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        for (child, child_offset) in self.children.iter().zip(&self.offsets).rev() {
            if child.hit_test(position.minus(*child_offset), result) {
                return true;
            }
        }
        false
    }

    // Both widths are the row's width, whether or not the bar would actually
    // be a row at that size: an intrinsic width is what the content wants,
    // and what this content wants is to be a row.

    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        if self.children.is_empty() {
            return 0.0;
        }
        self.row_width(
            self.children
                .iter()
                .map(|child| child.min_intrinsic_width(f32::INFINITY)),
        )
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        if self.children.is_empty() {
            return 0.0;
        }
        self.row_width(
            self.children
                .iter()
                .map(|child| child.max_intrinsic_width(f32::INFINITY)),
        )
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.intrinsic_height(width, false)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.intrinsic_height(width, true)
    }
}

/// Upstream `OverflowBar`, the widget: a row of children that becomes a
/// column when the row does not fit.
///
/// A facade over [`RenderOverflowBar`], as [`crate::widgets::Wrap`] is over
/// `RenderWrap` -- upstream's widget carries nothing the render object does
/// not, save the ambient text direction, which [`RenderOverflowBar::new`]
/// reads at construction for the same reason upstream reads it in
/// `createRenderObject`.
pub struct OverflowBar;

impl OverflowBar {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> RenderOverflowBar {
        RenderOverflowBar::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::RenderConstrainedBox;

    /// A child of a fixed size, which is all these tests need from one.
    fn box_of(width: f32, height: f32) -> RenderConstrainedBox {
        RenderConstrainedBox::new(BoxConstraints::new(width, width, height, height))
    }

    fn wide(max_width: f32) -> BoxConstraints {
        BoxConstraints::new(0.0, max_width, 0.0, f32::INFINITY)
    }

    #[test]
    fn children_that_fit_are_a_row() {
        let mut bar = OverflowBar::new()
            .with_spacing(10.0)
            .push(box_of(40.0, 20.0))
            .push(box_of(60.0, 30.0));
        let size = bar.layout(wide(500.0));
        // Shrink-wrapped: with no alignment the bar is exactly as wide as the
        // children plus the one gap, not as wide as it was offered.
        assert_eq!(size, Size::new(110.0, 30.0));
        assert_eq!(bar.child_offsets()[0], Offset::new(0.0, 5.0));
        assert_eq!(bar.child_offsets()[1], Offset::new(50.0, 0.0));
    }

    #[test]
    fn a_row_centres_its_children_against_the_tallest() {
        // Which is what makes buttons of different heights read as one bar
        // rather than as a ragged line.
        let mut bar = OverflowBar::new()
            .push(box_of(10.0, 10.0))
            .push(box_of(10.0, 50.0));
        bar.layout(wide(500.0));
        assert_eq!(bar.child_offsets()[0].dy, 20.0);
        assert_eq!(bar.child_offsets()[1].dy, 0.0);
    }

    #[test]
    fn children_that_do_not_fit_are_a_column() {
        // All of them, not the ones that did not fit: this is all-or-nothing,
        // which is the whole difference from a wrap. Two buttons on one line
        // and a third below reads as a mistake.
        let mut bar = OverflowBar::new()
            .with_overflow_spacing(4.0)
            .push(box_of(80.0, 20.0))
            .push(box_of(80.0, 20.0));
        let size = bar.layout(wide(100.0));
        assert_eq!(size, Size::new(100.0, 44.0));
        assert_eq!(bar.child_offsets()[0], Offset::new(0.0, 0.0));
        assert_eq!(bar.child_offsets()[1], Offset::new(0.0, 24.0));
    }

    #[test]
    fn the_spacing_is_what_tips_a_row_into_a_column() {
        // Two 50-wide children fit in 100 exactly -- until the gap between
        // them is counted, which is the point of counting it.
        let fits = |spacing: f32| {
            let mut bar = OverflowBar::new()
                .with_spacing(spacing)
                .push(box_of(50.0, 20.0))
                .push(box_of(50.0, 20.0));
            bar.layout(wide(100.0)).height == 20.0
        };
        assert!(fits(0.0));
        assert!(!fits(1.0));
    }

    #[test]
    fn a_stacked_bar_places_children_against_its_own_edge_not_the_widest_child() {
        // The available width is the constraint's, so an `End` alignment
        // reaches the bar's right edge even when every child is narrow.
        let mut bar = OverflowBar::new()
            .with_overflow_alignment(OverflowBarAlignment::End)
            .with_spacing(200.0)
            .push(box_of(20.0, 10.0))
            .push(box_of(40.0, 10.0));
        bar.layout(wide(100.0));
        assert_eq!(bar.child_offsets()[0].dx, 80.0);
        assert_eq!(bar.child_offsets()[1].dx, 60.0);
    }

    #[test]
    fn stacking_upward_puts_the_last_child_on_top() {
        // Which is what a dialog wants once its buttons stack: the confirming
        // action, written last, ends up nearest the top of the stack.
        let mut bar = OverflowBar::new()
            .with_spacing(200.0)
            .with_overflow_direction(VerticalDirection::Up)
            .push(box_of(20.0, 10.0))
            .push(box_of(20.0, 30.0));
        bar.layout(wide(100.0));
        assert_eq!(bar.child_offsets()[1].dy, 0.0, "the last child is on top");
        assert_eq!(bar.child_offsets()[0].dy, 30.0);
    }

    #[test]
    fn an_alignment_makes_the_bar_take_the_whole_width() {
        // Upstream's null alignment is not a synonym for `start`: without one
        // the bar shrink-wraps, and with one it takes everything, because an
        // alignment is an answer to what to do with the slack.
        let mut shrink_wrapped = OverflowBar::new().push(box_of(40.0, 20.0));
        assert_eq!(shrink_wrapped.layout(wide(500.0)).width, 40.0);

        let mut aligned = OverflowBar::new()
            .with_alignment(MainAxisAlignment::Start)
            .push(box_of(40.0, 20.0));
        assert_eq!(aligned.layout(wide(500.0)).width, 500.0);
        // And `start` still lays out from the left, so the difference is
        // visible only in the bar's own width.
        assert_eq!(aligned.child_offsets()[0].dx, 0.0);
    }

    #[test]
    fn end_alignment_pushes_the_row_to_the_far_edge() {
        let mut bar = OverflowBar::new()
            .with_alignment(MainAxisAlignment::End)
            .with_spacing(10.0)
            .push(box_of(40.0, 20.0))
            .push(box_of(50.0, 20.0));
        bar.layout(wide(300.0));
        // 300 - (40 + 10 + 50) = 200.
        assert_eq!(bar.child_offsets()[0].dx, 200.0);
        assert_eq!(bar.child_offsets()[1].dx, 250.0);
    }

    #[test]
    fn the_space_alignments_turn_slack_into_gap_rather_than_into_an_offset() {
        let mut between = OverflowBar::new()
            .with_alignment(MainAxisAlignment::SpaceBetween)
            .push(box_of(20.0, 10.0))
            .push(box_of(20.0, 10.0));
        between.layout(wide(100.0));
        assert_eq!(between.child_offsets()[0].dx, 0.0);
        assert_eq!(between.child_offsets()[1].dx, 80.0);

        let mut evenly = OverflowBar::new()
            .with_alignment(MainAxisAlignment::SpaceEvenly)
            .push(box_of(20.0, 10.0))
            .push(box_of(20.0, 10.0));
        evenly.layout(wide(100.0));
        // Three equal gaps of 20.
        assert_eq!(evenly.child_offsets()[0].dx, 20.0);
        assert_eq!(evenly.child_offsets()[1].dx, 60.0);
    }

    #[test]
    fn right_to_left_lays_the_row_out_from_the_right_edge() {
        let mut bar = OverflowBar::new()
            .with_text_direction(TextDirection::Rtl)
            .with_spacing(10.0)
            .push(box_of(40.0, 20.0))
            .push(box_of(50.0, 20.0));
        bar.layout(wide(500.0));
        // The bar is 100 wide (shrink-wrapped). The first child sits at its
        // right edge, and the second to the *left* of it -- which is why the
        // advance subtracts the next child's width rather than this one's:
        // an offset is a left edge, and the next child's left edge depends on
        // how wide that child is.
        assert_eq!(bar.child_offsets()[0].dx, 60.0);
        assert_eq!(bar.child_offsets()[1].dx, 0.0);
    }

    #[test]
    fn right_to_left_swaps_start_and_end_but_not_centre_when_stacked() {
        let stacked_at = |alignment: OverflowBarAlignment| {
            let mut bar = OverflowBar::new()
                .with_text_direction(TextDirection::Rtl)
                .with_overflow_alignment(alignment)
                .with_spacing(200.0)
                .push(box_of(20.0, 10.0))
                .push(box_of(20.0, 10.0));
            bar.layout(wide(100.0));
            bar.child_offsets()[0].dx
        };
        assert_eq!(stacked_at(OverflowBarAlignment::Start), 80.0);
        assert_eq!(stacked_at(OverflowBarAlignment::End), 0.0);
        assert_eq!(stacked_at(OverflowBarAlignment::Center), 40.0);
    }

    #[test]
    fn an_empty_bar_is_the_smallest_it_may_be() {
        let mut bar = OverflowBar::new().with_spacing(50.0);
        assert_eq!(
            bar.layout(BoxConstraints::new(10.0, 100.0, 5.0, 50.0)),
            Size::new(10.0, 5.0)
        );
        assert!(bar.child_offsets().is_empty());
        // And the spacing has nothing to sit between, so it contributes
        // nothing rather than one negative gap.
        assert_eq!(bar.min_intrinsic_width(f32::INFINITY), 0.0);
    }

    #[test]
    fn one_child_has_no_gaps() {
        let bar = OverflowBar::new()
            .with_spacing(50.0)
            .push(box_of(30.0, 10.0));
        assert_eq!(bar.min_intrinsic_width(f32::INFINITY), 30.0);
    }

    #[test]
    fn the_intrinsic_width_is_the_rows_even_where_the_bar_would_stack() {
        // An intrinsic width is what the content wants, and what this content
        // wants is to be a row. Asking at a width narrow enough to stack does
        // not change the answer -- the parameter is ignored, as upstream
        // ignores it.
        let bar = OverflowBar::new()
            .with_spacing(10.0)
            .push(box_of(40.0, 20.0))
            .push(box_of(50.0, 20.0));
        assert_eq!(bar.min_intrinsic_width(f32::INFINITY), 100.0);
        assert_eq!(bar.max_intrinsic_width(0.0), 100.0);
    }

    #[test]
    fn the_intrinsic_height_asks_whether_the_bar_would_stack() {
        let bar = OverflowBar::new()
            .with_overflow_spacing(4.0)
            .push(box_of(40.0, 20.0))
            .push(box_of(50.0, 30.0));
        // Wide enough to be a row: the tallest child.
        assert_eq!(bar.min_intrinsic_height(200.0), 30.0);
        // Too narrow: both children plus the gap between them.
        assert_eq!(bar.min_intrinsic_height(50.0), 54.0);
    }

    #[test]
    fn the_maximum_height_decides_it_would_stack_from_the_minimum_widths() {
        // Upstream asks `getMinIntrinsicWidth` in both height methods, not
        // the matching maximum. Written down because it looks like a slip and
        // is not: the question is "would this have to stack", and a bar has
        // to stack only when it cannot fit even at its children's narrowest.
        let bar = OverflowBar::new()
            .push(box_of(40.0, 20.0))
            .push(box_of(50.0, 30.0));
        assert_eq!(bar.max_intrinsic_height(200.0), 30.0);
        assert_eq!(bar.max_intrinsic_height(50.0), 50.0);
    }

    #[test]
    fn the_dry_layout_agrees_with_the_real_one() {
        let build = || {
            OverflowBar::new()
                .with_spacing(10.0)
                .with_overflow_spacing(4.0)
                .push(box_of(80.0, 20.0))
                .push(box_of(90.0, 30.0))
        };
        for limit in [500.0, 180.0, 100.0] {
            let mut laid_out = build();
            let dry = build();
            assert_eq!(
                dry.compute_dry_layout(wide(limit)),
                laid_out.layout(wide(limit)),
                "at a limit of {limit}"
            );
        }
    }
}
