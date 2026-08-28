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
use crate::framework::{
    AnyWidget, BuildContext, Component, DependentNotify, component, provide_model,
};
use crate::platform::Brightness;
use crate::presence::Orientation;
use crate::render::{EdgeInsets, Size};
use crate::widgets::Empty;

/// The parts of [`MediaQueryData`] a reader can depend on separately.
///
/// Upstream's `_MediaQueryAspect`, an enum the same size: a reader that
/// subscribes to the padding alone is not rebuilt because the view got taller,
/// and one that subscribes to the size is not rebuilt because the keyboard
/// opened.
mod aspect {
    pub(super) const SIZE: &str = "size";
    pub(super) const PADDING: &str = "padding";
    pub(super) const VIEW_INSETS: &str = "view_insets";
    pub(super) const TEXT_SCALE: &str = "text_scale";
    pub(super) const ALWAYS_USE_24_HOUR_FORMAT: &str = "always_use_24_hour_format";
    pub(super) const ORIENTATION: &str = "orientation";
}

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
    /// Upstream's `MediaQueryData.onOffSwitchLabels`: whether switches draw
    /// the I and O marks beside the thumb.
    ///
    /// An iOS accessibility setting, off by default, and **the platform bridge
    /// cannot report it yet** -- there is a
    /// `Binding::did_change_accessibility_features` hook and nothing behind
    /// it. It is carried here anyway because a `MediaQuery` is a widget: an
    /// application that knows the setting, or a test that wants the marks, can
    /// put one in the tree and everything below sees it. That is a different
    /// thing from a flag nobody can move.
    pub on_off_switch_labels: bool,
    /// Upstream's `MediaQueryData.alwaysUse24HourFormat`: whether a time is
    /// written 13:00 rather than 1:00 PM.
    ///
    /// Unlike [`on_off_switch_labels`](MediaQueryData::on_off_switch_labels)
    /// the platform **does** report this one, and has all along: it arrives on
    /// `flutter/settings` beside the text scale and the brightness, and
    /// [`crate::platform::UserSettings`] has stored it since that channel was
    /// written. What was missing was the last hop -- nothing carried it from
    /// there to a widget, so a dialog that wanted it had to be told.
    pub always_use_24_hour_format: bool,
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
            on_off_switch_labels: false,
            always_use_24_hour_format: false,
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
            // Not from the view and not from the platform either -- see the
            // field. The bridge has nowhere to read it from, so a view's data
            // says no and a `MediaQuery` above can say otherwise.
            on_off_switch_labels: false,
            // From the platform, like the two above it. Upstream reads it off
            // `platformDispatcher.alwaysUse24HourFormat` in the same
            // constructor.
            always_use_24_hour_format: crate::platform::user_settings().always_use_24_hour_format,
        }
    }

    /// Upstream's `MediaQueryData.orientation`, which is **derived and not
    /// stored**: a view is landscape when it is wider than it is tall.
    ///
    /// A square view is portrait, because the comparison is strict. That is
    /// upstream's and it matters more than it looks: a window dragged through
    /// square would otherwise flicker between the two answers on the frames
    /// where the two numbers are equal.
    ///
    /// It was being derived privately in two other files before it was here --
    /// `pickers.rs` for the time picker's portrait and landscape layouts, and
    /// `presence.rs` from constraints rather than from the view. The one in
    /// `presence.rs` is a different question (it asks about the box a widget
    /// was given, which is upstream's `OrientationBuilder`) and stays; this is
    /// the one that asks about the view.
    pub fn orientation(&self) -> Orientation {
        if self.size.width > self.size.height {
            Orientation::Landscape
        } else {
            Orientation::Portrait
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

    /// The same data with the given sides' *view insets* zeroed, for a subtree
    /// that has already made room for them.
    ///
    /// Upstream's `MediaQueryData.removeViewInsets`, and `Scaffold` is what
    /// calls it: a scaffold that has shrunk its body to sit above the keyboard
    /// has dealt with the keyboard, and a body still told the keyboard is
    /// there would make room for it twice.
    ///
    /// `view_padding` shrinks by as much as the inset did rather than going to
    /// zero, the mirror of [`MediaQueryData::remove_padding`] and upstream's
    /// same asymmetry.
    pub fn remove_view_insets(
        &self,
        left: bool,
        top: bool,
        right: bool,
        bottom: bool,
    ) -> MediaQueryData {
        if !(left || top || right || bottom) {
            return *self;
        }
        let removed = |remove: bool, inset: f32| if remove { 0.0 } else { inset };
        let kept = |remove: bool, view: f32, inset: f32| {
            if remove {
                (view - inset).max(0.0)
            } else {
                view
            }
        };
        MediaQueryData {
            view_insets: EdgeInsets {
                left: removed(left, self.view_insets.left),
                top: removed(top, self.view_insets.top),
                right: removed(right, self.view_insets.right),
                bottom: removed(bottom, self.view_insets.bottom),
            },
            view_padding: EdgeInsets {
                left: kept(left, self.view_padding.left, self.view_insets.left),
                top: kept(top, self.view_padding.top, self.view_insets.top),
                right: kept(right, self.view_padding.right, self.view_insets.right),
                bottom: kept(bottom, self.view_padding.bottom, self.view_insets.bottom),
            },
            ..*self
        }
    }

    /// The same data with a different text scale.
    ///
    /// Upstream's `copyWith(textScaler: ...)`. See [`MediaQuery::no_text_scaling`]
    /// for why a subtree would want one.
    pub fn with_text_scale(&self, factor: f32) -> MediaQueryData {
        MediaQueryData {
            text_scale_factor: factor,
            ..*self
        }
    }

    /// The same data with the text scale held inside a range.
    ///
    /// Upstream's `TextScaler.clamp`, reached through
    /// `MediaQuery.withClampedTextScaling`. The point is not to overrule the
    /// reader but to keep a layout that genuinely cannot survive 2.0 from
    /// breaking outright -- a smaller enlargement is still an enlargement.
    pub fn clamp_text_scale(&self, min: f32, max: f32) -> MediaQueryData {
        debug_assert!(max >= min, "a clamp with no room in it");
        MediaQueryData {
            text_scale_factor: self.text_scale_factor.clamp(min, max),
            ..*self
        }
    }
}

impl DependentNotify for MediaQueryData {
    /// Which parts of the data differ, asked for one aspect at a time.
    ///
    /// Upstream's `MediaQuery.updateShouldNotifyDependent`. An aspect this
    /// data cannot speak for counts as changed: better that a reader hears
    /// too much than that it silently never hears.
    fn is_aspect_stale(old: &MediaQueryData, new: &MediaQueryData, aspect: &str) -> bool {
        match aspect {
            aspect::SIZE => old.size != new.size,
            aspect::PADDING => old.padding != new.padding,
            aspect::VIEW_INSETS => old.view_insets != new.view_insets,
            aspect::TEXT_SCALE => old.text_scale_factor != new.text_scale_factor,
            aspect::ALWAYS_USE_24_HOUR_FORMAT => {
                old.always_use_24_hour_format != new.always_use_24_hour_format
            }
            // Derived, so the comparison is on the answer and not on the
            // field: a view that changes size without crossing square has not
            // changed orientation, and a reader that asked only about
            // orientation should not hear about it.
            aspect::ORIENTATION => old.orientation() != new.orientation(),
            _ => true,
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
        // A model rather than a plain provide, upstream's `MediaQuery extends
        // InheritedModel<_MediaQueryAspect>`: readers can name the part they
        // read, and are rebuilt only when that part changes.
        provide_model(data, child)
    }

    /// A subtree that does not scale its text.
    ///
    /// Upstream's `MediaQuery.withNoTextScaling`. It is for things whose size
    /// is not a reading size: an icon font, a logotype, a preview that is
    /// showing what some *other* setting looks like. Everything else should
    /// scale, which is why this is a deliberate opt-out rather than a default.
    pub fn no_text_scaling(child: AnyWidget) -> AnyWidget {
        component(RescaleText {
            child: std::cell::RefCell::new(Some(child)),
            min: 1.0,
            max: 1.0,
        })
    }

    /// A subtree whose text scale is held between `min` and `max`.
    ///
    /// Upstream's `MediaQuery.withClampedTextScaling`.
    pub fn clamped_text_scaling(min: f32, max: f32, child: AnyWidget) -> AnyWidget {
        component(RescaleText {
            child: std::cell::RefCell::new(Some(child)),
            min,
            max,
        })
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
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| crate::framework::leaf(|| Empty));
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
    static VIEW_INSETS: std::cell::Cell<EdgeInsets> = const { std::cell::Cell::new(EdgeInsets::ZERO) };
}

/// What is covering the view right now, as the platform last said -- the
/// keyboard, and essentially only it.
///
/// Upstream's `View.of(context).viewInsets`, and it exists for upstream's
/// reason. `EditableText.didChangeMetrics` reads the raw `FlutterView` rather
/// than the `MediaQuery`, deliberately: a `Scaffold` that has made room for
/// the keyboard **removes the inset from the `MediaQuery` it gives its body**,
/// so a field asking the context how far up the keyboard reaches is told
/// nothing -- by the very widget that dealt with it. The field still has to
/// know, because it is the one that has to get out of the way.
///
/// Not a dependency, because there is nothing to depend on: it is the
/// platform's own number, not an inherited value. Upstream is in the same
/// position and answers it the same way, with a `WidgetsBindingObserver` for
/// the notification rather than an inherited widget.
pub fn current_view_insets() -> EdgeInsets {
    VIEW_INSETS.with(|insets| insets.get())
}

/// Records what the platform last said. Called once a frame by the binding,
/// before anything is built; not public API.
pub(crate) fn set_current_view_insets(insets: EdgeInsets) {
    VIEW_INSETS.with(|slot| slot.set(insets));
}

/// The text scale a paragraph built right now should be shaped at.
///
/// Outside any [`MediaQuery`] -- a render object built on its own in a test,
/// say -- this is what the platform last said, which is what all text used
/// before a subtree could have its own. An accessibility setting the reader
/// has already asked every application for is the wrong thing to lose by
/// default.
pub fn current_text_scale() -> f32 {
    TEXT_SCALE
        .with(|scale| scale.get())
        .unwrap_or_else(|| crate::platform::text_scale_factor() as f32)
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

/// The nearest [`MediaQuery`]'s size, and a dependence on nothing else about
/// it.
///
/// Upstream's `MediaQuery.sizeOf`: a reader that asks for the size is not
/// rebuilt when the keyboard opens -- that is `view_insets`, and somebody
/// else's news.
pub fn size_of(context: &BuildContext) -> Size {
    context
        .inherited_aspect_or_default::<MediaQueryData>(aspect::SIZE)
        .size
}

/// The nearest [`MediaQuery`]'s padding, and a dependence on nothing else
/// about it. Upstream's `MediaQuery.paddingOf`.
pub fn padding_of(context: &BuildContext) -> EdgeInsets {
    context
        .inherited_aspect_or_default::<MediaQueryData>(aspect::PADDING)
        .padding
}

/// The nearest [`MediaQuery`]'s view insets -- the keyboard -- and a
/// dependence on nothing else about it. Upstream's `MediaQuery.viewInsetsOf`.
pub fn view_insets_of(context: &BuildContext) -> EdgeInsets {
    context
        .inherited_aspect_or_default::<MediaQueryData>(aspect::VIEW_INSETS)
        .view_insets
}

/// The nearest [`MediaQuery`]'s text scale, and a dependence on nothing else
/// about it. Upstream's `MediaQuery.textScalerOf`.
pub fn text_scale_of(context: &BuildContext) -> f32 {
    context
        .inherited_aspect_or_default::<MediaQueryData>(aspect::TEXT_SCALE)
        .text_scale_factor
}

/// Whether the reader's platform writes times as 13:00 rather than 1:00 PM,
/// and a dependence on nothing else about the view.
///
/// Upstream's `MediaQuery.alwaysUse24HourFormatOf`.
pub fn always_use_24_hour_format_of(context: &BuildContext) -> bool {
    context
        .inherited_aspect_or_default::<MediaQueryData>(aspect::ALWAYS_USE_24_HOUR_FORMAT)
        .always_use_24_hour_format
}

/// Whether the view is wider than it is tall, and a dependence on nothing else
/// about it. Upstream's `MediaQuery.orientationOf`.
///
/// This is not the same question as [`crate::presence::OrientationBuilder`],
/// which asks about the box a widget was handed. A widget in a narrow column
/// of a landscape window gets `Landscape` from this and `Portrait` from that,
/// and each is the right answer to its own question.
pub fn orientation_of(context: &BuildContext) -> Orientation {
    context
        .inherited_aspect_or_default::<MediaQueryData>(aspect::ORIENTATION)
        .orientation()
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
            bottom: if self.bottom {
                data.padding.bottom
            } else {
                0.0
            }
            .max(self.minimum.bottom),
        };
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| crate::framework::leaf(|| Empty));
        let inner = data.remove_padding(self.left, self.top, self.right, self.bottom);
        crate::framework::single(MediaQuery::new(inner, child), move |child| {
            Box::new(crate::render::RenderPadding::new(padding, child))
        })
    }
}

