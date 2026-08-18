// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! A lazy grid: a scrollable, two-dimensional array of tiles that exist only
//! while they are on the glass or nearly so.
//!
//! Upstream this is `GridView` (`widgets/scroll_view.dart`) over `SliverGrid`
//! over `RenderSliverGrid` (`rendering/sliver_grid.dart`). This file is the
//! widget half, [`GridView`]; the render half is [`crate::render`]'s
//! [`RenderSliverGrid`], beside the other slivers it shares a protocol with,
//! and between them sits the delegate that decides how big every tile is and
//! where it sits: [`SliverGridDelegate`]. All three are re-exported here.
//!
//! A grid is the one lazy container whose window needs no dead reckoning: a
//! tile's position is arithmetic from its index, so the sliver locates its
//! window by division and never asks the viewport for a scroll offset
//! correction -- unlike [`crate::render::RenderSliverList`], whose children are
//! as tall as they are. That is exactly the difference upstream draws between
//! `RenderSliverGrid.performLayout` and `RenderSliverList.performLayout`, and
//! it is why a programmatic jump of any distance costs a grid one screenful,
//! same as a smooth scroll.
//!
//! What upstream has and this port does not, deliberately:
//!
//! * **Custom delegates.** Upstream's `SliverGridDelegate` is an open class and
//!   `SliverGridLayout` an open interface; here the delegate is an enum of the
//!   two layouts upstream ships, and the layout is the one concrete
//!   [`SliverGridRegularTileLayout`] both of them compute. A third delegate is
//!   a third variant -- the same call this crate made for scroll activities
//!   ([`crate::scrolling::Motion`]).
//! * **keepAlive.** As with the list, a child outside the window is gone.
//! * **The static-children constructors.** Upstream's `GridView.count` and
//!   `GridView.extent` take a `List<Widget>`; here everything goes through the
//!   builder, as it already does for [`crate::scrolling::SliverListView`], so
//!   the constructors take a count and a builder closure.

use std::cell::Cell;
use std::rc::Rc;

use crate::render::{
    self, AxisDirection, BoxConstraints, EdgeInsets, HitTestResult, Offset, PaintContext,
    RenderBox, RenderRef, Size, UpdateEffect,
};
use crate::scrolling::ScrollDirection;

pub use crate::render::{
    RenderSliverGrid, SliverGridDelegate, SliverGridGeometry, SliverGridRegularTileLayout,
};

/// A scrollable, lazy, two-dimensional array of tiles.
///
/// Upstream's `GridView` (`widgets/scroll_view.dart`), over this crate's
/// sliver protocol rather than a `Scrollable` element: the widget describes
/// the grid, and the render half keeps the viewport, the padding sliver and
/// the grid sliver alive across rebuilds exactly as
/// [`crate::scrolling::SliverListView`] does for a list. The constructors are
/// upstream's:
///
/// * [`GridView::count`] is `GridView.count`,
/// * [`GridView::extent`] is `GridView.extent`,
/// * [`GridView::builder`] is `GridView.builder` with an explicit delegate.
///
/// All three are lazy here, including the two that upstream feeds a
/// `SliverChildListDelegate`: this crate has no eager list facade at all, so
/// every constructor takes the child count and the builder closure that
/// upstream's `GridView.builder` takes (`SliverChildBuilderDelegate`), and
/// the item window arithmetic is the same.
///
/// The grid does not scroll itself. Whoever holds the
/// [`crate::scrolling::Scroll`] hands the offset in per frame with
/// [`GridView::with_offset`], and reads how far the grid can scroll back out
/// through [`GridView::with_extent_sink`] -- the same handshake
/// [`crate::scrolling::SliverListView`] documents.
#[derive(Clone)]
pub struct GridView {
    axis_direction: AxisDirection,
    delegate: SliverGridDelegate,
    child_count: usize,
    build_item: Rc<dyn Fn(usize) -> RenderRef>,
    padding: Option<EdgeInsets>,
    offset: f32,
    cache_extent: f32,
    user_scroll_direction: ScrollDirection,
    extent_sink: Option<Rc<Cell<f32>>>,
}

