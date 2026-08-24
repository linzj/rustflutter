// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Navigation drawers, ported from `material/drawer.dart`.
//!
//! Upstream a drawer is driven by a `DrawerController`: an animation
//! controller slides it in, edge drags and flings steer the animation, and a
//! local history entry wires the back button to close it. This module is the
//! panel itself and the geometry -- [`Drawer`], the settle arithmetic, the
//! fling threshold -- and [`crate::drawer_host`] is the controller.
//!
//! It can also still be placed by hand, in a [`crate::components::Scaffold`]
//! slot over a scrim, which is what it was before there was an overlay.
//!
//! What is still not ported, each with upstream's own reason for it:
//!
//! - **The edge-drag gesture** (`_kEdgeDragWidth` 20.0, `_move`/`_settle` and
//!   the fling settle). Upstream only installs it on non-desktop platforms --
//!   `_buildDrawer` answers a `SizedBox.shrink` for a closed drawer when
//!   `isDesktop` -- and this crate's hosts are desktop. The arithmetic is here
//!   and tested; what is missing is the gesture that would drive it.
//! - **The local history entry** (`_ensureHistoryEntry`): it exists to make an
//!   Android back button pop the drawer; there is no Navigator to hold it.
//!
//! The slide animation used to be on that list, with the reason that there is
//! no controller without a route-like owner for it. There is one now: an
//! overlay entry outlives its frames, and a `StatefulComponent` inside one is
//! handed a clock. See [`crate::drawer_host::DrawerAnimation`].

use std::cell::RefCell;

use crate::animation::AnimationStatus;
use crate::routes::LocalHistoryEntry;

use crate::direction::TextDirection;
use crate::editable_text::TargetPlatform;
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
    /// Upstream's `semanticLabel`. `None` does **not** mean the panel is
    /// unnamed -- see [`Drawer::resolved_semantic_label`], where what it means
    /// depends on the platform.
    semantic_label: Option<String>,
}

impl Drawer {
    pub fn new(child: AnyWidget) -> Drawer {
        Drawer {
            child: RefCell::new(Some(child)),
            width: None,
            // `_DrawerDefaultsM3.elevation`.
            elevation: 1,
            semantic_label: None,
        }
    }

    /// Upstream's `Drawer.semanticLabel`.
    pub fn with_semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// What a screen reader is told this panel is, which upstream decides with
    /// a switch on the platform:
    ///
    /// ```dart
    /// TargetPlatform.iOS || TargetPlatform.macOS => semanticLabel,
    /// TargetPlatform.android || ... => semanticLabel ?? drawerLabel,
    /// ```
    ///
    /// # The two Apple platforms get no fallback, and that is the rule
    ///
    /// Everywhere else an unnamed drawer is announced as "Navigation menu".
    /// On iOS and macOS an unnamed drawer is announced as nothing, and a
    /// caller who wants a name has to give one.
    ///
    /// It reads like an omission and is not. VoiceOver already says a modal
    /// panel has appeared and that there is a way out of it; a framework
    /// adding "Navigation menu" on top of that is a second voice saying what
    /// the first one just said. On Android TalkBack does not, so the framework
    /// does.
    ///
    /// A caller's own label wins on every platform -- the difference is only
    /// what happens when there is none.
    pub fn resolved_semantic_label(&self, platform: TargetPlatform) -> Option<String> {
        match platform {
            TargetPlatform::IOS | TargetPlatform::MacOS => self.semantic_label.clone(),
            TargetPlatform::Android
            | TargetPlatform::Fuchsia
            | TargetPlatform::Linux
            | TargetPlatform::Windows => Some(self.semantic_label.clone().unwrap_or_else(|| {
                crate::material_app::DefaultMaterialLocalizations::DRAWER_LABEL.to_string()
            })),
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

// -- The controller, from `material/drawer.dart` -------------------------------

/// Upstream's `_kEdgeDragWidth`: the strip at the screen edge a drag can open
/// the drawer from.
pub const EDGE_DRAG_WIDTH: f32 = 20.0;
/// Upstream's `_kMinFlingVelocity`, in logical pixels per second.
pub const MIN_FLING_VELOCITY: f32 = 365.0;
/// Upstream's `_kBaseSettleDuration`.
pub const BASE_SETTLE_MILLISECONDS: u32 = 246;

/// What a released drag did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SettleOutcome {
    /// The drawer was not showing at all, so there was nothing to settle.
    NothingToSettle,
    /// Fast enough to count, so the direction decides whatever the position is.
    Flung { opening: bool },
    /// Slower than a fling, so the halfway point decides.
    SettledToNearest { opening: bool },
}

/// Upstream's `open` and `close` -- the two commands a caller can give a
/// drawer, as opposed to the drag that [`DrawerController::settle`] judges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawerCommand {
    Open,
    Close,
}

/// What one animation status does to the drawer's entry in the route's local
/// history -- the thing that makes a back press close it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryEntryChange {
    /// Upstream's `_ensureHistoryEntry`, which is idempotent: it adds one only
    /// if there is not one already, because a drawer can be flung forward more
    /// than once without arriving.
    Ensure,
    /// Drop it, and the drawer stops claiming the back button.
    Remove,
    /// What both terminal statuses do.
    Nothing,
}

