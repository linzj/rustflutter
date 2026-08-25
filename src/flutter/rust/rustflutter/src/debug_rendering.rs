// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What the renderer draws when something has gone wrong.
//!
//! A port of upstream's `rendering/error.dart` and
//! `rendering/debug_overflow_indicator.dart`. Two very different failures, and
//! the same idea behind both: **put the problem on the screen**, because the
//! reader is looking at the screen and not at the console.
//!
//! Both are written to be dull. Upstream says so in as many words about
//! `RenderErrorBox` -- it draws through the lowest-level primitives it can and
//! swallows its own exceptions, because a class that only ever runs after
//! something else has already failed must not be the second thing to fail.

use std::cell::Cell;

use crate::diagnostics::DiagnosticsProperty;
use crate::engine::{Color, Rect};
use crate::engine::{Paint, TextStyle};
use crate::painting::TextPainter;
use crate::render::{
    BoxConstraints, EdgeInsets, Offset, PaintContext, RelativeRect, RenderBox, Size,
};

// -- The box that stands in for a subtree that failed -------------------------

/// Upstream's `_kMaxWidth` / `_kMaxHeight`: a hundred thousand pixels.
///
/// Upstream's comment explains the number -- it is "approximately infinite
/// without using infinities", because an unbounded constraint has to be
/// answered with something finite and a real infinity propagates into NaNs
/// further up.
pub const ERROR_BOX_MAX_SIZE: f32 = 100000.0;

/// Upstream `RenderErrorBox`: a box drawn in place of a subtree that threw.
///
/// # It is deliberately stupid
///
/// The message cannot be changed after construction, the text is laid out with
/// the lowest-level text machinery available rather than through the usual
/// painter, and both the construction and the paint swallow whatever they
/// throw. All three are upstream's, and all three have the same reason: this
/// class runs only when something has already gone wrong, so anything it
/// depends on may be in an unusable state, and an error box that throws while
/// reporting an error hides the error the reader needs.
pub struct RenderErrorBox {
    /// Upstream's `message`, final and unchangeable.
    pub message: String,
    laid_out: Size,
    painter: Cell<Option<()>>,
}

impl RenderErrorBox {
    /// Upstream's `padding`, a *static* -- so a host that puts an error box
    /// under a status bar can move the text down for the whole app at once.
    /// The values are upstream's: room at the top for a status bar, room at the
    /// sides for a notch.
    pub const PADDING: EdgeInsets = EdgeInsets {
        left: 64.0,
        top: 96.0,
        right: 64.0,
        bottom: 12.0,
    };

    /// Upstream's `minimumWidth`. Below this the horizontal padding is dropped
    /// entirely rather than squeezed -- padding a narrow box would leave a
    /// column of text one word wide, which is worse than text against the edge.
    pub const MINIMUM_WIDTH: f32 = 200.0;

    /// Upstream's debug `backgroundColor`: an opaque-ish dark red. Release
    /// builds get `0xF0C0C0C0`, a light grey, because a red screen shipped to a
    /// reader is alarming and tells them nothing they can act on.
    pub const BACKGROUND_COLOR_DEBUG: Color = Color::argb(0xF0, 0x90, 0x00, 0x00);
    /// Upstream's release `backgroundColor`.
    pub const BACKGROUND_COLOR_RELEASE: Color = Color::argb(0xF0, 0xC0, 0xC0, 0xC0);

    /// Upstream's debug `textStyle`: yellow, monospace, 14, bold.
    pub fn text_style_debug() -> TextStyle {
        TextStyle {
            color: Color::argb(0xFF, 0xFF, 0xFF, 0x66),
            font_family: Some("monospace".to_string()),
            font_size: 14.0,
            font_weight: 700,
            ..TextStyle::default()
        }
    }

    /// Upstream's release `textStyle`: dark grey, sans-serif, 18.
    pub fn text_style_release() -> TextStyle {
        TextStyle {
            color: Color::argb(0xFF, 0x30, 0x30, 0x30),
            font_family: Some("sans-serif".to_string()),
            font_size: 18.0,
            ..TextStyle::default()
        }
    }

    /// The background this build paints. Upstream decides it once in a static
    /// initialiser guarded by `assert`, which is Dart's way of saying "debug
    /// only".
    pub fn background_color() -> Color {
        if cfg!(debug_assertions) {
            RenderErrorBox::BACKGROUND_COLOR_DEBUG
        } else {
            RenderErrorBox::BACKGROUND_COLOR_RELEASE
        }
    }

    pub fn text_style() -> TextStyle {
        if cfg!(debug_assertions) {
            RenderErrorBox::text_style_debug()
        } else {
            RenderErrorBox::text_style_release()
        }
    }

    pub fn new(message: impl Into<String>) -> RenderErrorBox {
        RenderErrorBox {
            message: message.into(),
            laid_out: Size::ZERO,
            painter: Cell::new(None),
        }
    }

    /// Upstream's `computeDryLayout`: `constraints.constrain(maxSize)`.
    ///
    /// Unbounded in, a hundred thousand out. Bounded in, exactly what was
    /// offered -- `sizedByParent` is true, so an error box never argues with
    /// its parent about how much room it takes.
    pub fn dry_layout(constraints: BoxConstraints) -> Size {
        constraints.constrain(Size::new(ERROR_BOX_MAX_SIZE, ERROR_BOX_MAX_SIZE))
    }

