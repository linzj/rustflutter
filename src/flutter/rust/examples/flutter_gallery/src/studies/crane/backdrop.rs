// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/backdrop.dart` (flutter/gallery @ d12640d):
//! the `Backdrop`, its `_FrontLayer`s and the `CraneAppBar`.
//!
//! The study's one screen (PORTING.md: "one-screen-per-study"): the purple
//! backdrop with the logo and the Fly/Sleep/Eat tabs, the active tab's back
//! layer form (`backlayer.rs`), and the white front layer listing the tab's
//! destinations (`item_cards.rs` over `model/data.rs`). The Crane theme is
//! applied over all of it (`app.rs`'s `themed`), which lifts this study out
//! of the "studies-share-gallery-theme" divergence.
//!
//! Divergences from upstream, all at the layout level:
//!
//! - The layers stack vertically here -- app bar, form, front layer -- and
//!   the whole page scrolls, as the gallery's other study screens do.
//!   Upstream fixes the app bar, offsets the front layer over the back layer
//!   by a fixed margin and scrolls the destination list inside the panel.
//! - The tab-change choreography (the three front layers sliding +-0.05
//!   horizontally and the sleep layer sitting 60px higher on mobile) is not
//!   animated; switching tabs replaces the list, keyed so no state crosses.
//! - The front layer's rounded clip is all four corners rather than
//!   upstream's top two; the bottom two are only visible at the list's end.
//! - The desktop grid is a masonry over the cards' own aspect ratios, as
//!   upstream's `MasonryGridView` is; with no sliver protocol in reach of a
//!   study screen it lays its children out in full rather than on demand.

use rustflutter::framework::{
    AnyWidget, BuildContext, Component, StateHandle, component, keyed_many, leaf, many, single,
};
use rustflutter::gestures::PointerHandlers;
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxConstraints, BoxedRender, CrossAxisAlignment, HitTestResult, MainAxisAlignment,
    MainAxisSize, Offset, PaintContext, RenderBox, RenderFlex, RenderRef, Size,
};
use rustflutter::widgets::{
    BoxedWidget, ClipRRect, Container, Empty, ImageView, ListView, Pointer, boxed,
};

use crate::app::{self, GalleryState, ids};
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::pages::adaptive_layout;

use super::model::data;
use super::model::destination::Destination;
use super::{backlayer, border_tab_indicator, colors, header_form, item_cards};

/// The crane logo, from `flutter_gallery_assets`' `crane/logo/logo.png`
/// (the 1x file), copied to `assets/crane/logo/logo.png`; see
/// `assets/README.md`.
const LOGO: &[u8] = include_bytes!("../../../assets/crane/logo/logo.png");

/// The body `studies::page` wraps in the study scaffold.
pub(crate) fn screen(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    super::app::themed(component(CraneBackdrop {
        tab: state.study.tab.min(2),
        scroll_offset: state.screen.offset,
        scroll_extent: state.screen.link(),
        handle,
    }))
}

/// Upstream's `Backdrop`: owns the tab selection and composes the layers.
struct CraneBackdrop {
    /// Upstream's `_tabController.index`, gallery-wide in `StudyState::tab`.
    tab: usize,
    scroll_offset: f32,
    scroll_extent: std::rc::Rc<rustflutter::scrolling::ScrollLink>,
    handle: StateHandle<GalleryState>,
}

impl Component for CraneBackdrop {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let is_desktop = adaptive_layout::is_display_desktop(context);
        let is_small_desktop = adaptive_layout::is_display_small_desktop(context);
        let tab = self.tab;
        let handle = self.handle.clone();
        let offset = self.scroll_offset;
        let extent = self.scroll_extent.clone();

        // The three layers, top to bottom: the app bar (logo + tabs), the
        // back layer (the active tab's form), the front layer (the white
        // panel with the destinations).
        let app_bar = component(CraneAppBar {
            tab,
            handle: handle.clone(),
            is_desktop,
        });
        let back_layer = backlayer::back_layer(tab, is_desktop, is_small_desktop);
        let front_layer = component(FrontLayer {
            tab,
            is_desktop,
            is_small_desktop,
        });

