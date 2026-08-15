// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! How big the view is, and what is covering it.
//!
//! Size and density, the reader's text scale and theme, and the insets that
//! say where the status bar, the gesture bar and the software keyboard are.
//! Upstream this is `widgets/media_query.dart`, and the reason it is an
//! inherited value rather than an argument is that almost everything depends
//! on some part of it -- a page's padding, whether a button is big enough to
//! hit, how tall a line of text is -- and none of that should have to be
//! threaded through every constructor between the root and the thing that
//! needs it.
//!
//! [`SafeArea`] is here too. Upstream it has its own file, but it is nothing
//! but a consumer of this data: pad by what the system covers, then tell the
//! subtree that it has been dealt with.
//!
//! # Three kinds of inset, and why
//!
//! * **`view_insets`** is what is covering the view and pushing content out of
//!   the way -- the software keyboard, essentially always, and only at the
//!   bottom. Something scrollable should shrink by this so its last item can
//!   still be scrolled to.
//! * **`view_padding`** is what the system draws over: the status bar, a
//!   notch, the gesture bar. It does not change when the keyboard opens.
//! * **`padding`** is `view_padding` minus `view_insets`, floored at zero --
//!   what is still covered by the system after the keyboard has taken its
//!   share. This is what [`SafeArea`] uses, and it is why a `SafeArea` at the
//!   bottom of the screen stops padding when the keyboard is up: the keyboard
//!   is already keeping content off the gesture bar.
//!
//! The subtraction happens in [`ViewMetrics::padding`], because upstream it
//! happens in `dart:ui` (`window.dart`) rather than in the widget layer.
//!
//! # What this is not, yet
//!
//! Upstream a widget that reads a `MediaQuery` is registered as a dependent of
//! it, and a change rebuilds exactly the dependents. [`provide`] has no
//! dependency tracking, so a change here rebuilds from the root -- which is
//! what a resize already did before this existed. See `PORTING_STATUS.md`.

use std::rc::Rc;

use crate::app::ViewMetrics;
use crate::framework::{AnyWidget, BuildContext, Component, component, provide};
use crate::platform::Brightness;
use crate::render::{EdgeInsets, Size};
use crate::widgets::Empty;

/// What the view is like, as the widgets below it see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaQueryData {
    /// The view's size in logical pixels.
    pub size: Size,
    /// Physical pixels per logical pixel.
    pub device_pixel_ratio: f32,
    /// What the system still covers, after the keyboard has taken its share.
    pub padding: EdgeInsets,
    /// What the system covers, ignoring the keyboard.
    pub view_padding: EdgeInsets,
    /// What is covering the view and displacing content -- the keyboard.
    pub view_insets: EdgeInsets,
    /// The reader's text size setting. 1.0 is unscaled.
    pub text_scale_factor: f32,
    /// Whether the platform is in light or dark mode.
    pub platform_brightness: Brightness,
}

impl Default for MediaQueryData {
    fn default() -> MediaQueryData {
        MediaQueryData {
            size: Size::ZERO,
            device_pixel_ratio: 1.0,
            padding: EdgeInsets::ZERO,
            view_padding: EdgeInsets::ZERO,
            view_insets: EdgeInsets::ZERO,
            text_scale_factor: 1.0,
            platform_brightness: Brightness::Light,
        }
    }
}

impl MediaQueryData {
    /// Reads it off the view, the way upstream's `MediaQueryData.fromView`
    /// does.
    ///
    /// The two settings that do not come from the view come from the platform:
    /// text scale and brightness are what `flutter/settings` last said.
    pub fn from_view(metrics: &ViewMetrics) -> MediaQueryData {
        MediaQueryData {
            size: metrics.logical_size(),
            device_pixel_ratio: metrics.device_pixel_ratio as f32,
            padding: metrics.padding(),
            view_padding: metrics.view_padding(),
            view_insets: metrics.view_insets(),
            text_scale_factor: crate::platform::text_scale_factor() as f32,
            platform_brightness: crate::platform::brightness(),
        }
    }