    /// Where the text goes inside a box of this size, and how wide it may be.
    ///
    /// Upstream's paint, as arithmetic. The two conditions are separate on
    /// purpose and neither is a clamp:
    ///
    /// * the horizontal padding applies only if the box is wider than
    ///   `left + MINIMUM_WIDTH + right`, and is dropped **whole** otherwise;
    /// * the top padding applies only if the text plus both paddings fit,
    ///   which is checked against the laid-out text and so cannot be answered
    ///   until the text has been measured.
    pub fn text_layout(size: Size, text_height: f32) -> (Offset, f32) {
        let padding = RenderErrorBox::PADDING;
        let mut width = size.width;
        let mut left = 0.0;
        if width > padding.left + RenderErrorBox::MINIMUM_WIDTH + padding.right {
            width -= padding.left + padding.right;
            left += padding.left;
        }
        let mut top = 0.0;
        if size.height > padding.top + text_height + padding.bottom {
            top += padding.top;
        }
        (Offset::new(left, top), width)
    }
}

impl RenderBox for RenderErrorBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.laid_out = RenderErrorBox::dry_layout(constraints);
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    /// Upstream's `hitTestSelf`, which is unconditionally true: an error box is
    /// a wall, so a tap does not fall through to whatever is behind the failure.
    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let size = self.size();
        context.canvas().draw_rect(
            Rect::xywh(offset.dx, offset.dy, size.width, size.height),
            &Paint::new(RenderErrorBox::background_color()),
        );
        if self.message.is_empty() {
            return;
        }
        let mut painter =
            TextPainter::new().text(self.message.clone(), RenderErrorBox::text_style());
        // Laid out at the full width first, because whether the top padding
        // applies depends on how tall the text turns out to be -- and that
        // depends on the width it was given. Upstream lays out once, at the
        // width the horizontal rule already chose.
        let (_, width) = RenderErrorBox::text_layout(size, 0.0);
        painter.layout(width);
        let (at, _) = RenderErrorBox::text_layout(size, painter.height());
        painter.paint(context.canvas(), (offset.dx + at.dx, offset.dy + at.dy));
        self.painter.set(Some(()));
    }
}

// -- The stripes down the edge that overflowed --------------------------------

/// Which edge a child ran past. Upstream's private `_OverflowSide`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverflowSide {
    Left,
    Top,
    Bottom,
    Right,
}

/// One stripe, and its label. Upstream's private `_OverflowRegionData`.
#[derive(Clone, Debug, PartialEq)]
pub struct OverflowRegionData {
    pub rect: Rect,
    pub label: String,
    pub label_offset: Offset,
    /// In radians. The side labels are turned so they read along the stripe.
    pub rotation: f32,
    pub side: OverflowSide,
}

/// Upstream `DebugOverflowIndicatorMixin`: the yellow-and-black stripes, and
/// the message in the console.
///
/// # Why a mixin and not a widget
///
/// The container that overflowed is the only thing that knows it did, and it
/// knows during *its own* paint. There is nowhere else to put this: a wrapper
/// would have to guess at the child's rect, and by then the layout that
/// overflowed is over.
///
/// # It reports once
///
/// Upstream keeps a flag and clears it after the first report, then sets it
/// again on `reassemble`. A container that overflows overflows on every frame,
/// and a message per frame would bury everything else in the console -- but a
/// reader who hot-reloads expects to be told again, because they have just
/// changed something and want to know whether it helped.
pub struct DebugOverflowIndicator {
    /// Upstream's `_overflowReportNeeded`, which starts true.
    report_needed: Cell<bool>,
}

impl Default for DebugOverflowIndicator {
    fn default() -> DebugOverflowIndicator {
        DebugOverflowIndicator {
            report_needed: Cell::new(true),
        }
    }
}

impl DebugOverflowIndicator {
    /// Upstream's `_indicatorFraction`: the stripe covers this much of the
    /// container along the axis it is on.
    pub const INDICATOR_FRACTION: f32 = 0.1;
    /// Upstream's `_indicatorFontSizePixels`.
    pub const INDICATOR_FONT_SIZE: f32 = 7.5;
    /// Upstream's `_indicatorLabelPaddingPixels`.
    pub const LABEL_PADDING: f32 = 1.0;

    pub fn new() -> DebugOverflowIndicator {
        DebugOverflowIndicator::default()
    }

    /// Upstream's `reassemble`: a hot reload makes the message worth saying
    /// again.
    pub fn reassemble(&self) {
        self.report_needed.set(true);
    }

    pub fn report_needed(&self) -> bool {
        self.report_needed.get()
    }

