// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The sliver widgets, from upstream `widgets/sliver.dart`.
//!
//! The sliver *render* objects landed with the P3 wave; this is the layer
//! above them, in the same shape as [`crate::widgets`]: one name apiece,
//! upstream's constructor with upstream's defaults, over a render object
//! that is already here.
//!
//! # Where the child manager went
//!
//! Upstream a lazy sliver is a widget, an element and a render object: the
//! element is the child manager, and the render object calls back into it to
//! build, dispose and reposition children. Here the render object takes the
//! builder directly ([`RenderSliverList::new`]), so the manager and the
//! element are one thing and there is no `SliverMultiBoxAdaptorElement` to
//! name -- it is ledgered, along with the two abstract widget bases whose
//! whole content is that relationship.

use std::rc::Rc;

use crate::render::{
    BoxedRender, ProxySliverBehavior, RenderBox, RenderProxySliver, RenderRef, RenderSliverGrid,
    RenderSliverList, SliverGridDelegate, SliverGroupChild,
};
use crate::render::{RenderSliverCrossAxisGroup, RenderSliverMainAxisGroup};

/// Upstream `SliverList`: children built on demand, each measured.
pub struct SliverList;

impl SliverList {
    /// Upstream `SliverList(delegate: SliverChildBuilderDelegate(builder,
    /// childCount: count))`, which is the only spelling that is actually
    /// lazy.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        child_count: usize,
        build_child: impl Fn(usize) -> RenderRef + 'static,
    ) -> RenderSliverList {
        RenderSliverList::new(child_count, build_child)
    }

    /// Upstream `SliverList.list`: children given rather than built. They
    /// are all built already, so nothing is lazy about it -- upstream says
    /// as much in its own documentation.
    pub fn list(children: Vec<BoxedRender>) -> RenderSliverList {
        let children = Rc::new(children);
        RenderSliverList::new(children.len(), move |index| children[index].clone())
    }
}

/// Upstream `SliverFixedExtentList`: every child the same extent, so the
/// window is arithmetic rather than measurement.
pub struct SliverFixedExtentList;

impl SliverFixedExtentList {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        child_count: usize,
        item_extent: f32,
        build_child: impl Fn(usize) -> RenderRef + 'static,
    ) -> RenderSliverList {
        RenderSliverList::new(child_count, build_child).with_item_extent(item_extent)
    }
}

/// Upstream `SliverGrid`: children laid out in tiles by a delegate.
pub struct SliverGrid;

impl SliverGrid {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        child_count: usize,
        delegate: SliverGridDelegate,
        build_child: impl Fn(usize) -> RenderRef + 'static,
    ) -> RenderSliverGrid {
        RenderSliverGrid::new(child_count, delegate, build_child)
    }

    /// Upstream `SliverGrid.count`: a fixed number of columns.
    pub fn count(
        cross_axis_count: usize,
        child_count: usize,
        build_child: impl Fn(usize) -> RenderRef + 'static,
    ) -> RenderSliverGrid {
        RenderSliverGrid::new(
            child_count,
            SliverGridDelegate::fixed_cross_axis_count(cross_axis_count),
            build_child,
        )
    }

    /// Upstream `SliverGrid.extent`: as many columns as fit at that width.
    pub fn extent(
        max_cross_axis_extent: f32,
        child_count: usize,
        build_child: impl Fn(usize) -> RenderRef + 'static,
    ) -> RenderSliverGrid {
        RenderSliverGrid::new(
            child_count,
            SliverGridDelegate::max_cross_axis_extent(max_cross_axis_extent),
            build_child,
        )
    }
}

/// Upstream `SliverOpacity`: the sliver painted at an opacity, and not
/// painted at all at zero.
pub struct SliverOpacity;

impl SliverOpacity {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(opacity: f32, sliver: impl RenderBox + 'static) -> RenderProxySliver {
        RenderProxySliver::new(ProxySliverBehavior::Opacity(opacity), sliver)
    }
}

/// Upstream `SliverIgnorePointer`: laid out and painted, never hit.
pub struct SliverIgnorePointer;

impl SliverIgnorePointer {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(ignoring: bool, sliver: impl RenderBox + 'static) -> RenderProxySliver {
        RenderProxySliver::new(
            if ignoring {
                ProxySliverBehavior::IgnorePointer
            } else {
                ProxySliverBehavior::PassThrough
            },
            sliver,
        )
    }
}