    /// The same data with the given sides' padding zeroed, for a subtree that
    /// has already dealt with them.
    ///
    /// `view_padding` shrinks by as much as `padding` did rather than going to
    /// zero, so a descendant that asks "how far down does the system bar
    /// reach" still gets the part the keyboard is responsible for. That
    /// asymmetry is upstream's, in `MediaQueryData.removePadding`.
    pub fn remove_padding(
        &self,
        left: bool,
        top: bool,
        right: bool,
        bottom: bool,
    ) -> MediaQueryData {
        if !(left || top || right || bottom) {
            return *self;
        }
        let removed = |remove: bool, padding: f32| if remove { 0.0 } else { padding };
        let kept = |remove: bool, view: f32, padding: f32| {
            if remove {
                (view - padding).max(0.0)
            } else {
                view
            }
        };
        MediaQueryData {
            padding: EdgeInsets {
                left: removed(left, self.padding.left),
                top: removed(top, self.padding.top),
                right: removed(right, self.padding.right),
                bottom: removed(bottom, self.padding.bottom),
            },
            view_padding: EdgeInsets {
                left: kept(left, self.view_padding.left, self.padding.left),
                top: kept(top, self.view_padding.top, self.padding.top),
                right: kept(right, self.view_padding.right, self.padding.right),
                bottom: kept(bottom, self.view_padding.bottom, self.padding.bottom),
            },
            ..*self
        }
    }
}

/// Publishes [`MediaQueryData`] to a subtree.
///
/// The root of a widget application is wrapped in one automatically -- see
/// `WidgetHost` -- so this is only needed to *change* what a subtree sees, the
/// way [`SafeArea`] does.
pub struct MediaQuery;

impl MediaQuery {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(data: MediaQueryData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }
}

/// What the nearest enclosing [`MediaQuery`] says.
///
/// Falls back to a default rather than panicking when there is none. Upstream
/// asserts instead (`debugCheckHasMediaQuery`), because there a
/// `MediaQuery`-less tree is a mistake in the app; here the root always has
/// one, so the fallback is only reachable from a test that mounted a widget on
/// its own.
pub fn media_query_of(context: &BuildContext) -> Rc<MediaQueryData> {
    context.inherited_or_default::<MediaQueryData>()
}

/// Insets its child by whatever the system is covering.
///
/// The status bar at the top, the gesture bar or navigation bar at the bottom,
/// a notch or a curved edge at the sides. The child is then told those sides
/// have been dealt with, so a `SafeArea` inside a `SafeArea` does not pad
/// twice.
///
/// ```ignore
/// component(SafeArea::new(component(Page)))
/// ```
pub struct SafeArea {
    child: std::cell::RefCell<Option<AnyWidget>>,
    left: bool,
    top: bool,
    right: bool,
    bottom: bool,
    minimum: EdgeInsets,
}

impl SafeArea {
    pub fn new(child: AnyWidget) -> SafeArea {
        SafeArea {
            child: std::cell::RefCell::new(Some(child)),
            left: true,
            top: true,
            right: true,
            bottom: true,
            minimum: EdgeInsets::ZERO,
        }
    }

    /// Which sides to avoid. A list that should scroll under the gesture bar
    /// but start below the status bar wants `top` only.
    pub fn with_sides(mut self, left: bool, top: bool, right: bool, bottom: bool) -> Self {
        self.left = left;
        self.top = top;
        self.right = right;
        self.bottom = bottom;
        self
    }

    /// Padding to apply even where the system covers nothing. The greater of
    /// the two wins, per side.
    pub fn with_minimum(mut self, minimum: EdgeInsets) -> Self {
        self.minimum = minimum;
        self
    }
}

impl Component for SafeArea {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let data = media_query_of(context);
        let padding = EdgeInsets {
            left: if self.left { data.padding.left } else { 0.0 }.max(self.minimum.left),
            top: if self.top { data.padding.top } else { 0.0 }.max(self.minimum.top),
            right: if self.right { data.padding.right } else { 0.0 }.max(self.minimum.right),
            bottom: if self.bottom { data.padding.bottom } else { 0.0 }.max(self.minimum.bottom),
        };
        let child = self.child.borrow_mut().take().unwrap_or_else(|| crate::framework::leaf(|| Empty));
        let inner = data.remove_padding(self.left, self.top, self.right, self.bottom);
        crate::framework::single(MediaQuery::new(inner, child), move |child| {
            Box::new(crate::render::RenderPadding::new(padding, child))
        })
    }
}