/// Upstream `DrawerControllerState`, as its decisions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawerController {
    pub alignment: DrawerAlignment,
    /// Whether an edge drag may open it. Upstream also turns this off on
    /// desktop regardless.
    pub enable_open_drag_gesture: bool,
    /// A caller's override for the grab strip's width.
    pub edge_drag_width: Option<f32>,
    /// The animation's 0..1 position: 0 closed, 1 open.
    pub value: f32,
}

impl DrawerController {
    pub fn new(alignment: DrawerAlignment) -> DrawerController {
        DrawerController {
            alignment,
            enable_open_drag_gesture: true,
            edge_drag_width: None,
            value: 0.0,
        }
    }

    pub fn with_value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    /// Which way a positive drag moves the drawer.
    ///
    /// Upstream switches on **both** the reading direction and the alignment:
    ///
    /// ```dart
    /// return switch ((Directionality.of(context), widget.alignment)) {
    ///   (TextDirection.rtl, DrawerAlignment.start) => -1,
    ///   (TextDirection.rtl, DrawerAlignment.end)   => 1,
    ///   (TextDirection.ltr, DrawerAlignment.start) => 1,
    ///   (TextDirection.ltr, DrawerAlignment.end)   => -1,
    /// };
    /// ```
    ///
    /// **This read the alignment alone**, which is right in a left-to-right
    /// reading and inverted in a right-to-left one: a start-aligned drawer
    /// lives on the right there, so the drag that opens it goes the other way.
    /// Every gesture on both drawers was backwards in RTL, and the drawer
    /// would have opened on a swipe meant to close it.
    ///
    /// The two arguments matter equally and only together -- flipping either
    /// one flips the answer, and flipping both leaves it alone, which is what
    /// makes this an exclusive-or rather than two separate rules.
    pub fn direction_factor(&self, text_direction: TextDirection) -> f32 {
        let start_aligned = matches!(self.alignment, DrawerAlignment::Start);
        let left_to_right = matches!(text_direction, TextDirection::Ltr);
        if start_aligned == left_to_right {
            1.0
        } else {
            -1.0
        }
    }

    /// Upstream's `visualVelocity`: pixels per second turned into the
    /// controller's own 0..1 per second by dividing by the drawer's width.
    ///
    /// **The controller does not know about pixels**; converting is the
    /// drawer's job, and it is the drawer's width that sets the exchange rate.
    /// A wider drawer moves through the same animation more slowly for the
    /// same finger speed, which is what makes a wide one feel heavy.
    pub fn visual_velocity(
        &self,
        pixel_velocity: f32,
        width: f32,
        text_direction: TextDirection,
    ) -> f32 {
        pixel_velocity / width * self.direction_factor(text_direction)
    }