/// Upstream `SliverOffstage`: laid out, never painted, never hit.
pub struct SliverOffstage;

impl SliverOffstage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(offstage: bool, sliver: impl RenderBox + 'static) -> RenderProxySliver {
        RenderProxySliver::new(
            if offstage {
                ProxySliverBehavior::Offstage
            } else {
                ProxySliverBehavior::PassThrough
            },
            sliver,
        )
    }
}

/// Upstream `SliverConstrainedCrossAxis`: the sliver laid against the
/// smaller of `max_extent` and the cross extent it was given.
pub struct SliverConstrainedCrossAxis;

impl SliverConstrainedCrossAxis {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(max_extent: f32, sliver: impl RenderBox + 'static) -> RenderProxySliver {
        RenderProxySliver::new(
            ProxySliverBehavior::ConstrainedCrossAxis(max_extent),
            sliver,
        )
    }
}

/// Upstream `SliverCrossAxisExpanded`: a group child that takes a share of
/// whatever the fixed-extent children left, rather than its own extent.
///
/// Upstream it is a `ParentDataWidget` the group reads off its child; this
/// crate's group takes its children's flex directly, so this is the pairing.
pub struct SliverCrossAxisExpanded;

impl SliverCrossAxisExpanded {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(flex: usize, sliver: impl RenderBox + 'static) -> SliverGroupChild {
        debug_assert!(flex > 0, "an expanded child takes a share, so at least one");
        SliverGroupChild {
            sliver: RenderRef::new(sliver),
            cross_axis_flex: flex,
        }
    }

    /// A child that keeps its own cross extent -- upstream's plain sliver in
    /// the group's `slivers` list, whose flex is zero.
    pub fn fixed(sliver: impl RenderBox + 'static) -> SliverGroupChild {
        SliverGroupChild {
            sliver: RenderRef::new(sliver),
            cross_axis_flex: 0,
        }
    }
}

/// Upstream `SliverCrossAxisGroup`: slivers side by side across the cross
/// axis, each seeing the whole main axis.
pub struct SliverCrossAxisGroup;

impl SliverCrossAxisGroup {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(children: Vec<SliverGroupChild>) -> RenderSliverCrossAxisGroup {
        RenderSliverCrossAxisGroup::new(children)
    }
}

/// Upstream `SliverMainAxisGroup`: slivers one after another along the main
/// axis, as one sliver.
pub struct SliverMainAxisGroup;