/// [`SafeArea`] as a widget, for the common case of avoiding every side.
pub fn safe_area(child: AnyWidget) -> AnyWidget {
    component(SafeArea::new(child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, leaf};
    use crate::render::RenderBox;
    use crate::widgets::{Constraints, SizedBox};

    fn metrics(padding_top: f64, inset_bottom: f64) -> ViewMetrics {
        ViewMetrics {
            device_pixel_ratio: 2.0,
            width: 800.0,
            height: 1600.0,
            padding_top,
            padding_right: 0.0,
            padding_bottom: 48.0,
            padding_left: 0.0,
            view_inset_top: 0.0,
            view_inset_right: 0.0,
            view_inset_bottom: inset_bottom,
            view_inset_left: 0.0,
        }
    }

    #[test]
    fn the_view_reports_logical_pixels_not_physical_ones() {
        let data = MediaQueryData::from_view(&metrics(84.0, 0.0));
        assert_eq!(data.size, Size::new(400.0, 800.0));
        assert_eq!(data.padding.top, 42.0);
        assert_eq!(data.device_pixel_ratio, 2.0);
    }

    #[test]
    fn the_keyboard_takes_the_bottom_padding_over() {
        // The gesture bar is 48 physical pixels; a keyboard 600 tall covers it
        // and then some, so nothing is left for the bottom padding to avoid --
        // the keyboard is already keeping content clear of it.
        let data = MediaQueryData::from_view(&metrics(84.0, 600.0));
        assert_eq!(data.view_padding.bottom, 24.0);
        assert_eq!(data.view_insets.bottom, 300.0);
        assert_eq!(data.padding.bottom, 0.0);
    }

    #[test]
    fn a_side_that_was_dealt_with_is_not_offered_again() {
        let data = MediaQueryData::from_view(&metrics(84.0, 0.0));
        let inner = data.remove_padding(false, true, false, false);
        assert_eq!(inner.padding.top, 0.0);
        assert_eq!(inner.padding.bottom, 24.0, "the other sides are untouched");
        // What the status bar covers is still discoverable, minus the part
        // that was consumed -- which here is all of it.
        assert_eq!(inner.view_padding.top, 0.0);
    }

    #[test]
    fn removing_nothing_changes_nothing() {
        let data = MediaQueryData::from_view(&metrics(84.0, 600.0));
        assert_eq!(data.remove_padding(false, false, false, false), data);
    }

    #[test]
    fn a_safe_area_pushes_its_child_below_the_status_bar() {
        let data = MediaQueryData::from_view(&metrics(84.0, 0.0));
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(
            data,
            component(SafeArea::new(leaf(|| SizedBox::new(100.0, 100.0)))),
        ));
        let mut root = tree.build_render_tree().expect("a tree was mounted");
        root.layout(Constraints::loose(400.0, 800.0));
        // 42 logical pixels of status bar at the top, 24 of gesture bar at the
        // bottom, around a child that asked for 100x100.
        assert_eq!(root.size().height, 100.0 + 42.0 + 24.0);
    }

    #[test]
    fn a_safe_area_inside_a_safe_area_does_not_pad_twice() {
        let data = MediaQueryData::from_view(&metrics(84.0, 0.0));
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(
            data,
            component(SafeArea::new(component(SafeArea::new(leaf(|| {
                SizedBox::new(100.0, 100.0)
            }))))),
        ));
        let mut root = tree.build_render_tree().expect("a tree was mounted");
        root.layout(Constraints::loose(400.0, 800.0));
        assert_eq!(root.size().height, 100.0 + 42.0 + 24.0);
    }

    #[test]
    fn a_minimum_applies_where_the_system_covers_nothing() {
        let data = MediaQueryData::default();
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(
            data,
            component(
                SafeArea::new(leaf(|| SizedBox::new(100.0, 100.0)))
                    .with_minimum(EdgeInsets::all(8.0)),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a tree was mounted");
        root.layout(Constraints::loose(400.0, 800.0));
        assert_eq!(root.size().height, 116.0);
    }
}