    /// Upstream `_settle`.
    ///
    /// Three ways out, and the order is the rule: **speed overrules position,
    /// but only when the speed is a real fling.** A drawer nine tenths open,
    /// flicked shut, closes; released gently at the same place, it opens. Below
    /// the threshold there is nothing to read into the movement, so the halfway
    /// point decides.
    pub fn settle(
        &self,
        pixel_velocity: f32,
        width: f32,
        text_direction: TextDirection,
    ) -> SettleOutcome {
        if self.value == 0.0 {
            // Upstream returns when the controller is dismissed: a drag that
            // never opened anything has nothing to settle.
            return SettleOutcome::NothingToSettle;
        }
        if pixel_velocity.abs() >= MIN_FLING_VELOCITY {
            let visual = self.visual_velocity(pixel_velocity, width, text_direction);
            return SettleOutcome::Flung {
                opening: visual > 0.0,
            };
        }
        SettleOutcome::SettledToNearest {
            opening: self.value >= 0.5,
        }
    }

    /// Upstream's `open` and `close`, as the velocity each flings with.
    ///
    /// `open` calls `fling()` with no argument and `close` calls
    /// `fling(velocity: -1.0)`, so the two are one command with a sign. The
    /// magnitude is not a speed anybody chose -- it is the unit velocity an
    /// `AnimationController` flings with by default, and `close` names it only
    /// because it has to write something down to put a minus in front of it.
    pub fn fling_velocity(command: DrawerCommand) -> f32 {
        match command {
            DrawerCommand::Open => 1.0,
            DrawerCommand::Close => -1.0,
        }
    }

    /// Upstream's `_animationStatusChanged`: what a status change does to the
    /// back-button entry.
    ///
    /// # Both ends of the motion are its beginning
    ///
    /// The two statuses that mean *arrived* -- `completed` and `dismissed` --
    /// do nothing at all, and upstream writes them out with an empty body
    /// rather than omitting them. Everything happens on `forward` and
    /// `reverse`, which are the moments the drawer **starts** moving.
    ///
    /// That is the opposite of the obvious arrangement, and it is right. A
    /// drawer sliding in is already covering the page, so a back press has to
    /// dismiss it from the first frame rather than from the last. And a drawer
    /// sliding out must stop claiming the back button immediately, or a press
    /// during the closing animation would be swallowed to close something
    /// already closing.
    ///
    /// Waiting for the terminal statuses would leave a window at each end
    /// where what the screen shows and what the back button does disagree.
    pub fn history_entry_change(status: AnimationStatus) -> HistoryEntryChange {
        match status {
            AnimationStatus::Forward => HistoryEntryChange::Ensure,
            AnimationStatus::Reverse => HistoryEntryChange::Remove,
            AnimationStatus::Dismissed | AnimationStatus::Completed => HistoryEntryChange::Nothing,
        }
    }

    /// Upstream's `_handleHistoryEntryRemoved`, whose two lines are ordered
    /// and not merely sequential:
    ///
    /// ```dart
    /// _historyEntry = null;
    /// close();
    /// ```
    ///
    /// The entry is dropped **before** the close, and that is what stops the
    /// loop. `close` flings in reverse, the reverse status asks for the entry
    /// to be removed, and removing an entry is what called this in the first
    /// place. Clearing the field first means the removal finds nothing to
    /// remove and the cycle ends there.
    ///
    /// Written the other way round it would still terminate -- the second
    /// removal would be of an already-removed entry -- but it would depend on
    /// the history entry tolerating that, which is a promise from somewhere
    /// else. This way the ordering carries the guarantee itself.
    pub fn on_history_entry_removed(entry: &mut Option<LocalHistoryEntry>) -> DrawerCommand {
        *entry = None;
        DrawerCommand::Close
    }