impl SliverMainAxisGroup {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(slivers: Vec<BoxedRender>) -> RenderSliverMainAxisGroup {
        RenderSliverMainAxisGroup::new(
            slivers
                .into_iter()
                .map(|sliver| SliverGroupChild {
                    sliver,
                    cross_axis_flex: 0,
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{
        AxisDirection, BoxConstraints, GrowthDirection, HitTestResult, Offset, RenderDecoratedBox,
        Size, SliverConstraints,
    };

    /// A fixed-size box that can be hit, for standing in as a list child.
    fn child(height: f32) -> RenderRef {
        RenderRef::new(
            RenderDecoratedBox::new().with_child(crate::widgets::SizedBox::new(40.0, height)),
        )
    }

    fn viewport(scroll_offset: f32, remaining: f32) -> SliverConstraints {
        SliverConstraints {
            axis_direction: AxisDirection::Down,
            cross_axis_direction: AxisDirection::Right,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: crate::scrolling::ScrollDirection::Forward,
            scroll_offset,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: remaining,
            remaining_cache_extent: remaining,
            cache_origin: 0.0,
            cross_axis_extent: 100.0,
            viewport_main_axis_extent: remaining,
        }
    }

    #[test]
    fn a_sliver_list_builds_only_the_window_it_is_asked_for() {
        let built = Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = Rc::clone(&built);
        let mut list = SliverList::new(100, move |index| {
            sink.borrow_mut().push(index);
            child(20.0)
        });
        list.sliver_layout(viewport(0.0, 60.0));
        // Three twenties fill sixty; the hundred children behind them were
        // never built.
        assert_eq!(*built.borrow(), vec![0, 1, 2]);
    }

    #[test]
    fn a_fixed_extent_list_places_by_arithmetic() {
        let mut list = SliverFixedExtentList::new(10, 25.0, |_| child(25.0));
        let geometry = list.sliver_layout(viewport(0.0, 50.0));
        assert_eq!(geometry.scroll_extent, 250.0, "ten children of twenty-five");
    }

    #[test]
    fn a_grid_by_count_divides_the_cross_extent() {
        let mut grid = SliverGrid::count(2, 4, |_| child(50.0));
        grid.sliver_layout(viewport(0.0, 200.0));
        // Two columns of fifty across a hundred, and four children in two
        // rows: a hundred of scroll extent.
        assert_eq!(grid.sliver_geometry().scroll_extent, 100.0);
    }

    #[test]
    fn an_opaque_sliver_at_zero_is_neither_painted_nor_hit() {
        // Slivers are hit through the sliver protocol, by main- and
        // cross-axis position, not by an offset in a box.
        let mut invisible = SliverOpacity::new(0.0, SliverList::list(vec![child(30.0)]));
        invisible.sliver_layout(viewport(0.0, 100.0));
        let mut result = HitTestResult::new();
        assert!(!invisible.sliver_hit_test(10.0, 10.0, &mut result));

        let mut visible = SliverOpacity::new(1.0, SliverList::list(vec![child(30.0)]));
        visible.sliver_layout(viewport(0.0, 100.0));
        let mut hit = HitTestResult::new();
        assert!(visible.sliver_hit_test(10.0, 10.0, &mut hit));
    }

    #[test]
    fn an_offstage_sliver_reports_no_geometry_at_all() {
        let mut offstage = SliverOffstage::new(true, SliverList::list(vec![child(30.0)]));
        let geometry = offstage.sliver_layout(viewport(0.0, 100.0));
        assert_eq!(geometry.paint_extent, 0.0);
        assert_eq!(geometry.scroll_extent, 0.0);

        // Not offstage, it is its child.
        let mut onstage = SliverOffstage::new(false, SliverList::list(vec![child(30.0)]));
        assert_eq!(
            onstage.sliver_layout(viewport(0.0, 100.0)).scroll_extent,
            30.0
        );
    }

    #[test]
    fn a_constrained_cross_axis_takes_the_smaller_of_the_two() {
        let mut narrow = SliverConstrainedCrossAxis::new(40.0, SliverList::list(vec![child(30.0)]));
        narrow.sliver_layout(viewport(0.0, 100.0));
        assert_eq!(narrow.sliver_geometry().cross_axis_extent, Some(40.0));

        // Asking for more than the viewport has gets the viewport's.
        let mut wide = SliverConstrainedCrossAxis::new(500.0, SliverList::list(vec![child(30.0)]));
        wide.sliver_layout(viewport(0.0, 100.0));
        assert_eq!(wide.sliver_geometry().cross_axis_extent, Some(100.0));
    }

    #[test]
    fn a_cross_axis_group_shares_what_the_fixed_children_left() {
        let mut group = SliverCrossAxisGroup::new(vec![
            SliverCrossAxisExpanded::fixed(SliverConstrainedCrossAxis::new(
                20.0,
                SliverList::list(vec![child(30.0)]),
            )),
            SliverCrossAxisExpanded::new(1, SliverList::list(vec![child(30.0)])),
        ]);
        group.sliver_layout(viewport(0.0, 100.0));
        // Twenty to the fixed child, the remaining eighty to the expanded
        // one -- which is where the second child is placed across.
        let mut offsets = Vec::new();
        group.visit_children(&mut |_, offset| offsets.push(offset.dx));
        assert_eq!(offsets, vec![0.0, 20.0]);
    }

    #[test]
    fn a_main_axis_group_is_its_children_end_to_end() {
        let mut group = SliverMainAxisGroup::new(vec![
            RenderRef::new(SliverList::list(vec![child(30.0)])),
            RenderRef::new(SliverList::list(vec![child(40.0)])),
        ]);
        let geometry = group.sliver_layout(viewport(0.0, 200.0));
        assert_eq!(geometry.scroll_extent, 70.0);
    }

    #[test]
    fn a_sliver_list_measured_against_a_box_constraint_still_answers() {
        // The sliver protocol here is a method on the box protocol, so a
        // sliver asked the box question answers rather than failing -- which
        // is what lets `SliverToBoxAdapter` be the identity.
        let mut list = SliverList::list(vec![child(30.0)]);
        let size = list.layout(BoxConstraints::loose(100.0, 100.0));
        assert!(size.width >= 0.0 && size.height >= 0.0);
        assert_ne!(size, Size::new(f32::NAN, f32::NAN));
    }
}
