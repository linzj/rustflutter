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
    link: Option<Rc<crate::scrolling::ScrollLink>>,
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
            link: None,
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
    pub fn with_link(mut self, link: Rc<crate::scrolling::ScrollLink>) -> Self {
        self.link = Some(link);
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
        if let Some(link) = &self.config.link {
            link.set_measurements(viewport.max_scroll_extent(), size.height);
        }
        size
    }

    fn size(&self) -> Size {
        self.viewport.as_ref().map_or(Size::ZERO, |v| v.size())
    }

    /// A viewport is as big as it is offered, and this host is the viewport it
    /// builds -- so the answer is the same whether or not that viewport exists
    /// yet, and no scrolling has to be worked out to give it.
    ///
    /// It matters that the answer does not depend on the viewport being built,
    /// because a dry measurement happens *before* the first layout, which is
    /// where the building happens. See PORTING_STATUS.md, tick 471.
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
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

// -- Tiles --------------------------------------------------------------------

/// Upstream `GridTile` (`material/grid_tile.dart`): one cell of a grid, with
/// something optionally banded across its top or its bottom.
///
/// The whole class is a [`crate::render::RenderStack`] with the child filling
/// it and the header and footer pinned to three edges each. What is worth
/// keeping from it is the early return: **with neither a header nor a footer
/// the tile is the child itself**, not a stack of one. A photo grid builds one
/// of these per cell, and a stack that exists to hold nothing is a layout pass
/// and a paint layer per cell for no drawn difference.
pub struct GridTile {
    child: std::cell::RefCell<Option<crate::framework::AnyWidget>>,
    header: std::cell::RefCell<Option<crate::framework::AnyWidget>>,
    footer: std::cell::RefCell<Option<crate::framework::AnyWidget>>,
}

impl GridTile {
    pub fn new(child: crate::framework::AnyWidget) -> GridTile {
        GridTile {
            child: std::cell::RefCell::new(Some(child)),
            header: std::cell::RefCell::new(None),
            footer: std::cell::RefCell::new(None),
        }
    }

    /// Banded across the top, typically a [`GridTileBar`].
    pub fn with_header(self, header: crate::framework::AnyWidget) -> Self {
        *self.header.borrow_mut() = Some(header);
        self
    }

    /// Banded across the bottom.
    pub fn with_footer(self, footer: crate::framework::AnyWidget) -> Self {
        *self.footer.borrow_mut() = Some(footer);
        self
    }
}

impl crate::framework::Component for GridTile {
    fn build(&self, _context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        let header = self.header.borrow_mut().take();
        let footer = self.footer.borrow_mut().take();
        if header.is_none() && footer.is_none() {
            // Upstream's early return, and see the type docs: a stack that
            // holds one child is a layout pass per cell for nothing.
            return child;
        }

        // The order matters and it is upstream's: the child first, so the
        // bands paint over it. A band is meant to sit on top of the photo.
        let has_header = header.is_some();
        let mut children = vec![child];
        children.extend(header);
        children.extend(footer);
        crate::framework::many(children, move |mut boxed| {
            let mut stack = crate::render::RenderStack::new();
            let mut boxed = boxed.drain(..);
            // `Positioned.fill`: the child is exactly the tile, which is what
            // makes the tile's size the grid delegate's business rather than
            // the photo's.
            stack = stack.push_positioned_boxed(
                boxed.next().expect("the child is always pushed"),
                crate::render::StackPosition::fill(),
            );
            if has_header {
                stack = stack.push_positioned_boxed(
                    boxed.next().expect("the header was counted"),
                    crate::render::StackPosition {
                        top: Some(0.0),
                        left: Some(0.0),
                        right: Some(0.0),
                        ..Default::default()
                    },
                );
            }
            if let Some(footer) = boxed.next() {
                stack = stack.push_positioned_boxed(
                    footer,
                    crate::render::StackPosition {
                        bottom: Some(0.0),
                        left: Some(0.0),
                        right: Some(0.0),
                        ..Default::default()
                    },
                );
            }
            stack
        })
    }
}