    /// Upstream's `dragAreaWidth`.
    ///
    /// Twenty pixels **plus the system inset on whichever side this drawer is
    /// actually on**. A phone with a curved edge or a notch in landscape puts
    /// unusable pixels along that edge, and a grab strip entirely inside them
    /// would leave the drawer impossible to open by drag.
    ///
    /// The four cases are one rule written out: start-aligned reads the leading
    /// inset, end-aligned the trailing one, and which is which depends on the
    /// reading direction.
    pub fn drag_area_width(&self, left_inset: f32, right_inset: f32, left_to_right: bool) -> f32 {
        if let Some(width) = self.edge_drag_width {
            return width;
        }
        let inset = match (self.alignment, left_to_right) {
            (DrawerAlignment::Start, true) => left_inset,
            (DrawerAlignment::Start, false) => right_inset,
            (DrawerAlignment::End, false) => left_inset,
            (DrawerAlignment::End, true) => right_inset,
        };
        EDGE_DRAG_WIDTH + inset
    }

    /// Whether an edge drag can open it here.
    ///
    /// Upstream disables it on desktop outright. There is no edge swipe on a
    /// desktop -- a drag from the window edge is a resize or a selection -- and
    /// taking it would break both without giving anything back, since a desktop
    /// drawer is opened from a button.
    pub fn opens_by_edge_drag(&self, is_desktop: bool) -> bool {
        self.enable_open_drag_gesture && !is_desktop
    }

    /// Upstream's outer and inner alignments, which are **opposites**: the
    /// outer is the screen edge the drawer hugs, the inner is the edge its
    /// contents are pushed against as it slides.
    pub fn outer_alignment(&self) -> DrawerAlignment {
        self.alignment
    }

    pub fn inner_alignment(&self) -> DrawerAlignment {
        match self.alignment {
            DrawerAlignment::Start => DrawerAlignment::End,
            DrawerAlignment::End => DrawerAlignment::Start,
        }
    }
}

