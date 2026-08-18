// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Navigation drawers, ported from `material/drawer.dart`.
//!
//! Upstream a drawer is driven by a `DrawerController`: an animation
//! controller slides it in, edge drags and flings steer the animation, and a
//! local history entry wires the back button to close it. Of that machinery,
//! what survives a framework with no routes and a desktop host is the shape:
//! the [`Drawer`] panel itself, and the [`crate::components::Scaffold`] slot
//! that stacks it over the page behind a scrim. Opening and closing are the
//! application's state, the same way every overlay here is (see the module
//! docs of [`crate::controls`]).
//!
//! What is deliberately not ported, each with upstream's own reason for it:
//!
//! - **The edge-drag gesture** (`_kEdgeDragWidth` 20.0, `_move`/`_settle` and
//!   the fling settle). Upstream only installs it on non-desktop platforms --
//!   `_buildDrawer` answers a `SizedBox.shrink` for a closed drawer when
//!   `isDesktop` -- and this crate's hosts are desktop.
//! - **The slide animation** (`_kBaseSettleDuration` 246ms of
//!   `AnimationController`): it is the controller's `value` that positions the
//!   drawer and fades the scrim, and there is no controller without a
//!   route-like owner for it. The drawer is simply present or absent.
//! - **The local history entry** (`_ensureHistoryEntry`): it exists to make an
//!   Android back button pop the drawer; there is no Navigator to hold it.

use std::cell::RefCell;

use crate::components::theme_of;
use crate::direction::TextDirection;
use crate::framework::{AnyWidget, BuildContext, Component, leaf, single};
use crate::render::{BoxConstraints, RenderConstrainedBox, Size};
use crate::widgets::Container;

/// The possible alignments of a [`Drawer`]. Upstream's `DrawerAlignment`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawerAlignment {
    /// At the start side: left in a left-to-right subtree, right in a
    /// right-to-left one.
    #[default]
    Start,
    /// At the end side.
    End,
}

/// Resolves an alignment to a physical side. Upstream computes this twice --
/// `_directionFactor` for the drag math and the `_drawerOuterAlignment` switch
/// for placement -- and both reduce to this when the drag is not ported.
pub(crate) fn drawer_on_left(alignment: DrawerAlignment, direction: TextDirection) -> bool {
    match (direction, alignment) {
        (TextDirection::Ltr, DrawerAlignment::Start) => true,
        (TextDirection::Ltr, DrawerAlignment::End) => false,
        (TextDirection::Rtl, DrawerAlignment::Start) => false,
        (TextDirection::Rtl, DrawerAlignment::End) => true,
    }
}

/// The Material spec's default drawer width. Upstream's `_kWidth`.
pub const DRAWER_WIDTH: f32 = 304.0;

/// A panel that slides in from the edge of a scaffold to show navigation
/// links. Upstream's `Drawer`.
///
/// The child is typically a column of [`crate::components::ListTile`]s, which
/// is what the gallery puts in one. The panel is as tall as it is allowed to
/// be and `width` across.
pub struct Drawer {
    child: RefCell<Option<AnyWidget>>,
    width: f32,
    elevation: u32,
}

impl Drawer {
    pub fn new(child: AnyWidget) -> Drawer {
        Drawer {
            child: RefCell::new(Some(child)),
            width: DRAWER_WIDTH,
            // `_DrawerDefaultsM3.elevation`.
            elevation: 1,
        }
    }

    /// Upstream's `Drawer.width`, falling back to `_kWidth` when null -- which
    /// here is what the constructor already set.
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Upstream's `Drawer.elevation`.
    pub fn with_elevation(mut self, elevation: u32) -> Self {
        self.elevation = elevation;
        self
    }
}

impl Component for Drawer {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| crate::widgets::Empty));
        let width = self.width;
        let elevation = self.elevation;
        // `_DrawerDefaultsM3.backgroundColor` is
        // `colorScheme.surfaceContainerLow`; the crate's Theme has no
        // container ramp, and `surface` is its lowest elevated surface.
        let surface = theme.surface;

        single(child, move |inner| {
            // `Drawer.build`: ConstrainedBox(BoxConstraints.expand(width: w))
            // around the Material -- tight width, as tall as offered.
            //
            // The M3 shape rounds the drawer's outer corners by 16
            // (`_DrawerDefaultsM3.shape`/`endShape`). The renderer has one
            // radius for all four corners -- the same limitation
            // `BottomSheet` documents -- and the two corners pinned to the
            // screen edge round off-screen, so a uniform 16 draws the same
            // shape the spec asks for.
            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_corner_radius(16.0)
                    .with_elevation(elevation)
                    .with_child(
                        RenderConstrainedBox::new(BoxConstraints::new(
                            width,
                            width,
                            0.0,
                            f32::INFINITY,
                        ))
                        .with_child(inner),
                    ),
            )
        })
    }
}

/// The scrim's color while a drawer is open. Upstream's `Colors.black54`,
/// the `DrawerController` default when neither the widget nor the
/// `DrawerTheme` says otherwise. (This is also what
/// [`crate::controls::Scrim`] paints.)
pub(crate) const DRAWER_SCRIM: crate::engine::Color = crate::engine::Color::argb(0x8A, 0, 0, 0);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Theme;
    use crate::framework::{ElementTree, component, provide};
    use crate::render::RenderBox;
    use crate::widgets::Empty;

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height))
    }

    #[test]
    fn a_drawer_is_304_wide_and_as_tall_as_offered() {
        // A colored container stands in for the drawer's content: it takes
        // everything it is offered, the way the list an app puts here does.
        let drawer = || {
            Drawer::new(leaf(|| {
                Container::new().with_color(crate::engine::Color::WHITE)
            }))
        };
        let size = lay_out(component(drawer()), 800.0, 600.0);
        assert_eq!(size, Size::new(DRAWER_WIDTH, 600.0));
    }

    #[test]
    fn a_drawer_accepts_an_explicit_width() {
        let size = lay_out(
            component(
                Drawer::new(leaf(|| {
                    Container::new().with_color(crate::engine::Color::WHITE)
                }))
                .with_width(240.0),
            ),
            800.0,
            600.0,
        );
        assert_eq!(size.width, 240.0);
    }

    #[test]
    fn the_start_side_follows_the_reading_direction() {
        // Upstream's `(Directionality, alignment)` switch in
        // `_directionFactor`, reduced to a side.
        assert!(drawer_on_left(DrawerAlignment::Start, TextDirection::Ltr));
        assert!(!drawer_on_left(DrawerAlignment::End, TextDirection::Ltr));
        assert!(!drawer_on_left(DrawerAlignment::Start, TextDirection::Rtl));
        assert!(drawer_on_left(DrawerAlignment::End, TextDirection::Rtl));
    }
}