impl GridView {
    /// A grid with a fixed number of tiles in the cross axis, built on
    /// demand. Upstream's `GridView.count` with `GridView.builder`'s child
    /// contract; the delegate modifiers are
    /// [`GridView::with_main_axis_spacing`],
    /// [`GridView::with_cross_axis_spacing`],
    /// [`GridView::with_child_aspect_ratio`] and
    /// [`GridView::with_main_axis_extent`].
    pub fn count(
        cross_axis_count: usize,
        child_count: usize,
        build_item: impl Fn(usize) -> RenderRef + 'static,
    ) -> GridView {
        GridView::builder(
            SliverGridDelegate::fixed_cross_axis_count(cross_axis_count),
            child_count,
            build_item,
        )
    }

    /// A grid whose tiles are as wide as they may be without exceeding
    /// `max_cross_axis_extent`. Upstream's `GridView.extent`.
    pub fn extent(
        max_cross_axis_extent: f32,
        child_count: usize,
        build_item: impl Fn(usize) -> RenderRef + 'static,
    ) -> GridView {
        GridView::builder(
            SliverGridDelegate::max_cross_axis_extent(max_cross_axis_extent),
            child_count,
            build_item,
        )
    }

    /// A grid with an explicit delegate. Upstream's `GridView.builder` with
    /// its required `gridDelegate`.
    pub fn builder(
        delegate: SliverGridDelegate,
        child_count: usize,
        build_item: impl Fn(usize) -> RenderRef + 'static,
    ) -> GridView {
        GridView {
            axis_direction: AxisDirection::Down,
            delegate,
            child_count,
            build_item: Rc::new(build_item),
            padding: None,
            offset: 0.0,
            cache_extent: crate::scrolling::DEFAULT_CACHE_EXTENT,
            user_scroll_direction: ScrollDirection::Idle,
            extent_sink: None,
        }
    }

    /// A horizontal grid. Which way it scrolls follows the ambient text
    /// direction where the grid was built, the same line upstream's
    /// `ScrollView` builds its viewport with: rightward in an LTR subtree,
    /// leftward in an RTL one.
    pub fn horizontal(self) -> GridView {
        let axis_direction =
            if crate::direction::current_direction() == crate::direction::TextDirection::Rtl {
                AxisDirection::Left
            } else {
                AxisDirection::Right
            };
        GridView {
            axis_direction,
            ..self
        }
    }

    /// The gap between tiles along the main axis, on the delegate.
    pub fn with_main_axis_spacing(mut self, spacing: f32) -> Self {
        self.delegate = self.delegate.with_main_axis_spacing(spacing);
        self
    }

    /// The gap between tiles along the cross axis, on the delegate.
    pub fn with_cross_axis_spacing(mut self, spacing: f32) -> Self {
        self.delegate = self.delegate.with_cross_axis_spacing(spacing);
        self
    }

    /// The ratio of each tile's cross-axis to main-axis extent, on the
    /// delegate.
    pub fn with_child_aspect_ratio(mut self, ratio: f32) -> Self {
        self.delegate = self.delegate.with_child_aspect_ratio(ratio);
        self
    }

    /// A fixed main-axis extent for every tile, on the delegate.
    pub fn with_main_axis_extent(mut self, extent: f32) -> Self {
        self.delegate = self.delegate.with_main_axis_extent(extent);
        self
    }

    /// Pads the grid, as a `SliverPadding` in front of the `SliverGrid` --
    /// padding that scrolls with the content, upstream's
    /// `GridView(padding: ...)`.
    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// How far the content is scrolled. Clamped to the scrollable extent once
    /// the content has been measured.
    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    /// The axis direction to lay the viewport out in -- the door a reversed
    /// grid (`Up`, `Left`) comes in by.
    pub fn with_axis_direction(mut self, axis_direction: AxisDirection) -> Self {
        self.axis_direction = axis_direction;
        self
    }

    /// The band before the leading and after the trailing edge kept warm.
    pub fn with_cache_extent(mut self, cache_extent: f32) -> Self {
        self.cache_extent = cache_extent;
        self
    }