/// Upstream `DrawerControllerState` is the state of the above; this crate holds
/// the decisions on one type.
pub type DrawerControllerState = DrawerController;

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
    // -- The controller ---------------------------------------------------------

    #[test]
    fn speed_overrules_position_but_only_when_it_is_a_real_fling() {
        // A drawer nine tenths open, flicked shut, closes; released gently at
        // the same place, it opens.
        let nearly_open = DrawerController::new(DrawerAlignment::Start).with_value(0.9);
        assert_eq!(
            nearly_open.settle(-MIN_FLING_VELOCITY - 1.0, DRAWER_WIDTH, TextDirection::Ltr),
            SettleOutcome::Flung { opening: false }
        );
        assert_eq!(
            nearly_open.settle(-MIN_FLING_VELOCITY + 1.0, DRAWER_WIDTH, TextDirection::Ltr),
            SettleOutcome::SettledToNearest { opening: true }
        );
    }

    #[test]
    fn below_the_threshold_the_halfway_point_decides() {
        let start = DrawerAlignment::Start;
        assert_eq!(
            DrawerController::new(start).with_value(0.49).settle(
                0.0,
                DRAWER_WIDTH,
                TextDirection::Ltr
            ),
            SettleOutcome::SettledToNearest { opening: false }
        );
        assert_eq!(
            DrawerController::new(start).with_value(0.5).settle(
                0.0,
                DRAWER_WIDTH,
                TextDirection::Ltr
            ),
            SettleOutcome::SettledToNearest { opening: true }
        );
    }

    #[test]
    fn a_drag_that_never_opened_anything_has_nothing_to_settle() {
        let closed = DrawerController::new(DrawerAlignment::Start);
        assert_eq!(
            closed.settle(-1000.0, DRAWER_WIDTH, TextDirection::Ltr),
            SettleOutcome::NothingToSettle
        );
    }

    #[test]
    fn an_end_aligned_drawer_reads_the_same_finger_the_other_way() {
        let start = DrawerController::new(DrawerAlignment::Start).with_value(0.5);
        let end = DrawerController::new(DrawerAlignment::End).with_value(0.5);
        let rightwards = MIN_FLING_VELOCITY + 100.0;

        assert_eq!(
            start.settle(rightwards, DRAWER_WIDTH, TextDirection::Ltr),
            SettleOutcome::Flung { opening: true }
        );
        assert_eq!(
            end.settle(rightwards, DRAWER_WIDTH, TextDirection::Ltr),
            SettleOutcome::Flung { opening: false },
            "the same swipe closes the one on the other side"
        );
    }

    #[test]
    fn a_wider_drawer_moves_more_slowly_for_the_same_finger_speed() {
        // Which is what makes a wide one feel heavy: the controller works in
        // 0..1, and the width is the exchange rate.
        let controller = DrawerController::new(DrawerAlignment::Start);
        let narrow = controller.visual_velocity(1000.0, 200.0, TextDirection::Ltr);
        let wide = controller.visual_velocity(1000.0, 400.0, TextDirection::Ltr);
        assert!(wide < narrow);
        assert_eq!(wide * 2.0, narrow);
    }

    // -- Where you can grab it ------------------------------------------------------

    #[test]
    fn the_grab_strip_starts_inside_the_system_inset() {
        // A phone with a curved edge puts unusable pixels along it, and a strip
        // entirely inside them would leave the drawer impossible to drag open.
        let controller = DrawerController::new(DrawerAlignment::Start);
        assert_eq!(controller.drag_area_width(0.0, 0.0, true), EDGE_DRAG_WIDTH);
        assert_eq!(
            controller.drag_area_width(44.0, 0.0, true),
            EDGE_DRAG_WIDTH + 44.0
        );
    }

    #[test]
    fn each_drawer_reads_the_inset_on_the_side_it_is_actually_on() {
        let start = DrawerController::new(DrawerAlignment::Start);
        let end = DrawerController::new(DrawerAlignment::End);

        assert_eq!(
            start.drag_area_width(44.0, 10.0, true),
            EDGE_DRAG_WIDTH + 44.0
        );
        assert_eq!(
            start.drag_area_width(44.0, 10.0, false),
            EDGE_DRAG_WIDTH + 10.0,
            "in right-to-left the start side is the right one"
        );
        assert_eq!(
            end.drag_area_width(44.0, 10.0, true),
            EDGE_DRAG_WIDTH + 10.0
        );
        assert_eq!(
            end.drag_area_width(44.0, 10.0, false),
            EDGE_DRAG_WIDTH + 44.0
        );
    }

    #[test]
    fn a_caller_who_named_a_width_gets_it_untouched() {
        let mut controller = DrawerController::new(DrawerAlignment::Start);
        controller.edge_drag_width = Some(64.0);
        assert_eq!(controller.drag_area_width(44.0, 0.0, true), 64.0);
    }

    #[test]
    fn there_is_no_edge_swipe_on_a_desktop() {
        // A drag from the window edge is a resize or a selection, and taking it
        // would break both while giving nothing back -- a desktop drawer is
        // opened from a button.
        let controller = DrawerController::new(DrawerAlignment::Start);
        assert!(controller.opens_by_edge_drag(false));
        assert!(!controller.opens_by_edge_drag(true));

        let mut disabled = controller;
        disabled.enable_open_drag_gesture = false;
        assert!(!disabled.opens_by_edge_drag(false));
    }

    #[test]
    fn the_outer_and_inner_alignments_are_opposites() {
        // The outer is the screen edge the drawer hugs; the inner is the edge
        // its contents are pushed against as it slides.
        let start = DrawerController::new(DrawerAlignment::Start);
        assert_eq!(start.outer_alignment(), DrawerAlignment::Start);
        assert_eq!(start.inner_alignment(), DrawerAlignment::End);

        let end = DrawerController::new(DrawerAlignment::End);
        assert_eq!(end.outer_alignment(), DrawerAlignment::End);
        assert_eq!(end.inner_alignment(), DrawerAlignment::Start);
    }
}