/// Upstream `SliverSafeArea`: [`SafeArea`] for a sliver.
///
/// The same two ideas, and both are worth naming because neither is the obvious
/// one.
///
/// **`minimum` is a floor, not an addition.** The greater of the minimum and
/// the system's own inset wins, per side -- so `minimum: all(16)` gives at
/// least sixteen everywhere and more where the notch demands it, rather than
/// sixteen plus the notch.
///
/// **The sides it consumed are zeroed out of the `MediaQuery` it passes down**,
/// so a safe area inside a safe area does not inset twice. Compare
/// [`crate::scroll_view::BoxScrollView`], which *splits* the padding by axis
/// rather than removing all of it: a list needs its children to keep the
/// cross-axis half, and this does not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliverSafeArea {
    pub left: bool,
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub minimum: EdgeInsets,
}

impl SliverSafeArea {
    pub fn new() -> SliverSafeArea {
        SliverSafeArea {
            left: true,
            top: true,
            right: true,
            bottom: true,
            minimum: EdgeInsets::ZERO,
        }
    }

    pub fn with_sides(mut self, left: bool, top: bool, right: bool, bottom: bool) -> Self {
        self.left = left;
        self.top = top;
        self.right = right;
        self.bottom = bottom;
        self
    }

    pub fn with_minimum(mut self, minimum: EdgeInsets) -> Self {
        self.minimum = minimum;
        self
    }