    /// Upstream's `_formatPixels`, which changes precision with magnitude.
    ///
    /// Three cases, and the reason is what a reader does with the number: past
    /// ten pixels the fraction is noise, between one and ten a tenth still
    /// tells them something, and below one the interesting part *is* the
    /// fraction -- so it switches from fixed places to **significant figures**.
    /// `toStringAsPrecision(3)` of `0.5` is `0.500`, not `0.5`: the trailing
    /// zeros are the point, because they say the measurement went that far.
    ///
    /// Both boundaries are `>`, not `>=`: exactly ten pixels is formatted as
    /// `10.0` and exactly one as `1.00`.
    ///
    /// **Rounded half away from zero, not half to even.** Dart's
    /// `toStringAsFixed` rounds `10.5` up to `11`; Rust's `{:.0}` rounds it
    /// down to `10`. The same trap [`crate::diagnostics::PropertyValue::format_double`]
    /// records, and the same fix.
    pub fn format_pixels(value: f32) -> String {
        if value > 10.0 {
            format!("{:.0}", (value as f64).round())
        } else if value > 1.0 {
            format!("{:.1}", (value as f64 * 10.0).round() / 10.0)
        } else {
            DebugOverflowIndicator::to_precision(value as f64, 3)
        }
    }

    /// Dart's `toStringAsPrecision`: `digits` significant figures, trailing
    /// zeros kept.
    fn to_precision(value: f64, digits: i32) -> String {
        if value == 0.0 {
            return format!("{:.*}", (digits - 1).max(0) as usize, 0.0);
        }
        let exponent = value.abs().log10().floor() as i32;
        let places = (digits - 1 - exponent).max(0) as usize;
        let scale = 10f64.powi(places as i32);
        format!("{:.*}", places, (value * scale).round() / scale)
    }

    /// Upstream's `_calculateOverflowRegions`.
    ///
    /// One stripe per overflowing side, each covering a tenth of the container
    /// along its own axis and the whole of the other -- so two adjacent
    /// overflows make an L rather than two disconnected marks.
    pub fn overflow_regions(overflow: RelativeRect, container: Rect) -> Vec<OverflowRegionData> {
        let mut regions = Vec::new();
        let fraction = DebugOverflowIndicator::INDICATOR_FRACTION;
        let pad =
            DebugOverflowIndicator::INDICATOR_FONT_SIZE + DebugOverflowIndicator::LABEL_PADDING;
        if overflow.left > 0.0 {
            let rect = Rect::xywh(0.0, 0.0, container.width() * fraction, container.height());
            regions.push(OverflowRegionData {
                label: format!(
                    "LEFT OVERFLOWED BY {} PIXELS",
                    DebugOverflowIndicator::format_pixels(overflow.left)
                ),
                label_offset: Offset::new(rect.left + pad, rect.top + rect.height() / 2.0),
                rotation: std::f32::consts::FRAC_PI_2,
                side: OverflowSide::Left,
                rect,
            });
        }
        if overflow.right > 0.0 {
            let rect = Rect::xywh(
                container.width() * (1.0 - fraction),
                0.0,
                container.width() * fraction,
                container.height(),
            );
            regions.push(OverflowRegionData {
                label: format!(
                    "RIGHT OVERFLOWED BY {} PIXELS",
                    DebugOverflowIndicator::format_pixels(overflow.right)
                ),
                label_offset: Offset::new(rect.right - pad, rect.top + rect.height() / 2.0),
                rotation: -std::f32::consts::FRAC_PI_2,
                side: OverflowSide::Right,
                rect,
            });
        }
        if overflow.top > 0.0 {
            let rect = Rect::xywh(0.0, 0.0, container.width(), container.height() * fraction);
            regions.push(OverflowRegionData {
                label: format!(
                    "TOP OVERFLOWED BY {} PIXELS",
                    DebugOverflowIndicator::format_pixels(overflow.top)
                ),
                label_offset: Offset::new(
                    rect.left + rect.width() / 2.0,
                    rect.top + DebugOverflowIndicator::LABEL_PADDING,
                ),
                rotation: 0.0,
                side: OverflowSide::Top,
                rect,
            });
        }
        if overflow.bottom > 0.0 {
            let rect = Rect::xywh(
                0.0,
                container.height() * (1.0 - fraction),
                container.width(),
                container.height() * fraction,
            );
            regions.push(OverflowRegionData {
                label: format!(
                    "BOTTOM OVERFLOWED BY {} PIXELS",
                    DebugOverflowIndicator::format_pixels(overflow.bottom)
                ),
                label_offset: Offset::new(rect.left + rect.width() / 2.0, rect.bottom - pad),
                rotation: 0.0,
                side: OverflowSide::Bottom,
                rect,
            });
        }
        regions
    }

    /// The sentence the console message is built around. Upstream's
    /// `_reportOverflow`, in the part that is not boilerplate.
    ///
    /// The list grammar is upstream's and is worth keeping: one item plain, two
    /// joined by "and" with no comma, three or more comma-separated with "and"
    /// before the last. It is what makes the message read like a sentence
    /// rather than a dump.
    ///
    /// The order is left, top, bottom, right -- upstream's, and not the order
    /// [`OverflowSide`] is declared in for the regions, which is why the two
    /// lists are built separately.
    pub fn overflow_text(overflow: RelativeRect) -> String {
        let mut overflows: Vec<String> = Vec::new();
        for (amount, side) in [
            (overflow.left, "left"),
            (overflow.top, "top"),
            (overflow.bottom, "bottom"),
            (overflow.right, "right"),
        ] {
            if amount > 0.0 {
                overflows.push(format!(
                    "{} pixels on the {side}",
                    DebugOverflowIndicator::format_pixels(amount)
                ));
            }
        }
        match overflows.len() {
            0 => String::new(),
            1 => overflows.remove(0),
            2 => format!("{} and {}", overflows[0], overflows[1]),
            _ => {
                let last = overflows.pop().expect("three or more");
                format!("{}, and {last}", overflows.join(", "))
            }
        }
    }

