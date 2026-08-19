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

use crate::direction::TextDirection;
use crate::framework::{AnyWidget, BuildContext, Component, leaf, single};
use crate::render::{BoxConstraints, RenderConstrainedBox};
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
    /// The width this drawer was given outright. `None` means "ask the
    /// theme", which is upstream's null.
    width: Option<f32>,
    elevation: u32,
}

impl Drawer {
    pub fn new(child: AnyWidget) -> Drawer {
        Drawer {
            child: RefCell::new(Some(child)),
            width: None,
            // `_DrawerDefaultsM3.elevation`.
            elevation: 1,
        }
    }

    /// Upstream's `Drawer.width`. Left unset, the width comes from
    /// `DrawerTheme.of(context)` and then from `_kWidth`.
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
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
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| crate::widgets::Empty));
        // Upstream's `Drawer.build`: the width and the background come off
        // `DrawerTheme.of(context)` and then off `_DrawerDefaultsM3`, whose
        // background is `colorScheme.surfaceContainerLow`.
        let drawer = crate::component_themes::ResolvedDrawer::of(context);
        let width = self.width.unwrap_or(drawer.width);
        let elevation = self.elevation;
        let surface = drawer.background;

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

/// The height a [`DrawerHeader`] reserves below the status bar. Upstream's
/// `_kDrawerHeaderHeight`, whose `+ 1.0` is written there as `// bottom edge`:
/// the hairline the header draws under itself is counted in, so the content
/// above it is a round 160 either way.
pub const DRAWER_HEADER_HEIGHT: f32 = 160.0 + 1.0;

/// The panel across the top of a [`Drawer`] -- an account name, an avatar, a
/// product mark. Upstream's `DrawerHeader`.
///
/// Two things it does that are easy to miss, and both are about the status
/// bar:
///
/// * **Its height is the status bar's plus a fixed 161.** A header that were
///   simply 161 tall would have its content pushed down by the notch and lose
///   the bottom of it; a header that ignored the status bar would draw under
///   the clock. Growing by exactly the inset keeps the drawn area constant.
/// * **It adds the same inset to its padding and then removes it from the
///   child.** Upstream's `MediaQuery.removePadding(removeTop: true)`. The
///   header has consumed the top inset by growing, so a child that also
///   inset itself for it would inset twice.
///
/// What is **not** ported: upstream's body is an `AnimatedContainer`, so a
/// decoration that changes walks to its new value over `duration`/`curve`.
/// This crate's implicit-animation helper ([`crate::implicit::animated`]) is
/// over `Lerp`, which requires `Copy`, and a [`crate::decoration::Decoration`]
/// is not one; so the decoration is applied directly and a change snaps. The
/// two fields are left off the API rather than carried and ignored -- a field
/// that does nothing is worse than an absent one, and adding them back when
/// there is an `AnimatedContainer` is the same edit either way.
pub struct DrawerHeader {
    child: RefCell<Option<AnyWidget>>,
    decoration: Option<crate::decoration::Decoration>,
    /// Upstream's default `EdgeInsets.fromLTRB(16, 16, 16, 8)`: the shorter
    /// bottom inset is the hairline's doing, which reads as space of its own.
    padding: crate::render::EdgeInsets,
    /// Upstream's default `EdgeInsets.only(bottom: 8)`, which is the gap
    /// between the header and the first item under it.
    margin: crate::render::EdgeInsets,
}

impl DrawerHeader {
    pub fn new(child: AnyWidget) -> DrawerHeader {
        DrawerHeader {
            child: RefCell::new(Some(child)),
            decoration: None,
            padding: crate::render::EdgeInsets {
                left: 16.0,
                right: 16.0,
                top: 16.0,
                bottom: 8.0,
            },
            margin: crate::render::EdgeInsets {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 8.0,
            },
        }
    }

    pub fn with_decoration(mut self, decoration: crate::decoration::Decoration) -> Self {
        self.decoration = Some(decoration);
        self
    }