    /// The padding this sliver applies.
    pub fn resolve_padding(&self, ambient: EdgeInsets) -> EdgeInsets {
        EdgeInsets {
            left: if self.left { ambient.left } else { 0.0 }.max(self.minimum.left),
            top: if self.top { ambient.top } else { 0.0 }.max(self.minimum.top),
            right: if self.right { ambient.right } else { 0.0 }.max(self.minimum.right),
            bottom: if self.bottom { ambient.bottom } else { 0.0 }.max(self.minimum.bottom),
        }
    }

    /// What the sliver below it sees.
    pub fn inner_padding(&self, ambient: EdgeInsets) -> EdgeInsets {
        EdgeInsets {
            left: if self.left { 0.0 } else { ambient.left },
            top: if self.top { 0.0 } else { ambient.top },
            right: if self.right { 0.0 } else { ambient.right },
            bottom: if self.bottom { 0.0 } else { ambient.bottom },
        }
    }
}

impl Default for SliverSafeArea {
    fn default() -> Self {
        SliverSafeArea::new()
    }
}

/// [`SafeArea`] as a widget, for the common case of avoiding every side.
pub fn safe_area(child: AnyWidget) -> AnyWidget {
    component(SafeArea::new(child))
}

/// Upstream `SystemTextScaler`, the `TextScaler` a `MediaQuery` carries when
/// the scaling came from the platform.
///
/// [`crate::painting::TextScaler`]'s own documentation anticipated this one:
/// the linear spelling is the only one the engine's bare factor can express,
/// and a non-linear platform scaler arrives as something else. This is that
/// something else.
///
/// **The reason it cannot be a number is that newer Android does not scale
/// linearly**: at a large accessibility setting, small text is enlarged
/// considerably and text that is already large is enlarged much less, so the
/// layout does not fall apart. No single multiplier describes that, so the
/// scaling has to be a call into the platform.
///
/// `text_scale_factor` survives anyway, and upstream says exactly what it is
/// for: **comparing two scalers, and not arithmetic.** Two system scalers with
/// the same factor produce the same output for the same input, so the factor is
/// a sound identity -- but multiplying a font size by it would be inventing a
/// linear model the platform never agreed to.
#[derive(Clone, Copy, Debug)]
pub struct SystemTextScaler {
    text_scale_factor: f32,
    /// The platform's answer, as a function this port can be handed for a test.
    scale_fn: fn(f32) -> f32,
}