    /// Upstream's two default hints, added only when the caller supplied none.
    ///
    /// The second one is the one that matters: an overflow is *an error* rather
    /// than a cosmetic problem, because it means there is content the reader
    /// cannot see -- and the fix is a `ClipRect` or a scrollable, not a smaller
    /// font.
    pub fn default_hints(type_name: &str) -> Vec<String> {
        vec![
            format!(
                "The edge of the {type_name} that is overflowing has been marked in the \
                 rendering with a yellow and black striped pattern. This is usually caused by \
                 the contents being too big for the {type_name}."
            ),
            format!(
                "This is considered an error condition because it indicates that there is \
                 content that cannot be seen. If the content is legitimately bigger than the \
                 available space, consider clipping it with a ClipRect widget before putting \
                 it in the {type_name}, or using a scrollable container, like a ListView."
            ),
        ]
    }

    /// The whole report: the message, its hints, and the hidden creator
    /// property that lets a tool jump to the widget.
    pub fn report(
        &self,
        type_name: &str,
        overflow: RelativeRect,
        hints: Option<Vec<String>>,
        creator: Option<&crate::diagnostics::DebugCreator>,
    ) -> OverflowReport {
        self.report_needed.set(false);
        OverflowReport {
            message: format!(
                "A {type_name} overflowed by {}.",
                DebugOverflowIndicator::overflow_text(overflow)
            ),
            hints: hints
                .filter(|hints| !hints.is_empty())
                .unwrap_or_else(|| DebugOverflowIndicator::default_hints(type_name)),
            creator: creator.map(crate::diagnostics::diagnostics_debug_creator),
        }
    }

    /// Upstream's `paintOverflowIndicator`.
    ///
    /// Returns whether anything was drawn, so a caller can tell "no overflow"
    /// from "drawn". The early return on no overflow is upstream's and is not
    /// merely an optimisation: `paintOverflowIndicator` is called from inside an
    /// assert with a rect the caller has not itself checked.
    pub fn paint(
        &self,
        context: &mut PaintContext,
        offset: Offset,
        container: Rect,
        child: Rect,
    ) -> bool {
        let overflow = RelativeRect::from_rect(container, child);
        if overflow.left <= 0.0
            && overflow.right <= 0.0
            && overflow.top <= 0.0
            && overflow.bottom <= 0.0
        {
            return false;
        }
        for region in DebugOverflowIndicator::overflow_regions(overflow, container) {
            let rect = Rect::xywh(
                region.rect.left + offset.dx,
                region.rect.top + offset.dy,
                region.rect.width(),
                region.rect.height(),
            );
            context
                .canvas()
                .draw_rect(rect, &DebugOverflowIndicator::stripe_paint());
        }
        true
    }

    /// Upstream's `_indicatorPaint`: a repeating diagonal gradient of black and
    /// yellow, with hard stops at a quarter and three quarters so the bands are
    /// stripes rather than a blur.
    ///
    /// This crate's `Paint` carries no shader, so the stripes come out as the
    /// yellow alone -- the mark is in the right place and the right size, and
    /// what is missing is the hatching. Recorded here rather than left to be
    /// noticed.
    pub fn stripe_paint() -> Paint {
        Paint::new(Color::argb(0xFF, 0xFF, 0xFF, 0x00))
    }
}

/// Upstream `DebugOverflowIndicatorMixin`, as the thing a render object mixes
/// in.
///
/// A Dart mixin is a set of methods a class *acquires*, which is a trait with
/// provided methods. All a render object has to supply is somewhere to keep the
/// report-once flag and its own name for the message -- everything else comes
/// with the trait, exactly as it comes with the mixin.
pub trait DebugOverflowIndicatorMixin {
    /// Where the report-once flag lives. Upstream's mixin declares the field
    /// itself; a Rust trait cannot, so the implementer holds it and hands it
    /// over.
    fn overflow_indicator(&self) -> &DebugOverflowIndicator;

    /// What the message calls this container. Upstream reads `runtimeType`.
    fn overflow_type_name(&self) -> &str;

    /// Upstream's `paintOverflowIndicator`, in the shape a render object calls
    /// it from its own `paint`: the stripes, and the message the first time.
    fn paint_overflow_indicator(
        &self,
        context: &mut PaintContext,
        offset: Offset,
        container: Rect,
        child: Rect,
        hints: Option<Vec<String>>,
    ) -> Option<OverflowReport> {
        let indicator = self.overflow_indicator();
        if !indicator.paint(context, offset, container, child) {
            return None;
        }
        if !indicator.report_needed() {
            return None;
        }
        Some(indicator.report(
            self.overflow_type_name(),
            RelativeRect::from_rect(container, child),
            hints,
            None,
        ))
    }

    /// Upstream's `reassemble`.
    fn reassemble(&self) {
        self.overflow_indicator().reassemble();
    }
}