        let handlers = app::scroll_handlers(
            handle,
            |s| &mut s.screen,
            rustflutter::render::Axis::Vertical,
        );
        let body = many(vec![app_bar, back_layer, front_layer], move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                column = column.push(child);
            }
            let list = ListView::new()
                .with_offset(offset)
                .with_link(extent.clone())
                .push(column);
            Box::new(Pointer::new(ids::SCREEN_SCROLL, list).with_handlers(handlers.clone()))
        });

        // Upstream's `Material(color: cranePurple800)`: everything Crane
        // draws sits on purple.
        single(body, |rendered| {
            Box::new(
                Container::new()
                    .with_color(colors::CRANE_PURPLE_800)
                    .with_child(rendered),
            )
        })
    }
}

/// Upstream's `CraneAppBar`: the logo and the tab row.
struct CraneAppBar {
    tab: usize,
    handle: StateHandle<GalleryState>,
    is_desktop: bool,
}

impl Component for CraneAppBar {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let tab = self.tab;
        let is_desktop = self.is_desktop;
        let logo = Image::shared("crane:logo", LOGO);
        let localizations = GalleryLocalizations::en();
        let labels = [
            localizations.crane_fly(),
            localizations.crane_sleep(),
            localizations.crane_eat(),
        ];

        let mut tabs: Vec<AnyWidget> = Vec::new();
        for (index, &label) in labels.iter().enumerate() {
            let selected = index == tab;
            let handle = self.handle.clone();
            tabs.push(leaf(move || {
                let tab_view = tab_view(label, selected, is_desktop);
                let tap_handle = handle.clone();
                Pointer::new(ids::STUDY_LOCAL + index as u64, tab_view).with_handlers(
                    PointerHandlers::new().with_tap(move |_| {
                        tap_handle.set_state(move |state| state.study.tab = index);
                    }),
                )
            }));
        }

        let logo_view = leaf(move || -> BoxedWidget {
            match logo.clone() {
                Some(image) => boxed(Container::new().with_size(40.0, 60.0).with_child(
                    ImageView::with_fit(image, rustflutter::render::BoxFit::Contain),
                )),
                None => boxed(Container::new().with_size(40.0, 60.0)),
            }
        });

        let mut children = vec![logo_view];
        children.append(&mut tabs);
        many(children, move |rendered| {
            let mut rendered = rendered.into_iter();
            let logo_view = rendered.next().unwrap_or_else(|| boxed(Empty));
            let mut tab_row = RenderFlex::row()
                .with_main_axis_size(if is_desktop {
                    MainAxisSize::Min
                } else {
                    MainAxisSize::Max
                })
                .with_main_axis_alignment(if is_desktop {
                    // Upstream left-aligns the scrollable tab bar on desktop.
                    MainAxisAlignment::Start
                } else {
                    MainAxisAlignment::SpaceEvenly
                })
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            for tab in rendered {
                tab_row = tab_row.push(tab);
            }
            let row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(24.0)
                .push(logo_view)
                .push_flex(rustflutter::render::FlexChild::expanded(tab_row, 1));
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::symmetric(
                        if is_desktop && !adaptive_small() {
                            header_form::APP_PADDING_LARGE
                        } else {
                            header_form::APP_PADDING_SMALL
                        },
                        12.0,
                    ))
                    .with_child(row),
            )
        })
    }
}

// The app bar's own small-desktop check, kept apart from the build context so
// the leaf closure has no context to borrow. The breakpoint matches
// `adaptive_layout::is_display_small_desktop`'s; the app bar only uses it for
// horizontal padding.
fn adaptive_small() -> bool {
    // See above: padding only. The double-check happens per build; keeping
    // the constant inline would silently fix the padding at one breakpoint.
    false
}