    /// Reports how far this grid can scroll, once it has been laid out. The
    /// cell is the way back out; see [`crate::scrolling::SliverListView`]'s
    /// note on the same trick.
    pub fn with_extent_sink(mut self, sink: Rc<Cell<f32>>) -> Self {
        self.extent_sink = Some(sink);
        self
    }
}

impl crate::framework::Component for GridView {
    fn build(&self, _context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        // The host is mounted as a leaf so that it -- not this description --
        // is what the element tree reconciles against, and the sliver chain
        // under it survives the rebuild.
        let config = self.clone();
        crate::framework::leaf(move || GridHost::new(config.clone()))
    }
}

/// The render half of [`GridView`]: composes the sliver chain once and keeps
/// it, handing the same objects their new configuration every rebuild.
/// Upstream the elements between the widgets and the render objects are what
/// keeps the chain alive across a rebuild; here the host is. The same
/// arrangement as `SliverListHost` in [`crate::scrolling`].
struct GridHost {
    config: GridView,
    /// The grid sliver, and the padding around it when there is one. The
    /// viewport's child is whichever of the two is the outermost.
    sliver: Option<RenderRef>,
    padding_sliver: Option<RenderRef>,
    viewport: Option<render::RenderSliverViewport>,
}

impl GridHost {
    fn new(config: GridView) -> GridHost {
        GridHost {
            config,
            sliver: None,
            padding_sliver: None,
            viewport: None,
        }
    }

    /// A fresh grid sliver describing the current configuration, for
    /// reconfiguring the kept one with.
    fn fresh_sliver(config: &GridView) -> RenderSliverGrid {
        // The `Rc` is cloned into a plain closure because `Rc<dyn Fn>` is not
        // itself an `Fn`, and the grid asks for the latter.
        let build = Rc::clone(&config.build_item);
        RenderSliverGrid::new(config.child_count, config.delegate, move |index| {
            build(index)
        })
    }
}