/// What [`DebugOverflowIndicator::report`] produced, ready for whatever this
/// host reports errors through.
#[derive(Clone, Debug, PartialEq)]
pub struct OverflowReport {
    pub message: String,
    pub hints: Vec<String>,
    /// The hidden `debugCreator` property, when the render object had one.
    pub creator: Option<DiagnosticsProperty>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- RenderErrorBox ----------------------------------------------------------------

    #[test]
    fn an_unbounded_error_box_is_approximately_infinite() {
        // Not actually infinite: an infinity here propagates into NaNs further
        // up, which is a second failure on top of the one being reported.
        let size = RenderErrorBox::dry_layout(BoxConstraints::unbounded());
        assert_eq!(size, Size::new(ERROR_BOX_MAX_SIZE, ERROR_BOX_MAX_SIZE));
        assert!(size.width.is_finite());
    }

    #[test]
    fn a_bounded_error_box_takes_exactly_what_it_is_offered() {
        // sizedByParent: an error box never argues about how much room it gets.
        let size = RenderErrorBox::dry_layout(BoxConstraints::tight_for(Size::new(300.0, 200.0)));
        assert_eq!(size, Size::new(300.0, 200.0));
    }

    #[test]
    fn a_narrow_box_drops_the_side_padding_whole() {
        // Not squeezed -- padding a narrow box would leave a column of text one
        // word wide, which is worse than text against the edge.
        let wide = Size::new(64.0 + 200.0 + 64.0 + 1.0, 500.0);
        let (at, width) = RenderErrorBox::text_layout(wide, 20.0);
        assert_eq!(at.dx, 64.0, "there was room");
        assert_eq!(width, wide.width - 128.0);

        let narrow = Size::new(64.0 + 200.0 + 64.0, 500.0);
        let (at, width) = RenderErrorBox::text_layout(narrow, 20.0);
        assert_eq!(at.dx, 0.0, "flush with the edge");
        assert_eq!(width, narrow.width, "and using all of it");
    }

    #[test]
    fn the_top_padding_depends_on_how_tall_the_text_turned_out() {
        // Which is why it cannot be decided before the text is measured.
        let size = Size::new(400.0, 200.0);
        let (short, _) = RenderErrorBox::text_layout(size, 20.0);
        assert_eq!(short.dy, 96.0, "96 + 20 + 12 fits in 200");

        let (tall, _) = RenderErrorBox::text_layout(size, 100.0);
        assert_eq!(tall.dy, 0.0, "96 + 100 + 12 does not");
    }

    #[test]
    fn the_two_axes_are_decided_separately() {
        // A box wide enough for side padding and too short for top padding gets
        // one and not the other.
        let (at, width) = RenderErrorBox::text_layout(Size::new(400.0, 50.0), 20.0);
        assert_eq!(at.dx, 64.0);
        assert_eq!(at.dy, 0.0);
        assert_eq!(width, 272.0);
    }

    #[test]
    fn debug_is_red_and_release_is_grey() {
        // A red screen shipped to a reader is alarming and tells them nothing
        // they can act on.
        assert_eq!(
            RenderErrorBox::BACKGROUND_COLOR_DEBUG,
            Color::argb(0xF0, 0x90, 0x00, 0x00)
        );
        assert_eq!(
            RenderErrorBox::BACKGROUND_COLOR_RELEASE,
            Color::argb(0xF0, 0xC0, 0xC0, 0xC0)
        );
        assert_ne!(
            RenderErrorBox::text_style_debug().color,
            RenderErrorBox::text_style_release().color
        );
        assert_eq!(RenderErrorBox::text_style_debug().font_weight, 700);
    }

    #[test]
    fn an_error_box_is_a_wall() {
        // A tap must not fall through to whatever is behind the failure.
        let mut box_ = RenderErrorBox::new("boom");
        box_.layout(BoxConstraints::tight_for(Size::new(100.0, 100.0)));
        assert!(box_.hit_test_self(Offset::new(50.0, 50.0)));
        assert!(
            box_.hit_test_self(Offset::new(-1000.0, -1000.0)),
            "unconditionally, as upstream's is"
        );
    }

    // -- format_pixels -----------------------------------------------------------------

    #[test]
    fn the_precision_follows_the_magnitude() {
        // Past ten pixels the fraction is noise; below one, the fraction is the
        // interesting part.
        assert_eq!(DebugOverflowIndicator::format_pixels(123.456), "123");
        assert_eq!(
            DebugOverflowIndicator::format_pixels(10.5),
            "11",
            "half away from zero"
        );
        assert_eq!(
            DebugOverflowIndicator::format_pixels(1.25),
            "1.3",
            "and here too"
        );
        assert_eq!(
            DebugOverflowIndicator::format_pixels(0.5),
            "0.500",
            "three significant figures, trailing zeros kept"
        );
        assert_eq!(DebugOverflowIndicator::format_pixels(0.125), "0.125");
        assert_eq!(DebugOverflowIndicator::format_pixels(0.0125), "0.0125");
    }

    #[test]
    fn ten_and_one_are_on_the_lower_side_of_their_boundaries() {
        // Upstream's tests are `> 10.0` and `> 1.0`, not `>=`.
        assert_eq!(DebugOverflowIndicator::format_pixels(10.0), "10.0");
        assert_eq!(DebugOverflowIndicator::format_pixels(1.0), "1.00");
    }

