//! The live tooltip: `raw_tooltip.rs`'s clock and geometry, hosted.
//!
//! `raw_tooltip.rs` has had the whole of a tooltip's behaviour for a long time
//! -- the wait before showing, the stay after, the exit delay, the touch path,
//! the announcement, and `position_dependent_box` for where the bubble goes --
//! and nothing to put it in. This is the part that was missing: an
//! `OverlayPortal` for the bubble, and the target's position in the overlay's
//! coordinates to place it against.
//!
//! # The two halves of "where"
//!
//! `position_dependent_box` wants the target's centre **in global
//! coordinates**, and a widget does not know where it is until layout has run.
//! So the two halves happen in different phases: the portal hands the bubble to
//! the theatre during build, and [`crate::theatre::RenderAnchored`] asks the
//! target where it ended up once layout is over.
//!
//! That is the whole reason this could not be written before L0: without
//! `RenderRef::transform_to` there was no way to ask.

use std::cell::RefCell;
use std::rc::Rc;

use crate::framework::{AnyWidget, many};
use crate::raw_tooltip::{TooltipPositionContext, position_dependent_box};
use crate::render::{BoxConstraints, Offset, RenderBox, RenderRef, Size};
use crate::theatre::{Anchor, Placement, PortalController, anchored, overlay_portal};

/// Upstream's `Tooltip.verticalOffset` default.
pub const DEFAULT_VERTICAL_OFFSET: f32 = 24.0;

/// The placement rule a tooltip uses: upstream's `positionDependentBox`, in the
/// shape [`crate::theatre::RenderAnchored`] asks for.
pub fn tooltip_placement(vertical_offset: f32, prefer_below: bool) -> Placement {
    Rc::new(
        move |target: crate::engine::Rect, bubble: Size, overlay: Size| {
            let context = TooltipPositionContext::new(
                // Upstream passes the target's *centre*.
                (
                    (target.left + target.right) / 2.0,
                    (target.top + target.bottom) / 2.0,
                ),
                (target.width(), target.height()),
                (bubble.width, bubble.height),
            )
            .with_overlay((overlay.width, overlay.height))
            .with_vertical_offset(vertical_offset)
            .with_prefer_below(prefer_below);
            let (x, y) = position_dependent_box(&context);
            Offset::new(x, y)
        },
    )
}

/// A tooltip: `child` as it was, and `bubble` above it while the pointer rests
/// on it.
///
/// The trigger here is hover in and hover out. The delays, the touch path and
/// the announcement belong to `raw_tooltip.rs` and are driven by whoever owns
/// the clock -- [`Tooltip::controller`] is how they reach this.
pub struct Tooltip {
    id: u64,
    controller: PortalController,
    anchor: Anchor,
    child: RefCell<Option<AnyWidget>>,
    bubble: Rc<dyn Fn() -> AnyWidget>,
    vertical_offset: f32,
    prefer_below: bool,
}

impl Tooltip {
    pub fn new(id: u64, child: AnyWidget, bubble: impl Fn() -> AnyWidget + 'static) -> Tooltip {
        Tooltip {
            id,
            controller: PortalController::new(),
            anchor: Anchor::new(),
            child: RefCell::new(Some(child)),
            bubble: Rc::new(bubble),
            vertical_offset: DEFAULT_VERTICAL_OFFSET,
            prefer_below: true,
        }
    }

    pub fn with_vertical_offset(mut self, offset: f32) -> Self {
        self.vertical_offset = offset;
        self
    }

    pub fn with_prefer_below(mut self, prefer_below: bool) -> Self {
        self.prefer_below = prefer_below;
        self
    }

    /// The controller, so a caller with its own clock -- a `RawTooltipState`,
    /// say -- decides when the bubble is up.
    pub fn controller(&self) -> PortalController {
        self.controller.clone()
    }

    pub fn anchor(&self) -> Anchor {
        self.anchor.clone()
    }

    /// The hit-test identity of the target.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn build(self) -> AnyWidget {
        let Tooltip {
            id,
            controller,
            anchor,
            child,
            bubble,
            vertical_offset,
            prefer_below,
        } = self;
        let child = child.borrow_mut().take().expect("a tooltip has a child");