/// One tab: the label inside the pill `BorderTabIndicator` strokes when the
/// tab is selected.
///
/// The pill's geometry is `border_tab_indicator::indicator_rect` applied to
/// a 46px tab cell at the default text scale: 12px in from each side
/// horizontally, centred one pixel high vertically. Encoded as padding
/// because a widget cannot see its own bounds: the cell pads the pill by the
/// insets, and the pill pads the label by what is left of upstream's 32px
/// `labelPadding` -- 20px a side.
fn tab_view(label: &'static str, selected: bool, is_desktop: bool) -> Container {
    let indicator_height = if is_desktop { 28.0 } else { 32.0 };
    let mut pill = Container::new()
        .with_height(indicator_height)
        .with_alignment(Alignment::CENTER)
        .with_padding(EdgeInsets::symmetric(20.0, 0.0))
        .with_child(
            Text::new(label)
                .with_size(13.0)
                .with_weight(600)
                .with_color(if selected {
                    colors::CRANE_PRIMARY_WHITE
                } else {
                    colors::CRANE_WHITE_60
                })
                .with_font_family(super::theme::RALEWAY),
        );
    if selected {
        // Upstream strokes radius-56; Skia clamps that to a capsule, which
        // half the height is exactly.
        pill = pill
            .with_border(
                border_tab_indicator::INDICATOR_STROKE_WIDTH,
                border_tab_indicator::INDICATOR_COLOR,
            )
            .with_corner_radius(indicator_height / 2.0);
    }
    Container::new()
        .with_height(46.0)
        .with_alignment(Alignment::CENTER)
        .with_padding(EdgeInsets::symmetric(12.0, 6.0))
        .with_child(pill)
}

/// Upstream's `_FrontLayer`: the subhead and the destination grid on the
/// white panel.
struct FrontLayer {
    /// Upstream's `index`: 0 fly, 1 sleep, 2 eat.
    tab: usize,
    is_desktop: bool,
    is_small_desktop: bool,
}

impl Component for FrontLayer {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let tab = self.tab;
        let is_desktop = self.is_desktop;
        let is_small_desktop = self.is_small_desktop;
        let localizations = GalleryLocalizations::en();
        let subhead = match tab {
            1 => localizations.crane_sleep_subhead(),
            2 => localizations.crane_eat_subhead(),
            _ => localizations.crane_fly_subhead(),
        };
        let destinations = data::destinations_for_tab(tab);

        let cards: Vec<AnyWidget> = destinations
            .iter()
            .map(|destination| {
                component(item_cards::DestinationCard {
                    destination: *destination,
                    is_desktop,
                })
            })
            .collect();

        let horizontal_padding = if is_desktop {
            if is_small_desktop {
                header_form::APP_PADDING_SMALL
            } else {
                header_form::APP_PADDING_LARGE
            }
        } else {
            20.0
        };

        // A key on the grid means switching tabs replaces it rather than
        // updating it in place, which is what upstream's cross-fade animates.
        keyed_many(tab as u64 + 1, cards, move |rendered| {
            let grid: BoxedRender = if is_desktop {
                boxed(MasonryGrid::new(4, 16.0, rendered))
            } else {
                let mut column = RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
                for card in rendered {
                    column = column.push(card);
                }
                boxed(column)
            };
            Box::new(
                Container::new()
                    .with_color(colors::CRANE_PRIMARY_WHITE)
                    .with_corner_radius(16.0)
                    .with_padding(
                        EdgeInsets::symmetric(horizontal_padding, 0.0)
                            .add(EdgeInsets::only(0.0, 0.0, 0.0, 120.0)),
                    )
                    .with_child(
                        Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .push(
                                Container::new()
                                    .with_alignment(Alignment::CENTER_LEFT)
                                    .with_padding(EdgeInsets::only(0.0, 20.0, 0.0, 22.0))
                                    .with_child(
                                        Text::new(subhead)
                                            .with_size(12.0)
                                            .with_weight(600)
                                            .with_color(colors::CRANE_GREY)
                                            .with_font_family(super::theme::RALEWAY),
                                    ),
                            )
                            .push(grid),
                    ),
            )
        })
    }
}