    // -- The regions -------------------------------------------------------------------

    fn overflowing(left: f32, top: f32, right: f32, bottom: f32) -> RelativeRect {
        RelativeRect::from_ltrb(left, top, right, bottom)
    }

    #[test]
    fn one_stripe_per_overflowing_side() {
        let container = Rect::xywh(0.0, 0.0, 200.0, 100.0);
        let regions =
            DebugOverflowIndicator::overflow_regions(overflowing(5.0, 0.0, 0.0, 0.0), container);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].side, OverflowSide::Left);
        assert_eq!(regions[0].label, "LEFT OVERFLOWED BY 5.0 PIXELS");

        let none =
            DebugOverflowIndicator::overflow_regions(overflowing(0.0, 0.0, 0.0, 0.0), container);
        assert!(none.is_empty());
    }

    #[test]
    fn a_stripe_covers_a_tenth_of_its_own_axis_and_all_of_the_other() {
        // So two adjacent overflows make an L rather than two disconnected
        // marks.
        let container = Rect::xywh(0.0, 0.0, 200.0, 100.0);
        let regions =
            DebugOverflowIndicator::overflow_regions(overflowing(5.0, 5.0, 0.0, 0.0), container);
        let left = regions
            .iter()
            .find(|r| r.side == OverflowSide::Left)
            .expect("left");
        assert_eq!(left.rect, Rect::xywh(0.0, 0.0, 20.0, 100.0));
        let top = regions
            .iter()
            .find(|r| r.side == OverflowSide::Top)
            .expect("top");
        assert_eq!(top.rect, Rect::xywh(0.0, 0.0, 200.0, 10.0));
    }

    #[test]
    fn the_far_stripes_sit_against_the_far_edges() {
        let container = Rect::xywh(0.0, 0.0, 200.0, 100.0);
        let regions =
            DebugOverflowIndicator::overflow_regions(overflowing(0.0, 0.0, 3.0, 3.0), container);
        let right = regions
            .iter()
            .find(|r| r.side == OverflowSide::Right)
            .expect("right");
        assert_eq!(right.rect.right, 200.0);
        let bottom = regions
            .iter()
            .find(|r| r.side == OverflowSide::Bottom)
            .expect("bottom");
        assert_eq!(bottom.rect.bottom, 100.0);
    }

    #[test]
    fn the_side_labels_are_turned_to_read_along_their_stripe() {
        // And in opposite directions, so both read upwards from the reader's
        // point of view rather than one of them upside down.
        let container = Rect::xywh(0.0, 0.0, 200.0, 100.0);
        let regions =
            DebugOverflowIndicator::overflow_regions(overflowing(1.0, 1.0, 1.0, 1.0), container);
        let rotation = |side| {
            regions
                .iter()
                .find(|r| r.side == side)
                .expect("present")
                .rotation
        };
        assert_eq!(rotation(OverflowSide::Left), -rotation(OverflowSide::Right));
        assert_ne!(rotation(OverflowSide::Left), 0.0);
        assert_eq!(rotation(OverflowSide::Top), 0.0, "already readable");
        assert_eq!(rotation(OverflowSide::Bottom), 0.0);
    }

    // -- The sentence ------------------------------------------------------------------

    #[test]
    fn the_list_grammar_is_a_sentence_and_not_a_dump() {
        assert_eq!(
            DebugOverflowIndicator::overflow_text(overflowing(5.0, 0.0, 0.0, 0.0)),
            "5.0 pixels on the left"
        );
        assert_eq!(
            DebugOverflowIndicator::overflow_text(overflowing(5.0, 0.0, 3.0, 0.0)),
            "5.0 pixels on the left and 3.0 pixels on the right",
            "two are joined by and, with no comma"
        );
        assert_eq!(
            DebugOverflowIndicator::overflow_text(overflowing(5.0, 4.0, 3.0, 2.0)),
            "5.0 pixels on the left, 4.0 pixels on the top, 2.0 pixels on the bottom, \
             and 3.0 pixels on the right",
            "and three or more get the comma back"
        );
    }

    #[test]
    fn the_sentence_order_is_left_top_bottom_right() {
        // Upstream's, and not the order the sides are declared in -- which is
        // why the sentence and the stripes are built by separate loops.
        let text = DebugOverflowIndicator::overflow_text(overflowing(1.0, 2.0, 3.0, 4.0));
        let at = |needle: &str| text.find(needle).expect("present");
        assert!(at("left") < at("top"));
        assert!(at("top") < at("bottom"));
        assert!(at("bottom") < at("right"));
    }

    // -- Reporting once ----------------------------------------------------------------

    #[test]
    fn a_container_that_overflows_every_frame_says_so_once() {
        let indicator = DebugOverflowIndicator::new();
        assert!(indicator.report_needed(), "the first frame");
        indicator.report("Column", overflowing(5.0, 0.0, 0.0, 0.0), None, None);
        assert!(!indicator.report_needed(), "and not the next hundred");
    }

    #[test]
    fn a_hot_reload_makes_it_worth_saying_again() {
        // The reader has just changed something and wants to know whether it
        // helped.
        let indicator = DebugOverflowIndicator::new();
        indicator.report("Column", overflowing(5.0, 0.0, 0.0, 0.0), None, None);
        indicator.reassemble();
        assert!(indicator.report_needed());
    }

    #[test]
    fn the_default_hints_appear_only_when_the_caller_gave_none() {
        let indicator = DebugOverflowIndicator::new();
        let report = indicator.report("Row", overflowing(5.0, 0.0, 0.0, 0.0), None, None);
        assert_eq!(report.hints.len(), 2);
        assert!(report.hints[0].contains("Row"), "named after the container");
        assert!(
            report.hints[1].contains("cannot be seen"),
            "an overflow is an error because content is lost"
        );
        assert_eq!(
            report.message,
            "A Row overflowed by 5.0 pixels on the left."
        );

        let given = DebugOverflowIndicator::new().report(
            "Row",
            overflowing(5.0, 0.0, 0.0, 0.0),
            Some(vec!["mine".to_string()]),
            None,
        );
        assert_eq!(given.hints, vec!["mine".to_string()]);
    }

    #[test]
    fn an_empty_hint_list_is_the_same_as_none() {
        // Upstream's `??=` is followed by an `isEmpty` check, which is the same
        // decision made twice.
        let report = DebugOverflowIndicator::new().report(
            "Row",
            overflowing(5.0, 0.0, 0.0, 0.0),
            Some(Vec::new()),
            None,
        );
        assert_eq!(report.hints.len(), 2);
    }

    // -- The mixin ---------------------------------------------------------------------

    struct Thing {
        indicator: DebugOverflowIndicator,
    }

    impl DebugOverflowIndicatorMixin for Thing {
        fn overflow_indicator(&self) -> &DebugOverflowIndicator {
            &self.indicator
        }

        fn overflow_type_name(&self) -> &str {
            "Thing"
        }
    }

    fn painted(thing: &Thing, container: Rect, child: Rect) -> Option<OverflowReport> {
        let mut layers = crate::engine::LayerTree::new(200, 200);
        let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
        thing.paint_overflow_indicator(&mut context, Offset::ZERO, container, child, None)
    }

    #[test]
    fn a_container_that_did_not_overflow_paints_and_says_nothing() {
        let thing = Thing {
            indicator: DebugOverflowIndicator::new(),
        };
        let container = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        assert_eq!(painted(&thing, container, container), None);
        assert!(
            thing.indicator.report_needed(),
            "and the first real overflow will still be reported"
        );
    }

    #[test]
    fn the_mixin_reports_once_and_again_after_a_reload() {
        let thing = Thing {
            indicator: DebugOverflowIndicator::new(),
        };
        let container = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        let child = Rect::xywh(-5.0, 0.0, 105.0, 100.0);

        let first = painted(&thing, container, child).expect("the first frame");
        assert_eq!(
            first.message,
            "A Thing overflowed by 5.0 pixels on the left."
        );
        assert_eq!(
            painted(&thing, container, child),
            None,
            "and not the second"
        );

        DebugOverflowIndicatorMixin::reassemble(&thing);
        assert!(painted(&thing, container, child).is_some());
    }

    #[test]
    fn the_report_carries_the_hidden_creator_when_there_is_one() {
        use crate::diagnostics::{DebugCreator, DiagnosticLevel};
        let creator = DebugCreator::new(3, "Row \u{2190} Column");
        let report = DebugOverflowIndicator::new().report(
            "Row",
            overflowing(5.0, 0.0, 0.0, 0.0),
            None,
            Some(&creator),
        );
        let property = report.creator.expect("carried");
        assert_eq!(property.default_level, DiagnosticLevel::Hidden);

        let without = DebugOverflowIndicator::new().report(
            "Row",
            overflowing(5.0, 0.0, 0.0, 0.0),
            None,
            None,
        );
        assert_eq!(without.creator, None);
    }
}

