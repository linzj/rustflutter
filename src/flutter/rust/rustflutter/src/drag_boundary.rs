// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! How far a dragged thing may go (upstream `widgets/drag_boundary.dart`).
//!
//! A draggable dialog that can be dragged off the screen is a dialog the
//! reader has lost. `DragBoundary` marks a region, and anything dragging
//! inside it can ask two questions: is this position still inside, and if
//! not, what is the nearest one that is.
//!
//! # Recorded divergences
//!
//! * Upstream is an `InheritedWidget` whose `forRectOf` finds the element,
//!   asks its render object for its size and converts to global coordinates.
//!   Here the boundary rectangle is what is provided, because a render
//!   object's global rectangle is not something a `BuildContext` in this
//!   crate can ask for -- so the caller states the rectangle rather than the
//!   framework deriving it. Upstream's `useGlobalPosition` chooses between
//!   the global rectangle and one at the origin; that choice is the caller's
//!   here, in which rectangle it provides.
//! * Upstream's `nearestPositionWithinBoundary` throws when the dragged thing
//!   is larger than the boundary. There is nothing to throw to here and
//!   nothing sensible to return, so it answers nothing -- see
//!   [`DragBoundaryDelegate::nearest_position_within_boundary`].

use crate::engine::Rect;
use crate::framework::{AnyWidget, BuildContext, provide};

/// Upstream `DragBoundaryDelegate<T>`: the two questions a drag asks of a
/// boundary.
///
/// Generic upstream because a boundary might one day constrain something
/// other than a rectangle -- a point, a path. Only the rectangle is
/// implemented there and here, and the trait is what says so.
pub trait DragBoundaryDelegate<T> {
    /// Upstream `isWithinBoundary`.
    fn is_within_boundary(&self, dragged_object: T) -> bool;

    /// Upstream `nearestPositionWithinBoundary`.
    ///
    /// Nothing when the dragged thing does not fit at all, where upstream
    /// throws: there is no nearest position inside a boundary too small to
    /// hold it, and inventing one -- pinning to a corner, say -- would place
    /// the thing somewhere the caller never asked for and could not detect.
    fn nearest_position_within_boundary(&self, dragged_object: T) -> Option<T>;
}

/// Upstream's `_DragBoundaryDelegateForRect`.
///
/// A boundary of `None` is upstream's "there is no `DragBoundary` above you":
/// everything is inside it and nothing needs moving. That is what
/// [`DragBoundary::for_rect_of`] falls back to, and it is why the answers are
/// permissive rather than empty -- a drag with no boundary is unbounded, not
/// forbidden.
pub struct DragBoundaryDelegateForRect {
    pub boundary: Option<Rect>,
}

impl DragBoundaryDelegateForRect {
    pub fn new(boundary: Option<Rect>) -> DragBoundaryDelegateForRect {
        DragBoundaryDelegateForRect { boundary }
    }
}

impl DragBoundaryDelegate<Rect> for DragBoundaryDelegateForRect {
    /// Upstream checks the two opposite corners, which is the whole test for
    /// one rectangle inside another.
    fn is_within_boundary(&self, dragged_object: Rect) -> bool {
        let Some(boundary) = self.boundary else {
            return true;
        };
        dragged_object.left >= boundary.left
            && dragged_object.top >= boundary.top
            && dragged_object.right <= boundary.right
            && dragged_object.bottom <= boundary.bottom
    }

    fn nearest_position_within_boundary(&self, dragged_object: Rect) -> Option<Rect> {
        let Some(boundary) = self.boundary else {
            return Some(dragged_object);
        };
        // Upstream throws here. A thing wider or taller than the boundary has
        // no position inside it, and the clamp below would produce a
        // nonsensical one -- the low end of the range above the high end.
        if boundary.right - dragged_object.width() < boundary.left
            || boundary.bottom - dragged_object.height() < boundary.top
        {
            return None;
        }
        // The far edge of the range is the boundary's less the object's own
        // size, which is what keeps the *whole* of it inside rather than just
        // its top-left corner.
        let left = dragged_object
            .left
            .clamp(boundary.left, boundary.right - dragged_object.width());
        let top = dragged_object
            .top
            .clamp(boundary.top, boundary.bottom - dragged_object.height());
        Some(Rect::xywh(
            left,
            top,
            dragged_object.width(),
            dragged_object.height(),
        ))
    }
}

/// Upstream `DragBoundary`: marks the region a drag is confined to.
pub struct DragBoundary;

impl DragBoundary {
    /// Installs a boundary for the subtree.
    ///
    /// The rectangle is in whatever coordinates the drags below will ask in;
    /// upstream's `useGlobalPosition` is that same choice made at the other
    /// end.
    pub fn new(boundary: Rect, child: AnyWidget) -> AnyWidget {
        provide(DragBoundaryRect(boundary), child)
    }

    /// Upstream `forRectMaybeOf`.
    pub fn for_rect_maybe_of(context: &mut BuildContext) -> Option<DragBoundaryDelegateForRect> {
        context
            .inherited::<DragBoundaryRect>()
            .map(|rect| DragBoundaryDelegateForRect::new(Some(rect.0)))
    }

    /// Upstream `forRectOf`, which falls back to an unbounded delegate rather
    /// than to nothing: code that drags does not want to know whether anybody
    /// set a boundary, only where it may go.
    pub fn for_rect_of(context: &mut BuildContext) -> DragBoundaryDelegateForRect {
        DragBoundary::for_rect_maybe_of(context)
            .unwrap_or_else(|| DragBoundaryDelegateForRect::new(None))
    }
}