/// Upstream `GridTileBar` (`material/grid_tile_bar.dart`): the band a
/// [`GridTile`] puts across its top or bottom.
///
/// Two things about it are worth stating, because both look arbitrary:
///
/// * **The bar is always dark.** Upstream wraps its content in
///   `Theme(data: ThemeData.dark())` and an `IconTheme` of white, whatever
///   the ambient theme is. The reason is what a bar sits on: a photograph,
///   whose colours nobody chose. Dark text over an unknown image is
///   unreadable in a way white text over one is not, so the bar does not ask
///   the theme.
/// * **The end padding depends on what is at that end.** 16 with nothing
///   there, 8 with a leading or trailing widget -- because an icon carries its
///   own visual padding inside its box and the full 16 next to it reads as a
///   gap.
pub struct GridTileBar {
    title: Option<String>,
    subtitle: Option<String>,
    leading: std::cell::RefCell<Option<crate::framework::AnyWidget>>,
    trailing: std::cell::RefCell<Option<crate::framework::AnyWidget>>,
    background_color: Option<crate::engine::Color>,
}

impl GridTileBar {
    /// A bar with a title or a subtitle, but not both.
    ///
    /// Upstream writes the two heights inline and names neither: a bar
    /// carrying both is 68 tall, anything else 48.
    pub const ONE_LINE_HEIGHT: f32 = 48.0;
    pub const TWO_LINE_HEIGHT: f32 = 68.0;