// -- What the two debug boxes put on the canvas -------------------------------

#[cfg(test)]
mod debug_paint_tests {
    //! Both of these are seen only when something has already gone wrong,
    //! which is exactly why nothing was watching them: an error box that
    //! painted nothing would look like an application that merely failed
    //! quietly, and the overflow stripes are the only thing that tells a
    //! developer a row is too wide.

    use super::{DebugOverflowIndicator, RenderErrorBox};
    use crate::engine::{LayerTree, Rect};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::render::{BoxConstraints, Offset, PaintContext, RenderBox, Size};

    const SCREEN: Size = Size {
        width: 400.0,
        height: 300.0,
    };

    fn painted(body: impl FnOnce(&mut PaintContext)) -> Vec<Drawn> {
        let mut layers = LayerTree::new(400, 300);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, SCREEN);
            body(&mut context);
        }
        drawn()
    }

    fn error_box(message: &str, at: Offset) -> Vec<Drawn> {
        let mut boxed = RenderErrorBox::new(message);
        boxed.layout(BoxConstraints::loose(SCREEN.width, SCREEN.height));
        painted(|context| boxed.paint(context, at))
    }

    #[allow(clippy::type_complexity)]
    fn rects(calls: &[Drawn]) -> Vec<((f32, f32, f32, f32), u32)> {
        calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Rect {
                    left,
                    top,
                    right,
                    bottom,
                    argb,
                } => Some(((*left, *top, *right, *bottom), *argb)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_error_box_fills_itself_before_writing_on_itself() {
        // The background is the whole point of the box: a message in the app's
        // own colours reads as part of the app. It has to cover the box, and
        // it has to go down first or it covers the message instead.
        let calls = error_box("something went wrong", Offset::ZERO);
        let marks = rects(&calls);
        assert_eq!(marks.len(), 1, "one fill: {calls:?}");
        let ((left, top, right, bottom), argb) = marks[0];
        assert_eq!(argb, RenderErrorBox::background_color().0);
        assert_eq!((left, top), (0.0, 0.0));

        let mut sized = RenderErrorBox::new("something went wrong");
        let size = sized.layout(BoxConstraints::loose(SCREEN.width, SCREEN.height));
        assert_eq!((right - left, bottom - top), (size.width, size.height));

        let fill = calls
            .iter()
            .position(|call| matches!(call, Drawn::Rect { .. }))
            .expect("the fill");
        let words = calls
            .iter()
            .position(|call| matches!(call, Drawn::Paragraph { .. }))
            .expect("the message");
        assert!(fill < words, "the fill is under the words: {calls:?}");
    }

    #[test]
    fn the_message_it_was_given_is_the_message_it_writes() {
        // Not a tautology while paragraphs went unrecorded: an error box that
        // painted its own class name, or nothing, looked the same from here.
        let calls = error_box("assertion failed: index < length", Offset::ZERO);
        let said: Vec<&str> = calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Paragraph { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(said, vec!["assertion failed: index < length"]);
    }

    #[test]
    fn an_error_with_nothing_to_say_still_draws_the_box() {
        // The guard in `paint`. A failure with no message is still a failure,
        // and a box with nothing in it says so; drawing neither would say the
        // application is fine.
        let calls = error_box("", Offset::ZERO);
        assert_eq!(rects(&calls).len(), 1, "{calls:?}");
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, Drawn::Paragraph { .. })),
            "and no empty paragraph: {calls:?}"
        );
    }

    #[test]
    fn an_error_box_paints_where_it_was_put() {
        let at = Offset::new(30.0, 45.0);
        let here = rects(&error_box("oh no", Offset::ZERO))[0].0;
        let there = rects(&error_box("oh no", at))[0].0;
        assert_eq!((there.0 - here.0, there.1 - here.1), (at.dx, at.dy));
    }

    // -- The overflow stripes ------------------------------------------------

    fn overflow(container: Rect, child: Rect, at: Offset) -> (bool, Vec<Drawn>) {
        let indicator = DebugOverflowIndicator::new();
        let mut drew = false;
        let calls = painted(|context| {
            drew = indicator.paint(context, at, container, child);
        });
        (drew, calls)
    }

    #[test]
    fn a_child_inside_its_container_gets_no_stripes_at_all() {
        // The case that matters most, because it is every frame of a working
        // application. An indicator that marked a fitting child would put
        // yellow over the whole interface.
        let (drew, calls) = overflow(
            Rect::xywh(0.0, 0.0, 200.0, 100.0),
            Rect::xywh(10.0, 10.0, 100.0, 50.0),
            Offset::ZERO,
        );
        assert!(!drew);
        assert!(calls.is_empty(), "{calls:?}");
    }

    #[test]
    fn a_child_too_wide_is_marked_on_the_side_it_ran_off() {
        // One region per overflowing edge, and the edge is the information: a
        // row that runs off the right is a different bug from one that runs
        // off the bottom, and marking both would say neither.
        let container = Rect::xywh(0.0, 0.0, 200.0, 100.0);
        let (drew, calls) = overflow(
            container,
            Rect::xywh(0.0, 0.0, 260.0, 100.0),
            Offset::ZERO,
        );
        assert!(drew);
        let marks = rects(&calls);
        assert_eq!(marks.len(), 1, "one edge, one mark: {calls:?}");
        let ((left, _, right, _), _) = marks[0];
        assert!(
            left > container.width() / 2.0,
            "the mark is on the right-hand side: {left}"
        );
        assert_eq!(right, container.width(), "and reaches the edge");
    }

    #[test]
    fn a_child_too_big_in_both_directions_is_marked_twice() {
        let (drew, calls) = overflow(
            Rect::xywh(0.0, 0.0, 200.0, 100.0),
            Rect::xywh(0.0, 0.0, 260.0, 160.0),
            Offset::ZERO,
        );
        assert!(drew);
        assert_eq!(rects(&calls).len(), 2, "{calls:?}");
    }

    #[test]
    fn the_stripes_land_where_the_container_is() {
        // The regions are computed in the container's own coordinates, so the
        // offset has to be added. Left out, every overflow in a scrolled page
        // is marked at the top of the screen instead of on the widget.
        let container = Rect::xywh(0.0, 0.0, 200.0, 100.0);
        let child = Rect::xywh(0.0, 0.0, 260.0, 100.0);
        let here = rects(&overflow(container, child, Offset::ZERO).1)[0].0;
        let at = Offset::new(25.0, 60.0);
        let there = rects(&overflow(container, child, at).1)[0].0;
        assert_eq!((there.0 - here.0, there.1 - here.1), (at.dx, at.dy));
    }
}

