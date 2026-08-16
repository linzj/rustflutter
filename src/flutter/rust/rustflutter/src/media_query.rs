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
//! # The reader's text size
//!
//! `text_scale_factor` is here rather than read straight off the platform so
//! that a subtree can have its own -- an icon font that should not grow, a
//! preview showing what another setting looks like. See
//! [`MediaQuery::no_text_scaling`] and [`current_text_scale`].

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

    /// The same data with a different text scale.
    ///
    /// Upstream's `copyWith(textScaler: ...)`. See [`MediaQuery::no_text_scaling`]
    /// for why a subtree would want one.
    pub fn with_text_scale(&self, factor: f32) -> MediaQueryData {
        MediaQueryData { text_scale_factor: factor, ..*self }
    }

    /// The same data with the text scale held inside a range.
    ///
    /// Upstream's `TextScaler.clamp`, reached through
    /// `MediaQuery.withClampedTextScaling`. The point is not to overrule the
    /// reader but to keep a layout that genuinely cannot survive 2.0 from
    /// breaking outright -- a smaller enlargement is still an enlargement.
    pub fn clamp_text_scale(&self, min: f32, max: f32) -> MediaQueryData {
        debug_assert!(max >= min, "a clamp with no room in it");
        MediaQueryData { text_scale_factor: self.text_scale_factor.clamp(min, max), ..*self }
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

    /// A subtree that does not scale its text.
    ///
    /// Upstream's `MediaQuery.withNoTextScaling`. It is for things whose size
    /// is not a reading size: an icon font, a logotype, a preview that is
    /// showing what some *other* setting looks like. Everything else should
    /// scale, which is why this is a deliberate opt-out rather than a default.
    pub fn no_text_scaling(child: AnyWidget) -> AnyWidget {
        component(RescaleText { child: std::cell::RefCell::new(Some(child)), min: 1.0, max: 1.0 })
    }

    /// A subtree whose text scale is held between `min` and `max`.
    ///
    /// Upstream's `MediaQuery.withClampedTextScaling`.
    pub fn clamped_text_scaling(min: f32, max: f32, child: AnyWidget) -> AnyWidget {
        component(RescaleText { child: std::cell::RefCell::new(Some(child)), min, max })
    }
}

/// Republishes the enclosing [`MediaQueryData`] with its text scale clamped.
///
/// A component rather than a plain `provide`, because it has to *read* the
/// enclosing data before it can change one field of it -- upstream reaches the
/// same way, through a `Builder`.
struct RescaleText {
    child: std::cell::RefCell<Option<AnyWidget>>,
    min: f32,
    max: f32,
}

impl Component for RescaleText {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let data = media_query_of(context);
        let child =
            self.child.borrow_mut().take().unwrap_or_else(|| crate::framework::leaf(|| Empty));
        MediaQuery::new(data.clamp_text_scale(self.min, self.max), child)
    }
}

// -- The scale in force while a subtree's render objects are being built ------
//
// Text is shaped at layout time, long after the walk that built the render
// tree has finished, so a paragraph cannot go looking for its `MediaQuery`
// when it needs the scale: it has to have been told. Upstream has the same
// split and answers it the same way -- `Text.build` reads
// `MediaQuery.textScalerOf(context)` and hands the result to `RenderParagraph`
// as a field, which passes it to `TextPainter` at layout.
//
// The equivalent of "reading it from the context" here is this: the render
// walk pushes the scale as it descends through a `MediaQuery`, and a paragraph
// constructed inside a build closure takes a copy. Which means the value is
// only meaningful *during* that walk -- hence a plain thread-local rather than
// anything the rest of the framework can see.

