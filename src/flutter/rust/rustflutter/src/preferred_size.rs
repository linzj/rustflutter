// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Two small widgets that say something about their own size (upstream
//! `widgets/preferred_size.dart`, `widgets/size_changed_layout_notifier.dart`).
//!
//! Neither draws anything. One answers a question about its size before it is
//! laid out, and the other says when its size has changed after it has been.
//!
//! # Recorded divergences
//!
//! * Upstream's `PreferredSizeWidget` is an interface a widget class
//!   implements, and a caller receives it as a `Widget` that happens to also
//!   be one. This crate's widgets are erased into
//!   [`AnyWidget`](crate::framework::AnyWidget) with no room for a second
//!   interface, so [`PreferredSize`] is a pair -- the size and the widget --
//!   and [`PreferredSizeWidget`] is the trait something that makes such a pair
//!   implements.

use std::cell::Cell;

use crate::framework::{AnyWidget, Notification, single};
use crate::render::{
    BoxConstraints, BoxedRender, HitTestResult, Offset, PaintContext, RenderBox, RenderRef, Size,
    UpdateEffect,
};

/// Upstream `PreferredSizeWidget`: a widget that knows how big it would like
/// to be before anybody measures it.
///
/// The question exists because an app bar has to be sized by the scaffold
/// above it, which lays out before the bar does. Nothing can measure the bar
/// at that point, so the bar is asked instead.
pub trait PreferredSizeWidget {
    /// Upstream `preferredSize`. Either dimension may be
    /// [`f32::INFINITY`](f32::INFINITY), which upstream reads as "no
    /// preference on this axis" and is why the answer is a `Size` rather than
    /// two numbers.
    fn preferred_size(&self) -> Size;

    /// The widget itself.
    fn build(self) -> AnyWidget;
}

/// Upstream `PreferredSize`: a child, plus the size to tell whoever asks.
///
/// It does not enforce the size and upstream is explicit that it does not:
/// the child is laid out with whatever constraints arrive, and the preferred
/// size is only what the *parent* was told when it was deciding how much room
/// to leave. A child that then takes more overflows, exactly as it would
/// without this.
pub struct PreferredSize {
    size: Size,
    child: AnyWidget,
}

impl PreferredSize {
    pub fn new(size: Size, child: AnyWidget) -> PreferredSize {
        PreferredSize { size, child }
    }
}

impl PreferredSizeWidget for PreferredSize {
    fn preferred_size(&self) -> Size {
        self.size
    }

    /// Upstream's `build`, which returns the child untouched: this widget is
    /// the answer to a question and nothing else.
    fn build(self) -> AnyWidget {
        self.child
    }
}

/// Upstream `SizeChangedLayoutNotification`: sent when something under a
/// [`size_changed_layout_notifier`] was laid out at a new size.
///
/// Upstream's extends `LayoutChangedNotification`, whose whole documented
/// point is that it arrives *during* layout and so must not itself cause
/// layout -- a listener that resized in response would loop. What it is for
/// is repainting: a decoration that has to follow the size of something it
/// does not lay out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SizeChangedLayoutNotification;

impl Notification for SizeChangedLayoutNotification {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Upstream `SizeChangedLayoutNotifier`: dispatches a
/// [`SizeChangedLayoutNotification`] when its child's size changes.
pub struct RenderSizeChangedWithCallback {
    child: BoxedRender,
    size: Size,
    /// Upstream's `_oldSize`, and the reason it is nullable: the first layout
    /// is not a change.
    old_size: Cell<Option<Size>>,
    on_layout_changed: std::rc::Rc<dyn Fn()>,
}

impl RenderSizeChangedWithCallback {
    pub fn new(
        child: impl RenderBox + 'static,
        on_layout_changed: std::rc::Rc<dyn Fn()>,
    ) -> RenderSizeChangedWithCallback {
        RenderSizeChangedWithCallback {
            child: RenderRef::new(child),
            size: Size::ZERO,
            old_size: Cell::new(None),
            on_layout_changed,
        }
    }
}

impl RenderBox for RenderSizeChangedWithCallback {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh
            .as_any_mut()
            .downcast_mut::<RenderSizeChangedWithCallback>()?;
        // The callback is captured against one context and upstream says so
        // explicitly: there is a one-to-one relationship between this object
        // and the context, so the callback never needs replacing.
        let effect = UpdateEffect::relayout_if(!self.child.is(&fresh.child));
        self.child = fresh.child.clone();
        Some(effect)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout_child(constraints, true);
        // Upstream's own comment: the *first* layout is not a change, and
        // sending one then would be "SizeObserver all over again" -- every
        // notifier in the tree firing on the first frame, when nothing has
        // changed yet.
        let old_size = self.old_size.replace(Some(self.size));
        if let Some(old_size) = old_size {
            if old_size != self.size {
                (self.on_layout_changed)();
            }
        }
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.child.dry_layout(constraints)
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
    }

    /// Upstream's `_RenderSizeChangedWithCallback` is a `RenderProxyBox`, and
    /// a proxy forwards its hit test. Saying nothing here would mean anything
    /// wrapped in one could be seen and not touched.
    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.child.hit_test(position, result)
    }
}