impl SystemTextScaler {
    pub fn new(text_scale_factor: f32, scale_fn: fn(f32) -> f32) -> SystemTextScaler {
        SystemTextScaler {
            text_scale_factor,
            scale_fn,
        }
    }

    /// For comparison only.
    pub fn text_scale_factor(&self) -> f32 {
        self.text_scale_factor
    }

    /// Upstream `scale`, which asks the platform dispatcher rather than
    /// multiplying.
    pub fn scale(&self, font_size: f32) -> f32 {
        (self.scale_fn)(font_size)
    }

    /// Upstream's `==`, which has a case worth keeping: a system scaler whose
    /// factor is exactly 1.0 **equals `TextScaler.noScaling`**, because the two
    /// are extensionally the same function. A widget comparing against "no
    /// scaling" to decide whether it can take a shortcut gets the right answer
    /// without knowing where the scaler came from.
    pub fn equals_no_scaling(&self) -> bool {
        self.text_scale_factor == 1.0
    }

    /// Upstream compares two `SystemTextScaler`s by their factors alone.
    pub fn same_as(&self, other: &SystemTextScaler) -> bool {
        self.text_scale_factor == other.text_scale_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream's `removeViewInsets`, which a `Scaffold` that made room for
    /// the keyboard applies to its body.
    #[test]
    fn removing_a_view_inset_takes_the_same_amount_off_the_view_padding() {
        let data = MediaQueryData {
            view_insets: EdgeInsets::only(0.0, 0.0, 0.0, 300.0),
            view_padding: EdgeInsets::only(0.0, 24.0, 0.0, 340.0),
            padding: EdgeInsets::only(0.0, 24.0, 0.0, 40.0),
            ..MediaQueryData::default()
        };
        let body = data.remove_view_insets(false, false, false, true);

        assert_eq!(body.view_insets.bottom, 0.0, "dealt with");
        assert_eq!(
            body.view_padding.bottom, 40.0,
            "the view padding loses exactly the inset, not everything -- what              the system still covers underneath the keyboard is a different              fact, and the asymmetry is upstream's"
        );
        assert_eq!(body.padding.bottom, 40.0, "untouched");
    }

    #[test]
    fn removing_nothing_is_the_same_data() {
        let data = MediaQueryData {
            view_insets: EdgeInsets::only(0.0, 0.0, 0.0, 300.0),
            ..MediaQueryData::default()
        };
        assert_eq!(data.remove_view_insets(false, false, false, false), data);
    }

    #[test]
    fn the_raw_view_insets_are_what_the_platform_last_said() {
        // Not the `MediaQuery`'s: a scaffold strips the inset from the data it
        // hands its body, and the field that has to get out of the keyboard's
        // way still has to know. Upstream reads `View.of(context).viewInsets`
        // for this and not `MediaQuery.of`.
        set_current_view_insets(EdgeInsets::only(0.0, 0.0, 0.0, 300.0));
        assert_eq!(current_view_insets().bottom, 300.0);
        set_current_view_insets(EdgeInsets::ZERO);
        assert_eq!(current_view_insets().bottom, 0.0);
    }

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

    /// The settings message the embedder writes, with the 24-hour flag set
    /// either way.
    ///
    /// Platform settings are thread-local and libtest gives each test its own
    /// thread, so this does not leak between tests -- but a test that sends it
    /// puts it back anyway, because `--test-threads=1` is a thing somebody
    /// runs when a test is flaky and that is the worst moment to find out.
    fn say_24_hour(flag: bool) {
        crate::platform::set_user_settings(&format!(
            r#"{{"textScaleFactor":1.0,"alwaysUse24HourFormat":{flag},"platformBrightness":"light"}}"#
        ));
    }

    #[test]
    fn the_platform_has_been_reporting_the_clock_all_along() {
        // The whole hop, end to end: the embedder writes it on
        // `flutter/settings`, `UserSettings` stores it, and `from_view` is what
        // was missing -- so before this the setting arrived and stopped there.
        say_24_hour(true);
        assert!(MediaQueryData::from_view(&metrics(0.0, 0.0)).always_use_24_hour_format);
        say_24_hour(false);
        assert!(!MediaQueryData::from_view(&metrics(0.0, 0.0)).always_use_24_hour_format);
    }

    #[test]
    fn a_square_view_is_portrait() {
        // Upstream compares strictly, and the boundary is the point: a window
        // dragged through square would otherwise have a frame where the two
        // numbers are equal and the answer could go either way.
        let square = MediaQueryData {
            size: Size::new(500.0, 500.0),
            ..MediaQueryData::default()
        };
        assert_eq!(square.orientation(), Orientation::Portrait);

        let wider = MediaQueryData {
            size: Size::new(500.1, 500.0),
            ..square
        };
        assert_eq!(wider.orientation(), Orientation::Landscape);

        let taller = MediaQueryData {
            size: Size::new(500.0, 500.1),
            ..square
        };
        assert_eq!(taller.orientation(), Orientation::Portrait);
    }

    #[test]
    fn a_reader_of_the_orientation_hears_about_turning_and_not_about_growing() {
        // The reason `orientation` is an aspect rather than something derived
        // at the call site: derived from `size`, every resize is news. Asked as
        // an aspect, only a resize that crosses square is.
        let portrait = MediaQueryData {
            size: Size::new(400.0, 800.0),
            ..MediaQueryData::default()
        };
        let taller = MediaQueryData {
            size: Size::new(400.0, 900.0),
            ..portrait
        };
        let turned = MediaQueryData {
            size: Size::new(800.0, 400.0),
            ..portrait
        };

        assert!(
            !MediaQueryData::is_aspect_stale(&portrait, &taller, aspect::ORIENTATION),
            "taller is not turned"
        );
        assert!(
            MediaQueryData::is_aspect_stale(&portrait, &taller, aspect::SIZE),
            "and a reader of the size does hear about it"
        );
        assert!(MediaQueryData::is_aspect_stale(
            &portrait,
            &turned,
            aspect::ORIENTATION
        ));
    }

    #[test]
    fn the_clock_is_its_own_aspect() {
        let twelve = MediaQueryData::default();
        let twenty_four = MediaQueryData {
            always_use_24_hour_format: true,
            ..twelve
        };
        assert!(MediaQueryData::is_aspect_stale(
            &twelve,
            &twenty_four,
            aspect::ALWAYS_USE_24_HOUR_FORMAT
        ));
        // And nothing else about the view changed, so nobody else is woken.
        assert!(!MediaQueryData::is_aspect_stale(
            &twelve,
            &twenty_four,
            aspect::SIZE
        ));
        assert!(!MediaQueryData::is_aspect_stale(
            &twelve,
            &twenty_four,
            aspect::TEXT_SCALE
        ));
    }

    #[test]
    fn a_view_reports_the_on_off_labels_off_because_it_cannot_read_them() {
        // The bridge has a `did_change_accessibility_features` hook and
        // nothing behind it, so a view's data says no and a `MediaQuery` above
        // is the only thing that can say otherwise.
        //
        // Checked through `from_view` rather than `default()`: a mutation
        // making the view report `true` survived a test that built the data
        // the other way -- which is the gate the plan added last tick, not
        // obeyed here the first time.
        assert!(!MediaQueryData::from_view(&metrics(84.0, 0.0)).on_off_switch_labels);
        assert!(!MediaQueryData::from_view(&metrics(0.0, 600.0)).on_off_switch_labels);
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
        assert_eq!(
            after.get(),
            1.5,
            "the inner scale leaked out of its subtree"
        );
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
        assert_eq!(
            outside.text_scale(),
            1.0,
            "no media query, no platform scale set"
        );

        let inside = with_text_scale(1.75, || crate::render::RenderParagraph::new("bigger"));
        assert_eq!(inside.text_scale(), 1.75);
        // And the scale is not still in force afterwards.
        assert_eq!(
            crate::render::RenderParagraph::new("after").text_scale(),
            1.0
        );
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

    // -- Aspect subscriptions -----------------------------------------------

    /// Counts its builds and reads one part of the enclosing MediaQuery, the
    /// way a real consumer would.
    struct Probe {
        read: fn(&BuildContext) -> f32,
        builds: Rc<std::cell::Cell<u32>>,
    }

    impl Component for Probe {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            self.builds.set(self.builds.get() + 1);
            let value = (self.read)(context);
            leaf(move || SizedBox::new(value, 1.0))
        }
    }

    fn probe(read: fn(&BuildContext) -> f32) -> (AnyWidget, Rc<std::cell::Cell<u32>>) {
        let builds = Rc::new(std::cell::Cell::new(0));
        (
            component(Probe {
                read,
                builds: Rc::clone(&builds),
            }),
            builds,
        )
    }

    fn probe_column(probes: Vec<AnyWidget>) -> AnyWidget {
        many(probes, |children| {
            let mut flex = crate::render::RenderFlex::column();
            for child in children {
                flex = flex.push(child);
            }
            Box::new(flex)
        })
    }

    #[test]
    fn a_size_reader_ignores_a_padding_change_and_vice_versa() {
        let (size_widget, size_builds) = probe(|context| size_of(context).width);
        let (padding_widget, padding_builds) = probe(|context| padding_of(context).top);
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(
            MediaQueryData::default(),
            probe_column(vec![size_widget, padding_widget]),
        ));
        assert_eq!(size_builds.get(), 1);
        assert_eq!(padding_builds.get(), 1);

        // The view gets taller; the padding did not move.
        let mut taller = MediaQueryData::default();
        taller.size = Size::new(400.0, 900.0);
        assert!(tree.publish(taller));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(size_builds.get(), 2, "the size reader was rebuilt");
        assert_eq!(padding_builds.get(), 1, "the padding reader was not");

        // The status bar grows; the view does not change size.
        let mut barred = taller;
        barred.padding = EdgeInsets {
            top: 42.0,
            ..EdgeInsets::ZERO
        };
        assert!(tree.publish(barred));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(
            size_builds.get(),
            2,
            "and this time the size reader was not"
        );
        assert_eq!(padding_builds.get(), 2, "but the padding reader was");
    }