    pub fn with_padding(mut self, padding: crate::render::EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    pub fn with_margin(mut self, margin: crate::render::EdgeInsets) -> Self {
        self.margin = margin;
        self
    }

    /// The height a header takes under a status bar of `status_bar_height`.
    pub fn height(status_bar_height: f32) -> f32 {
        status_bar_height + DRAWER_HEADER_HEIGHT
    }
}

impl Component for DrawerHeader {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| crate::widgets::Empty));
        let status_bar_height = crate::media_query::padding_of(context).top;
        // Upstream's `Divider.createBorderSide(context)`, which is the same
        // colour and thickness a `Divider` would draw -- the header's bottom
        // edge is a divider, and saying so is what keeps the two in step when
        // a theme moves one.
        let rule = crate::components::Divider::create_border_side(context);
        let height = DrawerHeader::height(status_bar_height);
        let margin = self.margin;
        let decoration = self.decoration.clone();
        // Upstream's `padding.add(EdgeInsets.only(top: statusBarHeight))`:
        // the inset the header grew by is handed to the padding, so the
        // content sits below the notch rather than under it.
        let padding = crate::render::EdgeInsets {
            top: self.padding.top + status_bar_height,
            ..self.padding
        };

        single(child, move |inner| {
            let mut body = crate::widgets::Container::new()
                .with_padding(padding)
                .with_child(inner);
            if let Some(decoration) = decoration.clone() {
                body = body.with_decoration(decoration);
            }
            Box::new(
                crate::widgets::Container::new()
                    .with_height(height)
                    .with_margin(margin)
                    .with_decoration(crate::decoration::Decoration::Box(
                        crate::decoration::BoxDecoration::new().with_border(
                            crate::borders::BoxBorder::Uniform(crate::borders::Border {
                                top: crate::borders::BorderSide::NONE,
                                right: crate::borders::BorderSide::NONE,
                                bottom: rule,
                                left: crate::borders::BorderSide::NONE,
                            }),
                        ),
                    ))
                    .with_child(body),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Theme;
    use crate::framework::{ElementTree, component, provide};
    use crate::render::RenderBox;
    use crate::render::Size;
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

    #[test]
    fn a_header_grows_by_the_status_bar_rather_than_ignoring_it() {
        // The two failures this rules out: a header of a flat 161 has its
        // content pushed down by the notch and loses the bottom of it, and a
        // header that ignored the inset draws under the clock. Growing by
        // exactly the inset keeps the drawn area constant.
        assert_eq!(DrawerHeader::height(0.0), 161.0);
        assert_eq!(DrawerHeader::height(24.0), 185.0);
        // The 161 is 160 plus the hairline the header draws under itself, so
        // the content above the rule is a round 160 either way.
        assert_eq!(DRAWER_HEADER_HEIGHT - 1.0, 160.0);
    }

    #[test]
    fn a_header_is_as_tall_as_it_says_and_as_wide_as_it_is_offered() {
        let size = lay_out(
            component(DrawerHeader::new(leaf(|| {
                Container::new().with_color(crate::engine::Color::WHITE)
            }))),
            300.0,
            600.0,
        );
        // 161 plus the 8 of bottom margin: a margin is outside the height.
        assert_eq!(size, Size::new(300.0, 169.0));
    }

    #[test]
    fn the_rule_under_a_header_is_the_dividers_own() {
        // Upstream's `Divider.createBorderSide(context)`, and saying so is
        // what keeps the two in step when a theme moves one of them.
        struct Reader(
            std::rc::Rc<
                std::cell::Cell<
                    Option<(
                        crate::borders::BorderSide,
                        crate::component_themes::ResolvedDivider,
                    )>,
                >,
            >,
        );
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                self.0.set(Some((
                    crate::components::Divider::create_border_side(context),
                    crate::component_themes::ResolvedDivider::of(context),
                )));
                leaf(|| Empty)
            }
        }

        let seen = std::rc::Rc::new(std::cell::Cell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), component(Reader(seen.clone()))));
        let (side, divider) = seen.take().expect("the reader built");
        assert_eq!(side.color, divider.color);
        // And the clamp comes with it: a zero thickness is the thinnest line
        // the device can draw, not no line.
        assert_eq!(side.width, divider.line_thickness());
        assert!(side.width >= 1.0);
    }
}