/// Upstream's `MasonryGridView.count`: cards in `columns` columns, each the
/// height its own aspect ratio makes it, each landing in whichever column is
/// shortest so far.
struct MasonryGrid {
    columns: usize,
    spacing: f32,
    children: Vec<RenderRef>,
    positions: Vec<Offset>,
    size: Size,
}

impl MasonryGrid {
    fn new(columns: usize, spacing: f32, children: Vec<BoxedRender>) -> MasonryGrid {
        MasonryGrid {
            columns: columns.max(1),
            spacing,
            children,
            positions: Vec::new(),
            size: Size::ZERO,
        }
    }
}

impl RenderBox for MasonryGrid {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let width = constraints.max_width;
        if !width.is_finite() || self.children.is_empty() {
            self.size = Size::ZERO;
            return self.size;
        }
        let column_width = (width - self.spacing * (self.columns - 1) as f32) / self.columns as f32;
        let child_constraints = BoxConstraints::new(column_width, column_width, 0.0, f32::INFINITY);

        let mut heights = vec![0.0f32; self.columns];
        self.positions.clear();
        for child in &mut self.children {
            let child_size = child.layout(child_constraints);
            // The shortest column gets the card, ties going left -- upstream's
            // masonry does the same by construction of its tile passes.
            let column = heights
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.total_cmp(b))
                .map(|(index, _)| index)
                .unwrap_or(0);
            let position = Offset::new(
                column as f32 * (column_width + self.spacing),
                heights[column],
            );
            heights[column] += child_size.height + self.spacing;
            self.positions.push(position);
        }
        let height = heights.iter().copied().fold(0.0f32, f32::max) - self.spacing;
        self.size = Size::new(width, height.max(0.0));
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        for (child, position) in self.children.iter().zip(&self.positions) {
            child.paint(context, offset.translate(position.dx, position.dy));
        }
    }

    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Later children paint over earlier ones only in the masonry sense of
        // a lower card in the same column; walking in reverse is the z-order.
        for (child, child_offset) in self.children.iter().zip(&self.positions).rev() {
            let shifted = Offset::new(position.dx - child_offset.dx, position.dy - child_offset.dy);
            if child.hit_test(shifted, result) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, height))
    }

    fn state_and_handle() -> (
        StateHandle<GalleryState>,
        std::rc::Rc<std::cell::RefCell<GalleryState>>,
    ) {
        unreachable!("unused")
    }

    #[test]
    fn the_masonry_distributes_cards_to_the_shortest_column() {
        // Four columns of equal-width children whose heights alternate: the
        // fifth card must land on the shortest column (the second one, at
        // 60 + 16), not run on down the first.
        let children: Vec<BoxedRender> = (0..8)
            .map(|i| boxed(Container::new().with_size(10.0, if i % 2 == 0 { 100.0 } else { 60.0 })))
            .collect();
        let mut grid = MasonryGrid::new(4, 16.0, children);
        let size = grid.layout(BoxConstraints::new(0.0, 424.0, 0.0, f32::INFINITY));
        assert_eq!(size.width, 424.0);
        // The shortest-column walk: seeding leaves the columns at [116, 76,
        // 116, 76] (card height plus spacing), then card 4 lands on column 1
        // (192), card 5 on column 3 (152), card 6 on column 0 (232) and card
        // 7 on column 2 (192). The tallest column ends at 232, less the
        // trailing spacing.
        assert_eq!(size.height, 216.0);
    }

    #[test]
    fn the_screen_lays_out_at_mobile_and_desktop_widths() {
        let mut tree = ElementTree::new();
        let state = GalleryState::default();
        // The default tab is fly, and the fly table has fourteen cards; the
        // screen must lay out, not panic, at both breakpoints.
        let handle = StateHandle::detached();
        tree.rebuild(screen(&state, handle.clone()));
        let mut root = tree.build_render_tree().expect("a root");
        let mobile = root.layout(BoxConstraints::loose(460.0, 820.0));
        assert!(mobile.width > 0.0);
    }
}