    #[test]
    fn the_keyboard_and_the_text_scale_are_separate_aspects() {
        // view_insets is the keyboard arriving; the text scale is a setting
        // changing. A reader of one should not hear about the other.
        let (insets_widget, insets_builds) = probe(|context| view_insets_of(context).bottom);
        let (scale_widget, scale_builds) = probe(text_scale_of);
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(
            MediaQueryData::default(),
            probe_column(vec![insets_widget, scale_widget]),
        ));

        let mut keyboard_up = MediaQueryData::default();
        keyboard_up.view_insets = EdgeInsets {
            bottom: 300.0,
            ..EdgeInsets::ZERO
        };
        assert!(tree.publish(keyboard_up));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(insets_builds.get(), 2, "the keyboard reader was rebuilt");
        assert_eq!(scale_builds.get(), 1, "the text scale reader was not");

        let louder = keyboard_up.with_text_scale(2.0);
        assert!(tree.publish(louder));
        assert_eq!(tree.rebuild_dirty(), 1);
        assert_eq!(
            insets_builds.get(),
            2,
            "and this time the keyboard reader was not"
        );
        assert_eq!(scale_builds.get(), 2, "but the text scale reader was");
    }

    #[test]
    fn a_reader_of_the_whole_data_hears_about_every_part() {
        // `media_query_of` -- what SafeArea reads -- did not qualify, so
        // nothing about aspect subscriptions may make it miss a change.
        let (whole_widget, builds) = probe(|context| media_query_of(context).size.width);
        let mut tree = ElementTree::new();
        tree.rebuild(MediaQuery::new(MediaQueryData::default(), whole_widget));

        let mut taller = MediaQueryData::default();
        taller.size = Size::new(400.0, 900.0);
        assert!(tree.publish(taller));
        tree.rebuild_dirty();
        assert_eq!(builds.get(), 2, "a size change is its news");

        let louder = taller.with_text_scale(1.5);
        assert!(tree.publish(louder));
        tree.rebuild_dirty();
        assert_eq!(builds.get(), 3, "and so is a text scale change");
    }
    // -- SystemTextScaler ------------------------------------------------------

    /// Android's non-linear curve, roughly: small text is enlarged a lot and
    /// text that is already large much less.
    fn android_non_linear(font_size: f32) -> f32 {
        if font_size <= 14.0 {
            font_size * 2.0
        } else if font_size <= 24.0 {
            font_size * 1.5
        } else {
            font_size * 1.1
        }
    }

    #[test]
    fn the_platform_scaler_cannot_be_a_number_because_it_is_not_a_line() {
        // At a large accessibility setting, small text is enlarged a lot and
        // text that is already large much less, so the layout does not fall
        // apart. No single multiplier says that.
        let scaler = SystemTextScaler::new(2.0, android_non_linear);
        assert_eq!(scaler.scale(10.0), 20.0);
        assert_eq!(scaler.scale(20.0), 30.0);
        assert!((scaler.scale(40.0) - 44.0).abs() < 1e-4);

        assert_ne!(
            scaler.scale(40.0),
            40.0 * scaler.text_scale_factor(),
            "multiplying by the factor would invent a model the platform never agreed to"
        );
    }

    #[test]
    fn the_factor_is_for_comparing_scalers_and_nothing_else() {
        // Two with the same factor produce the same output for the same input,
        // so it is a sound identity.
        let a = SystemTextScaler::new(1.5, android_non_linear);
        let b = SystemTextScaler::new(1.5, android_non_linear);
        let c = SystemTextScaler::new(2.0, android_non_linear);
        assert!(a.same_as(&b));
        assert!(!a.same_as(&c));
    }

    #[test]
    fn a_system_scaler_at_one_is_the_same_function_as_no_scaling_at_all() {
        // A widget comparing against "no scaling" to decide whether it can take
        // a shortcut gets the right answer without knowing where the scaler
        // came from.
        assert!(SystemTextScaler::new(1.0, |size| size).equals_no_scaling());
        assert!(!SystemTextScaler::new(1.5, android_non_linear).equals_no_scaling());
        assert_eq!(
            crate::painting::TextScaler::NO_SCALING.text_scale_factor,
            1.0
        );
    }
    // -- SliverSafeArea --------------------------------------------------------

    const NOTCH: EdgeInsets = EdgeInsets::only(0.0, 44.0, 0.0, 34.0);

    #[test]
    fn a_minimum_is_a_floor_and_not_an_addition() {
        // sixteen everywhere, and more where the notch demands it -- not
        // sixteen plus the notch.
        let area = SliverSafeArea::new().with_minimum(EdgeInsets::all(16.0));
        let padding = area.resolve_padding(NOTCH);
        assert_eq!(padding.top, 44.0, "the notch is larger, so the notch wins");
        assert_eq!(padding.bottom, 34.0);
        assert_eq!(
            padding.left, 16.0,
            "and the minimum wins where it is larger"
        );
        assert_eq!(padding.right, 16.0);
    }

    #[test]
    fn a_side_that_was_not_asked_for_still_gets_its_minimum() {
        let area = SliverSafeArea::new()
            .with_sides(true, false, true, true)
            .with_minimum(EdgeInsets::all(8.0));
        assert_eq!(area.resolve_padding(NOTCH).top, 8.0);
    }

    #[test]
    fn the_sides_it_consumed_are_zeroed_for_whatever_is_below_it() {
        // So a safe area inside a safe area does not inset twice.
        let area = SliverSafeArea::new();
        assert_eq!(area.inner_padding(NOTCH), EdgeInsets::ZERO);
    }

    #[test]
    fn a_side_it_left_alone_is_passed_through_untouched() {
        let bottom_only = SliverSafeArea::new().with_sides(false, false, false, true);
        let inner = bottom_only.inner_padding(NOTCH);
        assert_eq!(inner.top, 44.0, "still there for somebody else to handle");
        assert_eq!(inner.bottom, 0.0, "and this one is dealt with");
        assert_eq!(bottom_only.resolve_padding(NOTCH).top, 0.0);
    }

    #[test]
    fn a_sliver_safe_area_avoids_every_side_unless_told_otherwise() {
        let area = SliverSafeArea::default();
        assert!(area.left && area.top && area.right && area.bottom);
        assert_eq!(area.minimum, EdgeInsets::ZERO);
        assert_eq!(area.resolve_padding(NOTCH), NOTCH);
    }
}