#[cfg(test)]
mod drawer_direction_tests {
    use super::{DrawerAlignment, DrawerController, MIN_FLING_VELOCITY, SettleOutcome};
    use crate::direction::TextDirection;

    const WIDTH: f32 = 304.0;

    #[test]
    fn a_start_drawer_reverses_in_a_right_to_left_reading() {
        // It lives on the right there, so the drag that opens it goes the
        // other way. This read the alignment alone and was inverted for every
        // gesture on both drawers in RTL.
        let start = DrawerController::new(DrawerAlignment::Start);
        assert_eq!(start.direction_factor(TextDirection::Ltr), 1.0);
        assert_eq!(start.direction_factor(TextDirection::Rtl), -1.0);
    }

    #[test]
    fn and_so_does_an_end_drawer_the_other_way() {
        let end = DrawerController::new(DrawerAlignment::End);
        assert_eq!(end.direction_factor(TextDirection::Ltr), -1.0);
        assert_eq!(end.direction_factor(TextDirection::Rtl), 1.0);
    }

    #[test]
    fn the_two_arguments_matter_only_together() {
        // Flipping either flips the answer; flipping both leaves it alone.
        // That is what makes it one exclusive-or rather than two rules, and a
        // port that read one argument could not be right for more than half
        // the four cases.
        for alignment in [DrawerAlignment::Start, DrawerAlignment::End] {
            let controller = DrawerController::new(alignment);
            let ltr = controller.direction_factor(TextDirection::Ltr);
            let rtl = controller.direction_factor(TextDirection::Rtl);
            assert_eq!(ltr, -rtl, "{alignment:?}");
        }
        let start = DrawerController::new(DrawerAlignment::Start);
        let end = DrawerController::new(DrawerAlignment::End);
        assert_eq!(
            start.direction_factor(TextDirection::Ltr),
            end.direction_factor(TextDirection::Rtl),
            "both flipped is no flip at all"
        );
    }

    #[test]
    fn a_fling_opens_the_drawer_the_finger_moved_toward() {
        // Through settle, not the factor alone: the same push opens a start
        // drawer in one reading and closes it in the other.
        let half_open = DrawerController::new(DrawerAlignment::Start).with_value(0.5);
        let push = MIN_FLING_VELOCITY + 100.0;
        assert_eq!(
            half_open.settle(push, WIDTH, TextDirection::Ltr),
            SettleOutcome::Flung { opening: true }
        );
        assert_eq!(
            half_open.settle(push, WIDTH, TextDirection::Rtl),
            SettleOutcome::Flung { opening: false },
            "the same push, the other way round"
        );
    }

    #[test]
    fn but_a_gentle_release_settles_by_position_in_either_reading() {
        // Below the fling threshold the direction never enters into it, so
        // both readings agree -- which is why the test above uses a fling.
        let nearly_open = DrawerController::new(DrawerAlignment::Start).with_value(0.9);
        for direction in [TextDirection::Ltr, TextDirection::Rtl] {
            assert_eq!(
                nearly_open.settle(1.0, WIDTH, direction),
                SettleOutcome::SettledToNearest { opening: true },
                "{direction:?}"
            );
        }
    }
}

#[cfg(test)]
mod back_button_tests {
    use super::{DrawerCommand, DrawerController, HistoryEntryChange};
    use crate::animation::AnimationStatus;
    use crate::routes::LocalHistoryEntry;

    #[test]
    fn the_two_commands_are_one_fling_with_a_sign() {
        assert_eq!(DrawerController::fling_velocity(DrawerCommand::Open), 1.0);
        assert_eq!(DrawerController::fling_velocity(DrawerCommand::Close), -1.0);
    }

    #[test]
    fn the_back_button_is_claimed_when_the_drawer_starts_moving_not_when_it_arrives() {
        // A drawer sliding in already covers the page, so a back press has to
        // dismiss it from the first frame.
        assert_eq!(
            DrawerController::history_entry_change(AnimationStatus::Forward),
            HistoryEntryChange::Ensure
        );
        assert_eq!(
            DrawerController::history_entry_change(AnimationStatus::Reverse),
            HistoryEntryChange::Remove
        );
    }