        let show = controller.clone();
        let hide = controller.clone();
        let handlers = crate::gestures::PointerHandlers::new().with_hover_change(move |inside| {
            if inside {
                show.show();
            } else {
                hide.hide();
            }
        });

        // The anchor is filled in from the target's own assemble, which runs
        // before any layout -- so by the time the bubble needs a position there
        // is a handle to ask.
        let anchor_for_target = anchor.clone();
        let target = many(vec![child], move |mut rendered| {
            let child = rendered.pop().expect("the target");
            let region = crate::render::RenderPointerRegion::new(id, child.clone())
                .with_handlers(handlers.clone());
            anchor_for_target.set(child);
            region
        });

        let place = tooltip_placement(vertical_offset, prefer_below);
        overlay_portal(controller, target, move || {
            anchored(anchor.clone(), Rc::clone(&place), (bubble)())
        })
    }
}

/// A tooltip with the defaults.
pub fn tooltip(id: u64, child: AnyWidget, bubble: impl Fn() -> AnyWidget + 'static) -> AnyWidget {
    Tooltip::new(id, child, bubble).build()
}

/// Whether a pointer that has rested this long should show the tooltip.
/// Upstream's `waitDuration`, asked of the clock in `raw_tooltip.rs` rather
/// than kept here.
pub fn should_show_after(rested_ms: f32, wait_ms: f32) -> bool {
    rested_ms >= wait_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::ElementTree;
    use crate::render::{RenderConstrainedBox, RenderPadding};
    use crate::theatre::RenderAnchored;
    use crate::theatre::overlay;

    fn leaf(width: f32, height: f32) -> AnyWidget {
        crate::framework::leaf(move || RenderConstrainedBox::tight(width, height))
    }

    /// The page: a target pushed well away from the origin, so that "it read
    /// the target's global position" is distinguishable from "it read zero".
    fn page_with_tooltip(
        inset: f32,
        controller_out: &Rc<RefCell<Option<PortalController>>>,
    ) -> AnyWidget {
        let tip = Tooltip::new(9001, leaf(60.0, 20.0), || leaf(100.0, 30.0));
        *controller_out.borrow_mut() = Some(tip.controller());
        let anchored = tip.build();
        many(vec![anchored], move |mut rendered| {
            // Aligned inside the padding, so the target keeps its own 60 x 20.
            // Without this the tight constraints reach all the way down and
            // stretch it to fill the page -- which it did, and the arithmetic
            // in these tests was written against the size it was asked for
            // rather than the size it got.
            RenderPadding::new(
                crate::render::EdgeInsets::only(inset, inset, 0.0, 0.0),
                crate::render::RenderAlign::new(
                    crate::render::Alignment::new(-1.0, -1.0),
                    rendered.pop().expect("the tooltip target"),
                ),
            )
        })
    }

    fn laid_out(tree: &mut ElementTree) -> RenderRef {
        let root = tree.build_render_tree().expect("a mounted root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        // A frame lays out, then paints and hit-tests -- and the bubble's
        // position is worked out in those phases, not in layout, because that
        // is where asking an ancestor is legal. A harness that stopped after
        // layout would read the position from before the target had one.
        let mut discard = crate::render::HitTestResult::new();
        root.hit_test(Offset::new(1.0, 1.0), &mut discard);
        root
    }

    /// Where the bubble ended up, by finding the positioner in the render tree.
    fn bubble_offset(root: &RenderRef) -> Option<Offset> {
        fn walk(handle: &RenderRef, found: &mut Option<Offset>) {
            if found.is_some() {
                return;
            }
            let children: Vec<RenderRef> = handle.with(|object| {
                if let Some(position) = object.as_any().downcast_ref::<RenderAnchored>() {
                    *found = Some(position.placed());
                }
                let mut kids = Vec::new();
                object.visit_children(&mut |child, _| {
                    if let Some(child) = child.as_any().downcast_ref::<RenderRef>() {
                        kids.push(child.clone());
                    }
                });
                kids
            });
            for child in children {
                walk(&child, found);
            }
        }
        let mut found = None;
        walk(root, &mut found);
        found
    }

    fn mounted(inset: f32) -> (ElementTree, PortalController) {
        let slot: Rc<RefCell<Option<PortalController>>> = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(overlay(page_with_tooltip(inset, &slot)));
        tree.build_render_tree();
        let controller = slot.borrow().clone().expect("a controller");
        (tree, controller)
    }

    #[test]
    fn a_tooltip_shows_nothing_until_it_is_asked() {
        let (mut tree, controller) = mounted(0.0);
        let root = laid_out(&mut tree);
        assert!(!controller.is_showing());
        assert_eq!(bubble_offset(&root), None, "no bubble in the tree");
    }

    #[test]
    fn showing_it_puts_a_bubble_in_the_overlay() {
        let (mut tree, controller) = mounted(0.0);
        controller.show();
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        assert!(bubble_offset(&root).is_some(), "the bubble is hosted");
    }

    // -- The closed loop: L0 reaches L2 ------------------------------------------

    #[test]
    fn the_bubble_follows_the_target_across_the_page() {
        // The same tooltip, once near the origin and once pushed 300 across and
        // 200 down. If the bubble moves with it, the position came from the
        // target's *global* rectangle -- which is transform_to, reached through
        // an overlay, from a widget built somewhere else entirely.
        let (mut near_tree, near) = mounted(0.0);
        near.show();
        near_tree.rebuild_dirty();
        let near_at = bubble_offset(&laid_out(&mut near_tree)).expect("shown");

        let (mut far_tree, far) = mounted(300.0);
        far.show();
        far_tree.rebuild_dirty();
        let far_at = bubble_offset(&laid_out(&mut far_tree)).expect("shown");

        assert_ne!(
            near_at, far_at,
            "a bubble that did not move with its target read zero, not the target"
        );
        assert!(
            far_at.dx > near_at.dx && far_at.dy > near_at.dy,
            "and it moved the way the target did: {near_at:?} -> {far_at:?}"
        );
    }

    #[test]
    fn the_bubble_is_centred_on_the_target_and_below_it() {
        // Target: 60 x 20 at (300, 300), so its centre is (330, 310).
        // Bubble: 100 wide, so centred means x = 330 - 50 = 280.
        // Below means y = centre + the vertical offset.
        let (mut tree, controller) = mounted(300.0);
        controller.show();
        tree.rebuild_dirty();
        let at = bubble_offset(&laid_out(&mut tree)).expect("shown");

        assert_eq!(at.dx, 280.0, "centred on the target");
        assert_eq!(at.dy, 310.0 + 24.0, "and the default offset below it");
    }

    #[test]
    fn a_target_near_the_bottom_puts_its_bubble_above_itself() {
        // The preference is a preference: below if it fits, above if it does
        // not. 560 down a 600-tall overlay leaves no room underneath.
        let (mut tree, controller) = mounted(560.0);
        controller.show();
        tree.rebuild_dirty();
        let at = bubble_offset(&laid_out(&mut tree)).expect("shown");

        assert!(
            at.dy < 560.0,
            "the bubble went above the target rather than off the screen: {at:?}"
        );
    }

    #[test]
    fn a_target_at_the_right_edge_keeps_its_bubble_on_screen() {
        // 780 across an 800-wide overlay: centring a 100-wide bubble on it
        // would put half of it past the edge.
        let (mut tree, controller) = mounted(760.0);
        controller.show();
        tree.rebuild_dirty();
        let at = bubble_offset(&laid_out(&mut tree)).expect("shown");

        assert!(
            at.dx + 100.0 <= 800.0,
            "a tooltip off the edge is worse than one not quite where it was asked: {at:?}"
        );
    }

    #[test]
    fn hiding_it_takes_the_bubble_away() {
        let (mut tree, controller) = mounted(100.0);
        controller.show();
        tree.rebuild_dirty();
        assert!(bubble_offset(&laid_out(&mut tree)).is_some());

        controller.hide();
        tree.rebuild_dirty();
        assert_eq!(bubble_offset(&laid_out(&mut tree)), None);
    }

    #[test]
    fn the_wait_is_the_clocks_question_not_the_widgets() {
        assert!(!should_show_after(100.0, 500.0));
        assert!(should_show_after(500.0, 500.0));
        assert!(should_show_after(900.0, 500.0));
    }
}