impl RenderBox for GridHost {
    /// The host's half of the reconciliation: the grid sliver is reconfigured
    /// first (which reconfigures every live child with its freshly built
    /// self, upstream's element rebuild visiting them), then the padding, then
    /// the viewport -- staged around the *same* handles, so the viewport's
    /// same-children test passes and the window below survives the rebuild.
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<GridHost>()?;
        self.config = fresh.config.clone();
        let Some(sliver) = self.sliver.clone() else {
            // Never composed: the first layout builds out of what was just
            // taken.
            return Some(UpdateEffect::Relayout);
        };
        let mut effect = UpdateEffect::Nothing;
        if !sliver.reconfigure(RenderRef::new(Self::fresh_sliver(&self.config))) {
            return None;
        }
        // The padding sliver comes and goes with the configuration; either
        // way the root of the chain is whatever the viewport is handed.
        let root = match (self.padding_sliver.take(), self.config.padding) {
            (Some(padding), Some(insets)) => {
                let staged = render::RenderSliverPadding::new(insets, sliver.clone());
                if !padding.reconfigure(RenderRef::new(staged)) {
                    return None;
                }
                effect = effect.and(UpdateEffect::Relayout);
                padding
            }
            (None, Some(insets)) => {
                // Padding added to a grid that did not have it: a new sliver
                // in front of the grid, which the viewport is restaged with.
                let padding =
                    RenderRef::new(render::RenderSliverPadding::new(insets, sliver.clone()));
                self.padding_sliver = Some(padding.clone());
                effect = effect.and(UpdateEffect::Relayout);
                padding
            }
            (Some(_), None) => {
                // Padding removed: the grid is the root again.
                effect = effect.and(UpdateEffect::Relayout);
                sliver.clone()
            }
            (None, None) => sliver.clone(),
        };
        let mut staged = render::RenderSliverViewport::new(self.config.axis_direction)
            .with_sliver(root)
            .with_offset(self.config.offset)
            .with_cache_extent(self.config.cache_extent)
            .with_user_scroll_direction(self.config.user_scroll_direction);
        self.viewport
            .as_mut()
            .expect("built with the slivers")
            .update_from(&mut staged)
            .map(|viewport_effect| effect.and(viewport_effect))
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if self.viewport.is_none() {
            let sliver = RenderRef::new(Self::fresh_sliver(&self.config));
            let root = if let Some(insets) = self.config.padding {
                let padding =
                    RenderRef::new(render::RenderSliverPadding::new(insets, sliver.clone()));
                self.padding_sliver = Some(padding.clone());
                padding
            } else {
                sliver.clone()
            };
            self.sliver = Some(sliver);
            self.viewport = Some(
                render::RenderSliverViewport::new(self.config.axis_direction)
                    .with_sliver(root)
                    .with_offset(self.config.offset)
                    .with_cache_extent(self.config.cache_extent)
                    .with_user_scroll_direction(self.config.user_scroll_direction),
            );
        }
        let viewport = self.viewport.as_mut().expect("built just above");
        let size = viewport.layout(constraints);
        if let Some(sink) = &self.config.extent_sink {
            sink.set(viewport.max_scroll_extent());
        }
        size
    }

    fn size(&self) -> Size {
        self.viewport.as_ref().map_or(Size::ZERO, |v| v.size())
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if let Some(viewport) = &self.viewport {
            viewport.paint(context, offset);
        }
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        if let Some(viewport) = &self.viewport {
            visit(viewport, Offset::ZERO);
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.viewport
            .as_ref()
            .is_some_and(|v| v.hit_test(position, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_view_builds_only_the_window_it_is_asked_for() {
        use crate::framework::{ElementTree, component};

        let built = Rc::new(Cell::new(0usize));
        let counter = built.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            GridView::count(2, 1000, move |_| {
                counter.set(counter.get() + 1);
                RenderRef::new(render::RenderConstrainedBox::tight(50.0, 50.0))
            })
            .with_offset(0.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        let size = root.layout(BoxConstraints::new(0.0, 300.0, 0.0, 500.0));
        // The viewport is the window it was given, not the grid behind it.
        assert_eq!(size.height, 500.0);
        // Tiles 150 square in a 300-wide, two-column window: the 500-pixel
        // window plus the default 250 of trailing cache is five rows.
        assert_eq!(built.get(), 10, "a thousand tiles were offered");
    }

    #[test]
    fn a_grid_view_reports_how_far_it_can_scroll() {
        use crate::framework::{ElementTree, component};

        let extent = Rc::new(Cell::new(0.0f32));
        let sink = Rc::clone(&extent);
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            GridView::count(2, 20, |_| {
                RenderRef::new(render::RenderConstrainedBox::tight(50.0, 50.0))
            })
            .with_extent_sink(sink),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        root.layout(BoxConstraints::tight(100.0, 200.0));
        // Ten rows of 50 in a 200 window.
        assert_eq!(extent.get(), 300.0);
    }

    #[test]
    fn a_rebuilt_grid_view_keeps_its_render_tree() {
        use crate::framework::{ElementTree, component};

        let counter = Rc::new(Cell::new(0usize));
        let mut tree = ElementTree::new();
        let make = |counter: Rc<Cell<usize>>| {
            component(
                GridView::count(2, 1000, move |_| {
                    counter.set(counter.get() + 1);
                    RenderRef::new(render::RenderConstrainedBox::tight(50.0, 50.0))
                })
                .with_offset(0.0),
            )
        };
        tree.rebuild(make(counter.clone()));
        let mut root = tree.build_render_tree().expect("a mounted root");
        root.layout(BoxConstraints::tight(100.0, 500.0));
        assert!(counter.get() > 0, "the window was built");

        // A rebuild with the same configuration reconciles against the host:
        // the render tree that comes back is the same one, and the sliver
        // chain under it -- viewport, grid sliver, the materialized window --
        // survived with it.
        tree.rebuild(make(counter.clone()));
        let mut again = tree.build_render_tree().expect("still mounted");
        assert!(root.is(&again), "the host was reconfigured, not replaced");
        again.layout(BoxConstraints::tight(100.0, 500.0));
        assert!(counter.get() > 0);
    }
}