/// The boundary as it is handed down. A newtype so that a subtree cannot
/// pick up an unrelated `Rect` somebody else provided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragBoundaryRect(pub Rect);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{BuildContext as Context, Component, ElementTree, component, leaf};
    use crate::widgets::SizedBox;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A 100x100 boundary at the origin.
    fn boundary() -> DragBoundaryDelegateForRect {
        DragBoundaryDelegateForRect::new(Some(Rect::ltrb(0.0, 0.0, 100.0, 100.0)))
    }

    #[test]
    fn a_rect_inside_the_boundary_is_inside_and_does_not_move() {
        let delegate = boundary();
        let inside = Rect::ltrb(10.0, 10.0, 30.0, 30.0);
        assert!(delegate.is_within_boundary(inside));
        assert_eq!(
            delegate.nearest_position_within_boundary(inside),
            Some(inside)
        );
    }

    #[test]
    fn a_rect_touching_the_edge_is_still_inside() {
        // Flush against the boundary is not outside it, and a drag that
        // nudged such a thing away from the edge would be visibly wrong.
        let delegate = boundary();
        assert!(delegate.is_within_boundary(Rect::ltrb(0.0, 0.0, 20.0, 20.0)));
        assert!(delegate.is_within_boundary(Rect::ltrb(80.0, 80.0, 100.0, 100.0)));
    }

    #[test]
    fn the_whole_rect_is_kept_inside_and_not_just_its_corner() {
        // The far edge of the clamp is the boundary's less the object's own
        // size. Clamping the top-left alone would let a wide thing hang off
        // the right, which is the mistake the subtraction exists to prevent.
        let delegate = boundary();
        let hanging = Rect::ltrb(90.0, 10.0, 130.0, 50.0);
        assert!(!delegate.is_within_boundary(hanging));
        assert_eq!(
            delegate.nearest_position_within_boundary(hanging),
            Some(Rect::ltrb(60.0, 10.0, 100.0, 50.0)),
            "moved left until its right edge met the boundary"
        );
    }

    #[test]
    fn a_rect_off_the_near_side_is_pushed_back_to_the_edge() {
        let delegate = boundary();
        assert_eq!(
            delegate.nearest_position_within_boundary(Rect::ltrb(-30.0, -40.0, 10.0, 0.0)),
            Some(Rect::ltrb(0.0, 0.0, 40.0, 40.0))
        );
    }

    #[test]
    fn each_axis_is_clamped_on_its_own() {
        // A thing off the left and inside vertically moves sideways only.
        let delegate = boundary();
        assert_eq!(
            delegate.nearest_position_within_boundary(Rect::ltrb(-10.0, 40.0, 10.0, 60.0)),
            Some(Rect::ltrb(0.0, 40.0, 20.0, 60.0))
        );
    }

    #[test]
    fn something_too_big_for_the_boundary_has_no_nearest_position() {
        // Upstream throws. There is no position inside a boundary too small
        // to hold the thing, and inventing one -- pinning to a corner --
        // would put it somewhere the caller never asked for and could not
        // detect.
        let delegate = boundary();
        assert_eq!(
            delegate.nearest_position_within_boundary(Rect::ltrb(0.0, 0.0, 200.0, 50.0)),
            None,
            "wider than the boundary"
        );
        assert_eq!(
            delegate.nearest_position_within_boundary(Rect::ltrb(0.0, 0.0, 50.0, 200.0)),
            None,
            "taller than the boundary"
        );
        // Exactly the boundary's size still fits, and has one position.
        assert_eq!(
            delegate.nearest_position_within_boundary(Rect::ltrb(20.0, 20.0, 120.0, 120.0)),
            Some(Rect::ltrb(0.0, 0.0, 100.0, 100.0))
        );
    }

    #[test]
    fn no_boundary_means_unbounded_rather_than_forbidden() {
        // What `for_rect_of` falls back to when nothing above set one. A
        // drag with no boundary should go anywhere, so the answers are
        // permissive: everything is inside, and nothing needs moving.
        let delegate = DragBoundaryDelegateForRect::new(None);
        let anywhere = Rect::ltrb(-500.0, -500.0, 9000.0, 9000.0);
        assert!(delegate.is_within_boundary(anywhere));
        assert_eq!(
            delegate.nearest_position_within_boundary(anywhere),
            Some(anywhere)
        );
    }

    struct Reader {
        seen: Rc<RefCell<Option<Option<Rect>>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut Context) -> AnyWidget {
            *self.seen.borrow_mut() = Some(DragBoundary::for_rect_of(context).boundary);
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    #[test]
    fn a_drag_finds_the_boundary_above_it_and_falls_back_when_there_is_none() {
        let seen = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(DragBoundary::new(
            Rect::ltrb(0.0, 0.0, 100.0, 100.0),
            component(Reader {
                seen: Rc::clone(&seen),
            }),
        ));
        assert_eq!(
            *seen.borrow(),
            Some(Some(Rect::ltrb(0.0, 0.0, 100.0, 100.0)))
        );

        // Nothing above: unbounded, not absent. Code that drags wants to know
        // where it may go, not whether anybody set a boundary.
        let seen = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(component(Reader {
            seen: Rc::clone(&seen),
        }));
        assert_eq!(*seen.borrow(), Some(None));
    }
}