    pub fn new() -> GridTileBar {
        GridTileBar {
            title: None,
            subtitle: None,
            leading: std::cell::RefCell::new(None),
            trailing: std::cell::RefCell::new(None),
            background_color: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn with_leading(self, leading: crate::framework::AnyWidget) -> Self {
        *self.leading.borrow_mut() = Some(leading);
        self
    }

    pub fn with_trailing(self, trailing: crate::framework::AnyWidget) -> Self {
        *self.trailing.borrow_mut() = Some(trailing);
        self
    }

    /// Upstream's `backgroundColor`, left unset by default: a bar over a
    /// photograph is usually meant to be transparent, with the text alone
    /// carrying it.
    pub fn with_background_color(mut self, color: crate::engine::Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Upstream's height expression. Both lines of text need the taller bar;
    /// one line of either needs the shorter one, and so does a bar with no
    /// text at all.
    pub fn height(&self) -> f32 {
        if self.title.is_some() && self.subtitle.is_some() {
            GridTileBar::TWO_LINE_HEIGHT
        } else {
            GridTileBar::ONE_LINE_HEIGHT
        }
    }

    /// Upstream's `EdgeInsetsDirectional.only(start:, end:)`: 8 next to a
    /// leading or trailing widget, 16 next to nothing.
    pub fn padding(&self) -> crate::render::EdgeInsets {
        crate::render::EdgeInsets {
            left: if self.leading.borrow().is_some() {
                8.0
            } else {
                16.0
            },
            right: if self.trailing.borrow().is_some() {
                8.0
            } else {
                16.0
            },
            top: 0.0,
            bottom: 0.0,
        }
    }
}

impl Default for GridTileBar {
    fn default() -> GridTileBar {
        GridTileBar::new()
    }
}

impl crate::framework::Component for GridTileBar {
    fn build(&self, _context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        // Upstream's `ThemeData.dark()`, not the ambient theme -- see the type
        // docs. The two styles are that theme's `titleMedium` and `bodySmall`.
        let dark = crate::components::Theme::dark();
        let title_style = dark.title();
        let subtitle_style = dark.muted();
        let height = self.height();
        let padding = self.padding();
        let background = self.background_color;
        let title = self.title.clone();
        let subtitle = self.subtitle.clone();

        let leading = self.leading.borrow_mut().take();
        let trailing = self.trailing.borrow_mut().take();
        let has_leading = leading.is_some();
        let mut children = Vec::new();
        children.extend(leading);
        children.extend(trailing);

        crate::framework::many(children, move |mut boxed| {
            let mut boxed = boxed.drain(..);
            let leading = if has_leading { boxed.next() } else { None };
            let trailing = boxed.next();

            // One line each, elided: a bar is a fixed height, so a title that
            // wrapped would only be clipped. Upstream says so with
            // `DefaultTextStyle(softWrap: false, overflow: ellipsis)`.
            let one_line = |text: &str, style: &crate::engine::TextStyle| {
                crate::widgets::Text::new(text.to_string())
                    .with_style(style.clone())
                    .with_soft_wrap(false)
                    .with_overflow(crate::render::TextOverflow::Ellipsis)
                    .with_max_lines(1)
            };

            let mut row = crate::widgets::Row::new();
            if let Some(leading) = leading {
                row = row.push(
                    crate::widgets::Container::new()
                        .with_margin(crate::render::EdgeInsets {
                            left: 0.0,
                            right: 8.0,
                            top: 0.0,
                            bottom: 0.0,
                        })
                        .with_child(leading),
                );
            }
            // Upstream stacks the two lines in a `Column` when both are there
            // and shows whichever one is there otherwise -- in the *title's*
            // style either way, so a bar carrying only a subtitle still reads
            // as a title.
            let text: Option<crate::render::RenderFlex> = match (&title, &subtitle) {
                (Some(title), Some(subtitle)) => Some(
                    crate::widgets::Column::new()
                        .with_main_axis_size(crate::render::MainAxisSize::Min)
                        .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Start)
                        .push(one_line(title, &title_style))
                        .push(one_line(subtitle, &subtitle_style)),
                ),
                (Some(text), None) | (None, Some(text)) => Some(
                    crate::widgets::Column::new()
                        .with_main_axis_size(crate::render::MainAxisSize::Min)
                        .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Start)
                        .push(one_line(text, &title_style)),
                ),
                (None, None) => None,
            };
            if let Some(text) = text {
                row = row.push_flex(crate::widgets::Expanded::new(text));
            }
            if let Some(trailing) = trailing {
                row = row.push(
                    crate::widgets::Container::new()
                        .with_margin(crate::render::EdgeInsets {
                            left: 8.0,
                            right: 0.0,
                            top: 0.0,
                            bottom: 0.0,
                        })
                        .with_child(trailing),
                );
            }

            let mut container = crate::widgets::Container::new()
                .with_height(height)
                .with_padding(padding)
                .with_child(row);
            if let Some(background) = background {
                container = container.with_color(background);
            }
            container
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_measured_before_it_is_built_takes_the_room_it_would_take() {
        // A dry measurement happens *before* the first layout, and the first
        // layout is where the viewport gets built -- so the answer had to be
        // one that does not need it. It is: a viewport is as big as it is
        // offered. The default was `Size::ZERO`, which is what a grid measured
        // inside a flex reported before this.
        let host = GridHost::new(GridView::count(2, 10, |_| {
            RenderRef::new(render::RenderConstrainedBox::tight(50.0, 50.0))
        }));
        let room = render::BoxConstraints::loose(300.0, 400.0);
        assert!(host.viewport.is_none(), "nothing built yet");
        assert_eq!(
            render::RenderBox::compute_dry_layout(&host, room),
            render::Size::new(300.0, 400.0)
        );

        // And once it has been laid out, the wet answer is the same one.
        let mut laid_out = GridHost::new(GridView::count(2, 10, |_| {
            RenderRef::new(render::RenderConstrainedBox::tight(50.0, 50.0))
        }));
        assert_eq!(
            render::RenderBox::layout(&mut laid_out, room),
            render::RenderBox::compute_dry_layout(&host, room)
        );
    }

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

        let link = Rc::new(crate::scrolling::ScrollLink::default());
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            GridView::count(2, 20, |_| {
                RenderRef::new(render::RenderConstrainedBox::tight(50.0, 50.0))
            })
            .with_link(Rc::clone(&link)),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        root.layout(BoxConstraints::tight(100.0, 200.0));
        // Ten rows of 50 in a 200 window.
        assert_eq!(link.extent(), 300.0);
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

    /// Lays a widget out under a theme, the way the drawer's tests do.
    fn tile_size(widget: crate::framework::AnyWidget, width: f32, height: f32) -> Size {
        use crate::framework::{ElementTree, provide};
        let mut tree = ElementTree::new();
        tree.rebuild(provide(crate::components::Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height))
    }

    fn coloured(width: f32, height: f32) -> crate::framework::AnyWidget {
        crate::framework::leaf(move || render::RenderConstrainedBox::tight(width, height))
    }

    #[test]
    fn a_tile_with_no_bands_is_the_child_itself() {
        // Upstream's early return, and it is not a micro-optimisation: a photo
        // grid builds one of these per cell, and a stack holding one child is
        // a layout pass and a paint layer per cell for no drawn difference.
        use crate::framework::component;
        let size = tile_size(component(GridTile::new(coloured(80.0, 40.0))), 300.0, 300.0);
        assert_eq!(size, Size::new(80.0, 40.0));
    }

    #[test]
    fn a_tile_with_a_band_is_a_stack_the_child_fills() {
        use crate::framework::component;
        // The child is `Positioned.fill`, so it is the tile's size rather than
        // its own -- and the stack, having only positioned children, takes
        // everything it is offered.
        let size = tile_size(
            component(GridTile::new(coloured(80.0, 40.0)).with_header(coloured(10.0, 20.0))),
            300.0,
            300.0,
        );
        assert_eq!(size, Size::new(300.0, 300.0));
    }

    #[test]
    fn the_header_sits_at_the_top_and_the_footer_at_the_bottom() {
        use crate::framework::{ElementTree, component, provide};
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            component(
                GridTile::new(coloured(10.0, 10.0))
                    .with_header(coloured(10.0, 20.0))
                    .with_footer(coloured(10.0, 30.0)),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::tight(200.0, 100.0));
        let mut offsets = Vec::new();
        root.visit_children(&mut |_, offset| offsets.push(offset));
        // The child first -- so the bands paint over it, which is the point of
        // a band -- then the header at the top and the footer resting on the
        // bottom edge.
        assert_eq!(offsets[0], Offset::ZERO, "the child fills the tile");
        assert_eq!(offsets[1], Offset::ZERO, "the header is at the top");
        assert_eq!(offsets[2], Offset::new(0.0, 70.0), "100 less its 30");
    }

    #[test]
    fn a_bar_is_taller_only_when_it_has_both_lines() {
        // One line of either is the short bar, and so is a bar with no text at
        // all -- upstream's condition is `title != null && subtitle != null`,
        // not "has any text".
        assert_eq!(GridTileBar::new().height(), 48.0);
        assert_eq!(GridTileBar::new().with_title("A").height(), 48.0);
        assert_eq!(GridTileBar::new().with_subtitle("B").height(), 48.0);
        assert_eq!(
            GridTileBar::new()
                .with_title("A")
                .with_subtitle("B")
                .height(),
            68.0
        );
    }

    #[test]
    fn the_padding_shrinks_at_whichever_end_has_a_widget() {
        // An icon carries its own visual padding inside its box, so the full
        // 16 next to it reads as a gap. The two ends are decided separately.
        let plain = GridTileBar::new().padding();
        assert_eq!((plain.left, plain.right), (16.0, 16.0));

        let led = GridTileBar::new()
            .with_leading(coloured(24.0, 24.0))
            .padding();
        assert_eq!((led.left, led.right), (8.0, 16.0));

        let trailed = GridTileBar::new()
            .with_trailing(coloured(24.0, 24.0))
            .padding();
        assert_eq!((trailed.left, trailed.right), (16.0, 8.0));
    }

    #[test]
    fn a_bar_is_its_own_height_whatever_it_is_offered() {
        use crate::framework::component;
        // A band across a tile is a fixed height; the tile decides the width.
        let size = tile_size(
            component(
                GridTileBar::new()
                    .with_title("Sunset")
                    .with_subtitle("2019"),
            ),
            300.0,
            500.0,
        );
        assert_eq!(size.height, 68.0);
    }
}