/// Upstream `SizeChangedLayoutNotifier`.
///
/// The notification is dispatched from the sink the build gave it, because a
/// render object has no context of its own -- upstream captures the context
/// in the closure it hands the render object, which is the same arrangement.
pub fn size_changed_layout_notifier(
    sink: crate::framework::NotificationSink,
    child: AnyWidget,
) -> AnyWidget {
    single(child, move |child| {
        let sink = sink.clone();
        RenderSizeChangedWithCallback::new(
            child,
            std::rc::Rc::new(move || sink.dispatch(&SizeChangedLayoutNotification)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{
        BuildContext, Component, ElementTree, component, leaf, notification_listener,
    };
    use crate::widgets::SizedBox;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn a_preferred_size_answers_before_anything_measures_it() {
        // The question exists because a scaffold sizes its app bar before the
        // bar is laid out. Nothing can measure it at that point, so the bar
        // is asked.
        let bar = PreferredSize::new(
            Size::new(f32::INFINITY, 56.0),
            leaf(|| SizedBox::new(10.0, 10.0)),
        );
        assert_eq!(bar.preferred_size(), Size::new(f32::INFINITY, 56.0));
    }

    #[test]
    fn an_infinite_dimension_means_no_preference_on_that_axis() {
        // Which is why the answer is a `Size` and not two numbers: an app bar
        // has a height it wants and no opinion about width.
        let bar = PreferredSize::new(
            Size::new(f32::INFINITY, 56.0),
            leaf(|| SizedBox::new(1.0, 1.0)),
        );
        assert!(bar.preferred_size().width.is_infinite());
        assert_eq!(bar.preferred_size().height, 56.0);
    }

    #[test]
    fn the_preferred_size_does_not_constrain_the_child() {
        // Upstream is explicit that it does not enforce anything: the child
        // gets whatever constraints arrive, and the preferred size is only
        // what the parent was told while deciding how much room to leave. A
        // child that then takes more overflows, exactly as it would without
        // this widget.
        let mut tree = ElementTree::new();
        tree.rebuild(
            PreferredSize::new(Size::new(50.0, 50.0), leaf(|| SizedBox::new(200.0, 30.0))).build(),
        );
        let laid_out = tree
            .build_render_tree()
            .expect("a root")
            .layout(BoxConstraints::loose(1000.0, 1000.0));
        assert_eq!(
            laid_out,
            Size::new(200.0, 30.0),
            "the child's own size, not the preferred one"
        );
    }

    /// A component whose child's size it can change between builds.
    struct Resizing {
        width: Rc<Cell<f32>>,
        seen: Rc<RefCell<usize>>,
    }

    impl Component for Resizing {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let width = Rc::clone(&self.width);
            let seen = Rc::clone(&self.seen);
            notification_listener(
                move |_: &SizeChangedLayoutNotification| {
                    *seen.borrow_mut() += 1;
                    true
                },
                size_changed_layout_notifier(
                    context.notification_sink(),
                    leaf(move || SizedBox::new(width.get(), 10.0)),
                ),
            )
        }
    }

    #[test]
    fn the_first_layout_is_not_a_change() {
        // Upstream's own comment: sending one then would be "SizeObserver all
        // over again" -- every notifier in the tree firing on the first
        // frame, when nothing has changed yet.
        let width = Rc::new(Cell::new(100.0));
        let seen = Rc::new(RefCell::new(0));
        let mut tree = ElementTree::new();
        tree.rebuild(component(Resizing {
            width: Rc::clone(&width),
            seen: Rc::clone(&seen),
        }));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(1000.0, 1000.0));
        assert_eq!(*seen.borrow(), 0);
    }

    #[test]
    fn a_second_layout_at_the_same_size_is_not_a_change_either() {
        // A relayout that changed nothing should not wake anything up: this
        // notification is for repainting something that follows a size, and
        // repainting it for no reason is the cost the check avoids.
        let width = Rc::new(Cell::new(100.0));
        let seen = Rc::new(RefCell::new(0));
        let mut tree = ElementTree::new();
        tree.rebuild(component(Resizing {
            width: Rc::clone(&width),
            seen: Rc::clone(&seen),
        }));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(1000.0, 1000.0));
        root.layout(BoxConstraints::loose(1000.0, 1000.0));
        assert_eq!(*seen.borrow(), 0);
    }

    #[test]
    fn a_layout_at_a_new_size_calls_back_once() {
        // Driven against the render object directly: what this module does
        // is notice a size change, and the notification plumbing above it is
        // the framework's and has its own tests.
        let calls = Rc::new(Cell::new(0));
        let counted = Rc::clone(&calls);
        let mut notifier = RenderSizeChangedWithCallback::new(
            SizedBox::new(100.0, 10.0),
            Rc::new(move || counted.set(counted.get() + 1)),
        );

        notifier.layout(BoxConstraints::loose(1000.0, 1000.0));
        assert_eq!(calls.get(), 0, "the first layout is not a change");

        // Tight constraints the child has to obey, so its size changes.
        assert_eq!(
            notifier.layout(BoxConstraints::tight(40.0, 10.0)),
            Size::new(40.0, 10.0)
        );
        assert_eq!(calls.get(), 1);

        // Staying there is not another change.
        notifier.layout(BoxConstraints::tight(40.0, 10.0));
        assert_eq!(calls.get(), 1);

        // And going back is.
        notifier.layout(BoxConstraints::loose(1000.0, 1000.0));
        assert_eq!(calls.get(), 2);
    }
}