    #[test]
    fn and_arriving_at_either_end_does_nothing_at_all() {
        // Upstream writes both terminal statuses out with an empty body rather
        // than omitting them, which is the shape of a decision rather than an
        // oversight.
        for status in [AnimationStatus::Completed, AnimationStatus::Dismissed] {
            assert_eq!(
                DrawerController::history_entry_change(status),
                HistoryEntryChange::Nothing,
                "{status:?}"
            );
        }
    }

    #[test]
    fn every_status_is_answered_so_none_of_them_falls_through() {
        // The point of the previous two together: there is no status for which
        // the drawer has no rule.
        let all = [
            AnimationStatus::Forward,
            AnimationStatus::Reverse,
            AnimationStatus::Completed,
            AnimationStatus::Dismissed,
        ];
        let changes: Vec<_> = all
            .iter()
            .map(|status| DrawerController::history_entry_change(*status))
            .collect();
        assert_eq!(
            changes
                .iter()
                .filter(|c| **c == HistoryEntryChange::Ensure)
                .count(),
            1
        );
        assert_eq!(
            changes
                .iter()
                .filter(|c| **c == HistoryEntryChange::Remove)
                .count(),
            1
        );
    }

    #[test]
    fn a_back_press_drops_the_entry_before_it_starts_the_close() {
        // Which is what stops the loop: close flings in reverse, reverse asks
        // for the entry to be removed, and removing an entry is what called
        // this. Clearing it first means that removal finds nothing.
        let mut entry = Some(LocalHistoryEntry::new(1));
        let command = DrawerController::on_history_entry_removed(&mut entry);
        assert_eq!(command, DrawerCommand::Close);
        assert!(entry.is_none(), "cleared before the close, not after");

        // And the close's own status change now finds nothing to remove.
        assert_eq!(
            DrawerController::history_entry_change(AnimationStatus::Reverse),
            HistoryEntryChange::Remove
        );
        assert!(entry.is_none(), "so the cycle ends here");
    }
}

#[cfg(test)]
mod semantic_label_tests {
    use super::Drawer;
    use crate::editable_text::TargetPlatform;
    use crate::framework::leaf;
    use crate::render::RenderConstrainedBox;

    fn drawer() -> Drawer {
        Drawer::new(leaf(|| RenderConstrainedBox::tight(10.0, 10.0)))
    }

    #[test]
    fn an_unnamed_drawer_is_announced_as_a_navigation_menu() {
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert_eq!(
                drawer().resolved_semantic_label(platform).as_deref(),
                Some("Navigation menu"),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn but_on_the_two_apple_platforms_it_is_announced_as_nothing() {
        // Not an omission. VoiceOver already says a modal panel appeared and
        // that there is a way out; the framework adding "Navigation menu" on
        // top would be a second voice repeating the first.
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert_eq!(
                drawer().resolved_semantic_label(platform),
                None,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_caller_that_named_it_wins_everywhere() {
        // The platform decides the fallback, never the override.
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
            TargetPlatform::IOS,
            TargetPlatform::MacOS,
        ] {
            assert_eq!(
                drawer()
                    .with_semantic_label("Account switcher")
                    .resolved_semantic_label(platform)
                    .as_deref(),
                Some("Account switcher"),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_platforms_disagree_only_about_the_unnamed_case() {
        // Stated as its own test because it is the whole content of the
        // switch: with a label every platform agrees, without one they split.
        let named = |platform| {
            drawer()
                .with_semantic_label("Account switcher")
                .resolved_semantic_label(platform)
        };
        assert_eq!(named(TargetPlatform::IOS), named(TargetPlatform::Android));
        assert_ne!(
            drawer().resolved_semantic_label(TargetPlatform::IOS),
            drawer().resolved_semantic_label(TargetPlatform::Android)
        );
    }
}