thread_local! {
    static TEXT_SCALE: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// The text scale a paragraph built right now should be shaped at.
///
/// Outside any [`MediaQuery`] -- a render object built on its own in a test,
/// say -- this is what the platform last said, which is what all text used
/// before a subtree could have its own. An accessibility setting the reader
/// has already asked every application for is the wrong thing to lose by
/// default.
pub fn current_text_scale() -> f32 {
    TEXT_SCALE.with(|scale| scale.get()).unwrap_or_else(|| crate::platform::text_scale_factor() as f32)
}

/// Runs `body` with `scale` as the ambient text scale, restoring whatever was
/// in force before. Called by the render walk; not public API.
pub(crate) fn with_text_scale<R>(scale: f32, body: impl FnOnce() -> R) -> R {
    let previous = TEXT_SCALE.with(|current| current.replace(Some(scale)));
    // The restore has to happen even if `body` unwinds, or one panicking
    // subtree would leave every later frame shaping at its scale.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
    TEXT_SCALE.with(|current| current.set(previous));
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
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
    use crate::framework::{ElementTree, leaf, many};
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

    // -- The reader's text size --------------------------------------------

    /// A leaf that records the text scale in force where it was built.
    ///
    /// Which is the whole question: a paragraph made inside this closure takes
    /// the same value, and shapes at it later.
    fn scale_probe(into: Rc<std::cell::Cell<f32>>) -> AnyWidget {
        leaf(move || {
            into.set(current_text_scale());
            SizedBox::new(1.0, 1.0)
        })
    }

    #[test]
    fn a_subtree_is_built_at_its_own_media_querys_text_scale() {
        let seen = Rc::new(std::cell::Cell::new(0.0));
        let data = MediaQueryData::from_view(&metrics(0.0, 0.0)).with_text_scale(1.5);
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(data, scale_probe(Rc::clone(&seen))));
        let _ = tree.build_render_tree();
        assert_eq!(seen.get(), 1.5);
    }

    #[test]
    fn a_nested_media_query_only_changes_its_own_subtree() {
        let outer = Rc::new(std::cell::Cell::new(0.0));
        let inner = Rc::new(std::cell::Cell::new(0.0));
        let after = Rc::new(std::cell::Cell::new(0.0));
        let data = MediaQueryData::from_view(&metrics(0.0, 0.0)).with_text_scale(1.5);

        let (o, i, a) = (Rc::clone(&outer), Rc::clone(&inner), Rc::clone(&after));
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(
            data,
            many(
                vec![
                    scale_probe(o),
                    MediaQuery::new(data.with_text_scale(3.0), scale_probe(i)),
                    // Built after the nested one, so it is what catches a
                    // scale that was pushed and never popped.
                    scale_probe(a),
                ],
                |children| {
                    let mut flex = crate::render::RenderFlex::column();
                    for child in children {
                        flex = flex.push(child);
                    }
                    Box::new(flex)
                },
            ),
        ));
        let _ = tree.build_render_tree();
        assert_eq!(outer.get(), 1.5);
        assert_eq!(inner.get(), 3.0);
        assert_eq!(after.get(), 1.5, "the inner scale leaked out of its subtree");
    }

    #[test]
    fn a_subtree_can_opt_out_of_scaling_altogether() {
        // An icon font, a logotype: things whose size is not a reading size.
        let seen = Rc::new(std::cell::Cell::new(0.0));
        let data = MediaQueryData::from_view(&metrics(0.0, 0.0)).with_text_scale(2.0);
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(
            data,
            MediaQuery::no_text_scaling(scale_probe(Rc::clone(&seen))),
        ));
        let _ = tree.build_render_tree();
        assert_eq!(seen.get(), 1.0);
    }

    #[test]
    fn a_subtree_can_hold_the_scale_inside_a_range() {
        let seen = Rc::new(std::cell::Cell::new(0.0));
        let data = MediaQueryData::from_view(&metrics(0.0, 0.0)).with_text_scale(2.5);
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(
            data,
            MediaQuery::clamped_text_scaling(1.0, 1.6, scale_probe(Rc::clone(&seen))),
        ));
        let _ = tree.build_render_tree();
        // Clamped, not overruled: a smaller enlargement is still an
        // enlargement, which is the difference between this and opting out.
        assert_eq!(seen.get(), 1.6);
    }

    #[test]
    fn a_paragraph_takes_the_scale_where_it_was_built() {
        let outside = crate::render::RenderParagraph::new("plain");
        assert_eq!(outside.text_scale(), 1.0, "no media query, no platform scale set");

        let inside = with_text_scale(1.75, || crate::render::RenderParagraph::new("bigger"));
        assert_eq!(inside.text_scale(), 1.75);
        // And the scale is not still in force afterwards.
        assert_eq!(crate::render::RenderParagraph::new("after").text_scale(), 1.0);
    }

    #[test]
    fn without_a_media_query_the_platform_setting_still_applies() {
        // Every application asked the reader for this setting; a render object
        // built outside a tree is not a reason to throw the answer away.
        crate::platform::set_user_settings(r#"{"textScaleFactor":1.3}"#);
        assert_eq!(current_text_scale(), 1.3);
        assert_eq!(crate::render::RenderParagraph::new("x").text_scale(), 1.3);
        crate::platform::reset();
    }
}
