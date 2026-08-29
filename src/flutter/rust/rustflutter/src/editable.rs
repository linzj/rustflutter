// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Editable text: the widget an application actually writes.
//!
//! [`crate::services::text_input`] is the channel the platform edits through.
//! Nothing above this file should ever see it. Upstream draws the same line and
//! draws it here: `EditableText` is the one class that implements
//! `TextInputClient`, opens and closes the connection, and keeps the platform's
//! copy of the text and the framework's copy the same. An application writes
//! `TextField(...)` and never learns that an IME exists.
//!
//! That is not tidiness. Adapting to an IME is genuinely hard -- typing 中文 is
//! several keystrokes that produce no text, then a candidate list, then one
//! character, and the half-typed part has to be drawn underlined and can be
//! replaced wholesale -- and it is exactly the same work in every application.
//! Doing it once, here, is the difference between a framework that supports
//! text input and one that has a channel for it.
//!
//! # What this layer owns
//!
//! - the field's [`TextEditingValue`], across frames
//! - opening the connection when the field is tapped, and closing it when
//!   another field is
//! - drawing the text, the composing underline and the caret
//! - telling the platform where the caret is, so the candidate list lands under
//!   the word being typed rather than in the corner of the window
//!
//! # What it does not own
//!
//! Editing. Backspace, the arrow keys, selection and the composition are the
//! platform's, applied to the platform's copy of the text, and arrive here as a
//! finished [`TextEditingValue`]. That is upstream's arrangement, and it is the
//! only one that can be right: the IME edits text the framework has not been
//! told about yet.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::components::theme_of;
use crate::engine::{Color, Rect, TextStyle};
use crate::framework::{AnyWidget, BuildContext, Key, StateHandle, StatefulComponent, leaf};
use crate::gestures::{PointerHandlers, TapEvent};
use crate::painting;
use crate::render::{BoxConstraints, PaintContext, RenderBox, RenderPointerRegion};
use crate::services::text_input::{
    self, TextEditingValue, TextInputAction, TextInputClient, TextInputConfiguration,
    TextInputConnection, TextInputType,
};
use crate::widgets::{Offset, Size};

/// How wide the caret is drawn, in logical pixels. Upstream's `cursorWidth`.
const CARET_WIDTH: f32 = 2.0;

/// Room left for the caret beyond the text, in logical pixels. Upstream's
/// `_caretMargin` (`_kCaretGap + cursorWidth`): the horizontal content that a
/// single-line field scrolls through is the text *plus this*, so the caret can
/// sit after the last character without being the thing that overflows.
const CARET_MARGIN: f32 = 1.0 + CARET_WIDTH;

/// Half of the caret's blink cycle, in microseconds. Upstream's
/// `_kCursorBlinkHalfPeriod`, half a second: the caret is shown for one half
/// and hidden for the other, starting shown the moment editing starts.
const CARET_BLINK_HALF_PERIOD_MICROS: i64 = 500_000;

/// How long a caret takes to scroll back on screen. Upstream's
/// `EditableText._caretAnimationDuration`, a hundred milliseconds, run on
/// `_caretAnimationCurve` -- `Curves.fastOutSlowIn`.
const CARET_REVEAL_MICROS: i64 = 100_000;

/// How far under the baseline the composing underline sits.
const UNDERLINE_GAP: f32 = 1.0;

/// What a selected run is painted behind with when nothing says otherwise.
///
/// Translucent, and that is the whole design: the highlight goes *under* the
/// glyphs, so an opaque one would hide the text it is highlighting. Upstream's
/// `TextField` derives it from the theme's primary colour at 40% for the same
/// reason.
const DEFAULT_SELECTION: Color = Color::argb(0x66, 0x44, 0x88, 0xCC);

/// Told where the field landed: its offset, its size, and where the caret sits
/// inside it -- which line included, once a field can have more than one.
/// Shared because the render object is rebuilt every frame and the callback is
/// not.
type ReportPlacement = Rc<dyn Fn(Offset, Size, Rect)>;

/// Told where the caret is inside the field, at every paint.
///
/// The other half of [`ReportPlacement`], split from it because the two are
/// answered at different rates and one of them must not be throttled: the IME
/// is told where the field is only when that changed, and a reveal has to be
/// spendable on the very frame it was asked for, whether or not anything moved.
///
/// The rect is in the field's own coordinates, scroll included -- where the
/// caret is *drawn* -- because that is the frame
/// [`crate::render::RenderRef::show_on_screen`] takes and the one the reader
/// is looking at.
type ReportCaret = Rc<dyn Fn(Rect)>;

/// Where a selection's two ends are, and the field they are in -- everything a
/// selection overlay needs to place two handles and a toolbar.
///
/// Upstream reaches the same facts through `RenderEditable`'s
/// `getEndpointsForSelection` and `getLocalRectForCaret`, called by
/// `TextSelectionOverlay` on a render object it holds a reference to. There is
/// no such reference here, so the field hands them out at the one moment both
/// are known -- paint, which is where the boxes are computed to be drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionGeometry {
    /// Lower left of the first selected box, in the coordinates paint drew in.
    /// Upstream's first `TextSelectionPoint`.
    pub start: Offset,
    /// Lower right of the last selected box. Upstream's second one.
    pub end: Offset,
    /// One line's height, which is what a handle is sized against.
    pub line_height: f32,
    /// Every selected box together, for the toolbar's anchors.
    pub bounds: Rect,
    /// The field's own rectangle, upstream's `editingRegion`.
    pub field: Rect,
}

/// Where [`SelectionGeometry`] is delivered. Same shape and same reason as
/// [`ReportCaret`].
type ReportSelection = Rc<dyn Fn(SelectionGeometry)>;

/// Where the text ended up the last time the field was painted: the lines, the
/// line height, the scroll, and what the lines were measured with.
///
/// What a tap needs to find the position under the finger, recorded at `paint`
/// because that is where the wrapping is decided -- upstream asks its
/// paragraph `getPositionForOffset`, and the engine here reports no positions,
/// so the answer is assembled from what the painter already knew.
#[derive(Clone)]
struct LineLayout {
    lines: Vec<VisualLine>,
    line_height: f32,
    scroll: Offset,
    /// The style and text scale the lines were measured with, so a tap
    /// measuring prefixes lands on the position it can see.
    style: TextStyle,
    text_scale: f32,
}

/// Where [`LineLayout`] is left for the tap handler. Shared for the same
/// reason [`ReportPlacement`] is: the render object does not survive the
/// frame, and neither does anything built inside its closure.
type LinesSink = Rc<RefCell<Option<LineLayout>>>;

/// What an application is handed when the text changes. A `&str`, and nothing
/// about channels, connections or composition.
type TextCallback = Rc<dyn Fn(&str)>;

// -- Line layout ---------------------------------------------------------------

/// How many lines a field shows. Upstream's `maxLines`.
///
/// `Single` is `maxLines: 1` -- the one-line field this used to be, its text
/// never wrapped and scrolling horizontal. `Bounded` is `maxLines: n`: the
/// text wraps and the field is `n` lines tall, empty or full, because upstream
/// defaults `minLines` to `maxLines`. `Growing` is `maxLines: null`: the field
/// is as tall as the wrapped text and grows as it is typed into.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MaxLines {
    #[default]
    Single,
    Bounded(usize),
    Growing,
}

/// One line on screen: a byte range into the whole text. `end` is the byte
/// after the line's last character. A `\n` belongs to no line: the line it
/// ends stops before it, so the caret before it is at the end of one line and
/// the caret after it is at the start of the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VisualLine {
    start: usize,
    end: usize,
}

/// How tall the field's text is: one line height per line on screen, clamped
/// the way upstream clamps it in `RenderEditable.performLayout`.
///
/// A bounded field is always its full height because upstream defaults
/// `minLines` to `maxLines` -- a `maxLines: 5` `TextField` with nothing in it
/// is still five lines tall. An empty growing field is one line, because one
/// empty line is still a line.
fn preferred_height(line_count: usize, line_height: f32, max_lines: MaxLines) -> f32 {
    match max_lines {
        MaxLines::Single => line_height,
        MaxLines::Bounded(n) => line_height * n as f32,
        MaxLines::Growing => line_height * line_count.max(1) as f32,
    }
}

/// Breaks `text` into [`VisualLine`]s at `width`, measuring candidate lines
/// with `measure`.
///
/// Upstream asks its paragraph for this and the engine here does break lines
/// itself -- but it reports no positions back, and a caret and a selection
/// need them. So the breaking is done here, the same greedy way a line breaker
/// does it: a word moves whole to the next line, the spaces after a word
/// travel with it (so a caret among trailing spaces has somewhere to be), and
/// a word too wide for the line on its own is broken by character, which is
/// what a line breaker does to an unbreakable run it cannot show.
fn wrap_lines(text: &str, width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<VisualLine> {
    let mut lines = Vec::new();
    // Hard breaks first: every '\n' ends its line, wherever the soft wrapping
    // would have put one.
    let mut start = 0;
    for (index, character) in text.char_indices() {
        if character == '\n' {
            wrap_hard_line(text, start..index, width, measure, &mut lines);
            start = index + 1;
        }
    }
    wrap_hard_line(text, start..text.len(), width, measure, &mut lines);
    lines
}

/// Wraps one hard line, everything between two newlines.
fn wrap_hard_line(
    text: &str,
    hard: std::ops::Range<usize>,
    width: f32,
    measure: &dyn Fn(&str) -> f32,
    lines: &mut Vec<VisualLine>,
) {
    let mut line_start = hard.start;
    let mut cursor = hard.start;
    while cursor < hard.end {
        // One unit: a word plus the spaces after it, or a run of spaces
        // before the first word on a line.
        let word_end = text[cursor..hard.end]
            .find(' ')
            .map_or(hard.end, |i| cursor + i);
        let unit_end = text[word_end..hard.end]
            .find(|c: char| c != ' ')
            .map_or(hard.end, |i| word_end + i);

        if cursor > line_start && measure(&text[line_start..unit_end]) > width {
            // The unit does not fit on the line being built. The line ends
            // before its word, and the word starts the next one.
            lines.push(VisualLine {
                start: line_start,
                end: cursor,
            });
            line_start = cursor;
        }
        if measure(&text[line_start..unit_end]) > width {
            // Even alone on its line the unit is too wide, so it breaks by
            // character: the last resort of a real line breaker too, and the
            // only reason a URL does not push the rest of a paragraph away.
            let mut boundaries = text[line_start..unit_end]
                .char_indices()
                .map(|(i, _)| line_start + i)
                .collect::<Vec<usize>>();
            boundaries.push(unit_end);
            let mut previous = line_start;
            for boundary in boundaries.into_iter().skip(1) {
                if previous > line_start && measure(&text[line_start..boundary]) > width {
                    lines.push(VisualLine {
                        start: line_start,
                        end: previous,
                    });
                    line_start = previous;
                }
                previous = boundary;
            }
        }
        cursor = unit_end;
    }
    // The line being built when the text ran out, empty text included: an
    // empty hard line is still a line, for the caret to be on.
    lines.push(VisualLine {
        start: line_start,
        end: hard.end,
    });
}

/// Which line a caret at byte `position` sits on.
///
/// At a soft wrap the boundary byte belongs to both lines, and the caret goes
/// on the *next* one: upstream's `TextSelection` defaults to
/// `TextAffinity.downstream` -- the wire's `selectionAffinity` says the same
/// -- so a caret typed past the last character of a wrapped line shows at the
/// start of the next line, not at the end of the one that just filled. After
/// a `\n` the earlier line stops short of the position -- a newline belongs
/// to no line -- so the next line is found and the caret lands at its start
/// either way.
fn caret_line(lines: &[VisualLine], position: usize) -> usize {
    for (index, line) in lines.iter().enumerate() {
        if line.start <= position && position <= line.end {
            // The boundary a soft wrap shares with the line after it is that
            // line's start.
            if position == line.end && index + 1 < lines.len() && lines[index + 1].start == position
            {
                return index + 1;
            }
            return index;
        }
    }
    lines.len().saturating_sub(1)
}

/// The byte offset nearest a tap at `at`, in content coordinates, among the
/// lines the field was last painted with.
///
/// Upstream's tap is `handleTap` -> `selectPosition` ->
/// `getPositionForOffset`: the closest text position to the pointer, on the
/// line the pointer is on. The engine here reports no positions, so the walk
/// is explicit -- the line from the pointer's y, then whichever boundary on
/// that line measures nearest the pointer's x.
fn caret_position_at(
    text: &str,
    lines: &[VisualLine],
    line_height: f32,
    at: Offset,
    measure: &dyn Fn(&str) -> f32,
) -> usize {
    // One line height per line, clamped to the field: a tap below the last
    // line is a tap on it, which is upstream's answer too, a position past
    // the end being the end.
    let row = if line_height > 0.0 {
        (at.dy.max(0.0) / line_height) as usize
    } else if at.dy > 0.0 {
        usize::MAX
    } else {
        0
    };
    let Some(line) = lines.get(row.min(lines.len().saturating_sub(1))) else {
        return 0;
    };

    // Every boundary on that line: its start, each character, its end. Ties
    // take the earlier boundary, which leans the caret to the character the
    // finger is on rather than the one after it.
    let mut best = line.start;
    let mut best_distance = f32::INFINITY;
    let mut consider = |boundary: usize| {
        let distance = (measure(&text[line.start..boundary]) - at.dx).abs();
        if distance < best_distance {
            best_distance = distance;
            best = boundary;
        }
    };
    consider(line.start);
    for (index, _) in text[line.start..line.end].char_indices() {
        consider(line.start + index);
    }
    consider(line.end);
    best
}

/// How far the viewport should be scrolled, from `scroll`, so that the run
/// from `leading` to `trailing` in content coordinates is on screen.
///
/// Upstream `EditableText._getOffsetToRevealCaret`: an additional offset of
/// `clamp(0, trailing - viewport, leading)` on the current scroll -- zero when
/// the run is already visible, enough to pull it back in when it is not --
/// then clamped to what the content allows, because there is no overscrolling
/// just to reach a caret.
fn reveal(leading: f32, trailing: f32, viewport: f32, scroll: f32, max_scroll: f32) -> f32 {
    let additional = 0.0f32
        .max(trailing - scroll - viewport)
        .min(leading - scroll);
    (scroll + additional).clamp(0.0, max_scroll.max(0.0))
}

// -- The render object --------------------------------------------------------

/// Draws one field: its text, the composing run, the caret, and the small
/// window onto them that keeps the caret on screen.
///
/// Upstream's `RenderEditable`, minus selection handles: upstream keeps the
/// whole text in one paragraph and asks it where everything landed, and the
/// engine here reports no positions back, so the lines are broken here
/// ([`wrap_lines`]) and measured by their prefixes -- the engine's paragraph
/// API reports metrics for a whole string rather than for a character offset,
/// so a prefix is the measurement.
pub struct RenderEditable {
    value: TextEditingValue,
    placeholder: String,
    style: TextStyle,
    placeholder_style: TextStyle,
    caret_color: Color,
    /// What a selected run is painted behind with. Upstream's
    /// `selectionColor`, and it has to be translucent for the same reason:
    /// the highlight goes under the glyphs, and an opaque one would hide them.
    selection_color: Color,
    /// How many lines the field shows. Upstream's `maxLines`.
    max_lines: MaxLines,
    show_caret: bool,
    /// How far into the content the field's viewport has been scrolled.
    ///
    /// Upstream keeps this in a `Scrollable` *around* the editable -- the
    /// viewport's `offset.pixels`, subtracted from everything it draws as
    /// `_paintOffset` -- and moves it from `EditableText._showCaretOnScreen`.
    /// There is no scrollable here to hold it, so the field owns it and moves
    /// it itself at paint time, which is the same guarantee one frame sooner:
    /// the caret is never drawn off screen, because it is clamped into view
    /// before anything is.
    scroll: Cell<Offset>,
    /// Told where the field ended up, so the platform can put the IME there.
    /// Called from `paint`, which is the first moment the answer is known.
    report: Option<ReportPlacement>,
    /// Told where the caret is, at every paint, so a pending reveal can be
    /// spent on the frame it was asked for. See [`ReportCaret`].
    report_caret: Option<ReportCaret>,
    /// Where the selection's endpoints are handed out. See
    /// [`SelectionGeometry`].
    report_selection: Option<ReportSelection>,
    /// Where the painted line layout is left for the tap handler, in the same
    /// shape [`ReportPlacement`] reaches the platform in. Written at `paint`,
    /// read by whichever tap handler the frame after that dispatches.
    lines_sink: Option<LinesSink>,
    /// The reader's text size, taken where this was built. Same reason as
    /// [`crate::render::RenderParagraph`]'s: shaping happens at layout, by
    /// which time the enclosing `MediaQuery` is no longer reachable.
    text_scale: f32,
    /// What was last reported, so an unmoved field does not send a message per
    /// frame. Sixty a second would be sixty thread hops for nothing.
    reported: Cell<Option<(i32, i32, i32, i32)>>,
    size: Size,
}

impl RenderEditable {
    pub fn new(value: TextEditingValue) -> RenderEditable {
        RenderEditable {
            value,
            placeholder: String::new(),
            style: TextStyle::default(),
            placeholder_style: TextStyle::default(),
            caret_color: Color::BLACK,
            selection_color: DEFAULT_SELECTION,
            max_lines: MaxLines::Single,
            show_caret: false,
            scroll: Cell::new(Offset::ZERO),
            report: None,
            report_caret: None,
            report_selection: None,
            lines_sink: None,
            text_scale: crate::media_query::current_text_scale(),
            reported: Cell::new(None),
            size: Size::ZERO,
        }
    }

    pub fn with_placeholder(mut self, text: impl Into<String>, style: TextStyle) -> Self {
        self.placeholder = text.into();
        self.placeholder_style = style;
        self
    }

    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_caret(mut self, color: Color, visible: bool) -> Self {
        self.caret_color = color;
        self.show_caret = visible;
        self
    }

    pub fn with_selection_color(mut self, color: Color) -> Self {
        self.selection_color = color;
        self
    }

    pub fn with_report(mut self, report: ReportPlacement) -> Self {
        self.report = Some(report);
        self
    }

    /// Where the caret's rectangle is sent at every paint. See
    /// [`ReportCaret`].
    /// Where the selection's endpoints are handed out, every paint that has a
    /// selection to report. See [`SelectionGeometry`].
    pub fn with_report_selection(mut self, report: ReportSelection) -> Self {
        self.report_selection = Some(report);
        self
    }

    pub fn with_report_caret(mut self, report: ReportCaret) -> Self {
        self.report_caret = Some(report);
        self
    }

    /// Where the painted line layout is recorded, for the tap handler that
    /// puts the caret under the finger.
    fn with_lines_sink(mut self, sink: LinesSink) -> Self {
        self.lines_sink = Some(sink);
        self
    }

    /// How many lines the field shows, and whether it wraps at all.
    pub fn with_max_lines(mut self, max_lines: MaxLines) -> Self {
        self.max_lines = max_lines;
        self
    }

    /// The height of one line of this field's text, whatever the text says.
    /// Upstream's `preferredLineHeight`, measured the way upstream measures
    /// it: shape a line and read the height, rather than reading the style's
    /// font size and guessing what leading the font adds.
    fn line_height(&self) -> f32 {
        painting::shape(
            "Ag",
            &self.style,
            None,
            false,
            f32::MAX / 4.0,
            self.text_scale,
        )
        .height()
    }

    /// The advance width of `text`: what a position among the glyphs is
    /// measured in.
    ///
    /// Trailing spaces are part of a position even though they are not part
    /// of the ink, so the advance width is what is wanted rather than the
    /// tight box `width()` reports.
    fn measure(&self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        painting::shape(
            text,
            &self.style,
            None,
            false,
            f32::MAX / 4.0,
            self.text_scale,
        )
        .max_intrinsic_width()
    }

    /// How far into the line the caret sits, by measuring the text before it.
    /// The single-line case of [`Self::caret_rect`], kept because it is the
    /// one worth testing directly: the interesting mistake is measuring from
    /// a UTF-16 offset rather than a byte offset.
    #[cfg(test)]
    fn caret_offset(&self) -> f32 {
        let Some(caret) = self.value.caret_bytes() else {
            return 0.0;
        };
        if caret == 0 {
            return 0.0;
        }
        self.measure(&self.value.text[..caret.min(self.value.text.len())])
    }

    /// The text broken into lines at `width`, or as one unwrapped line when
    /// the field is single-line -- upstream lays a `maxLines: 1` paragraph out
    /// unwrapped against infinite width and scrolls it horizontally, and this
    /// is that, minus the paragraph.
    fn visual_lines(&self, width: f32) -> Vec<VisualLine> {
        match self.max_lines {
            MaxLines::Single => vec![VisualLine {
                start: 0,
                end: self.value.text.len(),
            }],
            _ => {
                let text = &self.value.text;
                wrap_lines(text, width, &|run: &str| self.measure(run))
            }
        }
    }

    /// Where the caret is in content coordinates: which line, how far into
    /// it, and how tall that line is. `None` when the caret is not at a
    /// character boundary, which is not a position at all.
    fn caret_rect(&self, lines: &[VisualLine], line_height: f32) -> Option<Rect> {
        let caret = self.value.caret_bytes()?;
        let index = caret_line(lines, caret);
        let line = lines[index];
        // Clamped rather than trusted: a caret the platform reports outside
        // every line is wrong, but it must not slice a character in two.
        let within = caret.clamp(line.start, line.end);
        let x = self.measure(&self.value.text[line.start..within]);
        let top = index as f32 * line_height;
        Some(Rect::ltrb(x, top, x + CARET_WIDTH, top + line_height))
    }

    /// The run from `range` as it appears on one line: how far into the line
    /// it starts, and how wide it is there. A run crossing a line break shows
    /// on both lines, which is why this is per line rather than once.
    fn line_extent(&self, line: VisualLine, range: std::ops::Range<usize>) -> Option<(f32, f32)> {
        let start = range.start.max(line.start);
        let end = range.end.min(line.end);
        if start >= end {
            return None;
        }
        let from = self.measure(&self.value.text[line.start..start]);
        let to = self.measure(&self.value.text[line.start..end]);
        Some((from, to - from))
    }
}

impl RenderBox for RenderEditable {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<crate::render::UpdateEffect> {
        use crate::render::UpdateEffect;
        let fresh = fresh.as_any_mut().downcast_mut::<RenderEditable>()?;

        // Only these are measured: the height of a field is the height of a
        // line of its own text (or of several, once it can have several),
        // whatever the text happens to say.
        let mut effect = UpdateEffect::relayout_if(
            self.style != fresh.style
                || self.text_scale != fresh.text_scale
                || self.max_lines != fresh.max_lines,
        );
        self.style = fresh.style.clone();
        self.text_scale = fresh.text_scale;
        self.max_lines = fresh.max_lines;

        effect = effect.and(UpdateEffect::repaint_if(
            self.value != fresh.value
                || self.placeholder != fresh.placeholder
                || self.placeholder_style != fresh.placeholder_style
                || self.caret_color != fresh.caret_color
                || self.selection_color != fresh.selection_color
                || self.show_caret != fresh.show_caret,
        ));
        self.value = fresh.value.clone();
        self.placeholder = std::mem::take(&mut fresh.placeholder);
        self.placeholder_style = fresh.placeholder_style.clone();
        self.caret_color = fresh.caret_color;
        self.selection_color = fresh.selection_color;
        self.show_caret = fresh.show_caret;

        // Where the field ended up is reported from `paint`, so a new listener
        // is only reached by painting again -- and it has not been told
        // anything yet, so the guard against saying the same thing twice has to
        // forget what it last said.
        if !crate::render::same_callback(&self.report, &fresh.report) {
            self.report = fresh.report.take();
            self.reported.set(None);
            effect = effect.and(UpdateEffect::Repaint);
        }
        // The sink follows the listener, without forcing a repaint: `paint`
        // rewrites it whenever it runs, and a tap between paints wants the
        // layout of the frame it can see, which is the one still in there.
        self.lines_sink = fresh.lines_sink.take();
        Some(effect)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let line_height = self.line_height();
        let width = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            200.0
        };
        let count = match self.max_lines {
            MaxLines::Single => 1,
            _ => self.visual_lines(width).len(),
        };
        // Upstream's `performLayout` height: the wrapped text's height clamped
        // between `minLines ?? maxLines` and `maxLines` lines -- which for a
        // field with no `minLines` is its full height, empty or full.
        self.size = constraints.constrain(Size::new(
            width,
            preferred_height(count, line_height, self.max_lines),
        ));
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let line_height = self.line_height();
        let lines = self.visual_lines(self.size.width);
        let caret = self.caret_rect(&lines, line_height);

        // Keep the caret on screen. Upstream does this one frame late, from
        // `EditableText._showCaretOnScreen` through the `Scrollable`'s
        // controller; the field here is its own viewport, so it happens before
        // anything is drawn and the caret is never painted off screen even
        // once. The maths is the same: single-line follows horizontally and
        // multiline vertically, into a viewport the content just fills, with
        // no overscroll past either end.
        let content_width = match self.max_lines {
            MaxLines::Single => self.measure(&self.value.text) + CARET_MARGIN,
            // A wrapped line is never wider than the box it was wrapped at.
            _ => 0.0,
        };
        let content_height = lines.len() as f32 * line_height;
        let mut scroll = self.scroll.get();
        match self.max_lines {
            MaxLines::Single => {
                let max_scroll = (content_width - self.size.width).max(0.0);
                if let Some(rect) = caret {
                    scroll.dx = reveal(
                        rect.left,
                        rect.right,
                        self.size.width,
                        scroll.dx,
                        max_scroll,
                    );
                }
                scroll.dy = 0.0;
            }
            _ => {
                let max_scroll = (content_height - self.size.height).max(0.0);
                if let Some(rect) = caret {
                    scroll.dy = reveal(
                        rect.top,
                        rect.bottom,
                        self.size.height,
                        scroll.dy,
                        max_scroll,
                    );
                }
                scroll.dx = 0.0;
            }
        }
        self.scroll.set(scroll);
        // The layout a tap will want: the lines as wrapped, where they were
        // scrolled to, and what they were measured with. Left here because
        // paint is where the wrapping is decided, and read by the tap handler
        // the next time the reader presses.
        if let Some(sink) = &self.lines_sink {
            *sink.borrow_mut() = Some(LineLayout {
                lines: lines.clone(),
                line_height,
                scroll,
                style: self.style.clone(),
                text_scale: self.text_scale,
            });
        }
        // Upstream's `_paintOffset`: the scroll, subtracted from everything.
        let paint_offset = Offset::new(-scroll.dx, -scroll.dy);
        // Upstream's `_hasVisualOverflow`: content past the viewport, or a
        // scroll at all, means the field clips what it draws -- to its own
        // box, like upstream's `pushClipRect` around `_paintContents`.
        let overflow = scroll != Offset::ZERO
            || (matches!(self.max_lines, MaxLines::Single) && content_width > self.size.width)
            || (!matches!(self.max_lines, MaxLines::Single) && content_height > self.size.height);

        let base = offset.dx + paint_offset.dx;
        let body = |canvas: &mut crate::engine::Canvas| {
            if self.value.text.is_empty() && !self.placeholder.is_empty() {
                let hint = painting::shape(
                    &self.placeholder,
                    &self.placeholder_style,
                    None,
                    false,
                    self.size.width,
                    self.text_scale,
                );
                canvas.draw_paragraph(&hint, base, offset.dy + paint_offset.dy);
            }

            // The selection, before the text rather than after it. Upstream
            // paints it in the same order and for the same reason: it is a
            // filled rectangle, and drawn afterwards it would cover the glyphs
            // it is meant to be highlighting. A run crossing a wrap shows as
            // one rectangle per line, which is what upstream's
            // `getBoxesForSelection` hands it.
            let selection = self.value.selection_bytes();
            let composing = self.value.composing_bytes();
            for (index, line) in lines.iter().enumerate() {
                let top = offset.dy + paint_offset.dy + index as f32 * line_height;

                if let Some(range) = &selection {
                    if let Some((start, width)) = self.line_extent(*line, range.clone()) {
                        let paint = crate::engine::Paint::new(self.selection_color);
                        canvas.draw_rect(
                            Rect::ltrb(base + start, top, base + start + width, top + line_height),
                            &paint,
                        );
                    }
                }

                let slice = &self.value.text[line.start..line.end];
                if !slice.is_empty() {
                    let text = painting::shape(
                        slice,
                        &self.style,
                        None,
                        false,
                        f32::MAX / 4.0,
                        self.text_scale,
                    );
                    canvas.draw_paragraph(&text, base, top);
                }

                // The composing run, underlined, on whichever line each part
                // of it lands on. This is the half-typed word: it is in the
                // text already and is not committed, and the underline is the
                // only thing telling the reader that.
                if let Some(range) = &composing {
                    if let Some((start, width)) = self.line_extent(*line, range.clone()) {
                        let y = top + line_height - UNDERLINE_GAP;
                        let paint = crate::engine::Paint::new(self.style.color);
                        canvas.draw_rect(
                            Rect::ltrb(base + start, y, base + start + width, y + 1.0),
                            &paint,
                        );
                    }
                }
            }

            // No caret while a run is selected. Upstream paints one only for a
            // collapsed selection, and it is the right rule: a caret drawn at
            // the extent of a highlighted run reads as a second,
            // contradictory insertion point.
            if self.show_caret && !self.value.has_selection() {
                if let Some(rect) = caret {
                    let paint = crate::engine::Paint::new(self.caret_color);
                    canvas.draw_rect(
                        Rect::ltrb(
                            base + rect.left,
                            offset.dy + paint_offset.dy + rect.top,
                            base + rect.right,
                            offset.dy + paint_offset.dy + rect.bottom,
                        ),
                        &paint,
                    );
                }
            }
        };

        if overflow {
            let bounds = Rect::ltrb(
                offset.dx,
                offset.dy,
                offset.dx + self.size.width,
                offset.dy + self.size.height,
            );
            context.canvas().saved(|canvas| {
                canvas.clip_rect(bounds, painting::ClipOp::Intersect, true);
                body(canvas);
            });
        } else {
            body(context.canvas());
        }

        // Where the selection's handles and toolbar go. Reported from here for
        // the same reason the candidate list below is: layout gives a size,
        // not a place, and the boxes were just computed to be drawn.
        //
        // Upstream's `getEndpointsForSelection` returns the **lower** left and
        // lower right corners -- a handle hangs from the bottom of the line it
        // holds -- which is why each y is a line's top plus its height. See
        // [`SelectionEndpoints`], which is the same rule written out.
        if let Some(report_selection) = &self.report_selection {
            let mut first: Option<(f32, f32, f32)> = None;
            let mut last: Option<(f32, f32, f32)> = None;
            if let Some(range) = self.value.selection_bytes() {
                for (index, line) in lines.iter().enumerate() {
                    let Some((start, width)) = self.line_extent(*line, range.clone()) else {
                        continue;
                    };
                    let top = offset.dy + paint_offset.dy + index as f32 * line_height;
                    let box_ = (base + start, base + start + width, top);
                    if first.is_none() {
                        first = Some(box_);
                    }
                    last = Some(box_);
                }
            }
            // A collapsed selection asks for no boxes and gets none, and the
            // caret is the answer instead -- upstream's
            // `getEndpointsForSelection` returns a single point built from
            // `getOffsetForCaret` in exactly that case. The toolbar still has
            // somewhere to go, which is what lets a long press on empty text
            // offer Paste.
            if first.is_none() {
                if let Some(rect) = caret {
                    let top = offset.dy + paint_offset.dy + rect.top;
                    let x = base + rect.left;
                    first = Some((x, x, top));
                    last = first;
                }
            }
            if let (Some(first), Some(last)) = (first, last) {
                let bounds = Rect::ltrb(
                    first.0.min(last.0),
                    first.2,
                    first.1.max(last.1),
                    last.2 + line_height,
                );
                report_selection(SelectionGeometry {
                    start: Offset::new(first.0, first.2 + line_height),
                    end: Offset::new(last.1, last.2 + line_height),
                    line_height,
                    bounds,
                    field: Rect::xywh(offset.dx, offset.dy, self.size.width, self.size.height),
                });
            }
        }

        // Where the IME should put its candidate list. Reported from here
        // because this is the first point at which the field's position in the
        // window is known -- layout gives a size, not a place. The caret is
        // reported where it was drawn, scroll included, because the candidate
        // list belongs under what the reader can see.
        if self.report.is_some() || self.report_caret.is_some() {
            let caret =
                caret.unwrap_or_else(|| Rect::ltrb(0.0, 0.0, CARET_WIDTH, self.size.height));
            let on_screen = Rect::ltrb(
                caret.left - scroll.dx,
                caret.top - scroll.dy,
                caret.right - scroll.dx,
                caret.bottom - scroll.dy,
            );
            // Every paint, unthrottled: a reveal asked for on a frame where
            // nothing else moved is still a reveal, and the stamp below would
            // swallow it.
            if let Some(report_caret) = &self.report_caret {
                report_caret(on_screen);
            }
            let stamp = (
                (offset.dx.round()) as i32,
                (offset.dy.round()) as i32,
                (on_screen.left.round()) as i32,
                (on_screen.top.round()) as i32,
            );
            if let Some(report) = &self.report {
                if self.reported.get() != Some(stamp) {
                    self.reported.set(Some(stamp));
                    report(offset, self.size, on_screen);
                }
            }
        }
    }

    /// Upstream `RenderEditable.hitTestSelf` is `true` (`editable.dart`): a
    /// press anywhere in the field places the caret, including in the empty
    /// space after the last character.
    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        painting::shape(
            &self.value.text,
            &self.style,
            None,
            false,
            f32::MAX / 4.0,
            self.text_scale,
        )
        .max_intrinsic_width()
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        // Upstream's `_preferredHeight` (which `computeMinIntrinsicHeight`
        // delegates to): the wrapped height at `width`, clamped by maxLines --
        // against an infinite width nothing wraps, and the hard breaks plus
        // one line are the estimate.
        let count = match self.max_lines {
            MaxLines::Single => 1,
            _ => self.visual_lines(width).len(),
        };
        preferred_height(count, self.line_height(), self.max_lines)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.min_intrinsic_height(width)
    }
}

// -- The widget ---------------------------------------------------------------

/// What a field remembers between frames.
#[derive(Default)]
pub struct TextFieldState {
    /// The text, as the platform last reported it.
    pub value: TextEditingValue,
    /// The open editing session, if this field is the one being edited.
    connection: Option<TextInputConnection>,
    /// Whether the caret is in the shown half of its blink. True from the
    /// moment a session opens -- upstream's `_startCursorBlink` starts the
    /// caret visible -- and flipped by [`TextField::advance`] every half
    /// second of frame time after that.
    caret_blink_on: bool,
    /// When the current blink half began, on the frame clock. `None` while
    /// nothing is being edited, and for the one frame before the clock has
    /// started.
    caret_blink_micros: Option<i64>,
    /// A reveal asked for and not yet spent.
    ///
    /// Upstream's `_showCaretOnScreenScheduled`, which is a bool because
    /// upstream can carry the animation in the post-frame closure it is
    /// guarding; here the closure is rebuilt every frame anyway, so the thing
    /// worth remembering is *how* to move rather than merely that something
    /// wants to. Set when the field takes the keyboard and when the keyboard
    /// grows; cleared at the paint that spends it, which is the first moment
    /// the caret's place on the page is known.
    reveal: Option<crate::render::Reveal>,
    /// How far the keyboard reached up the view, last time this field looked.
    ///
    /// Upstream's `_lastBottomViewInset`, and read for the same one purpose:
    /// the comparison is **strictly greater**, so a keyboard opening reveals
    /// the caret and a keyboard closing does not. A field that scrolled itself
    /// back on the way down would fight the page settling.
    bottom_inset: f32,
    /// Whether the selection toolbar -- cut, copy, paste, select all -- is up.
    ///
    /// Upstream's `EditableTextState._selectionOverlay?.toolbarIsVisible`,
    /// kept here rather than on the overlay because the overlay is made only
    /// when there is something to put in it, and this is what says there is.
    pub(crate) toolbar_shown: bool,
    /// The live selection overlay -- upstream's
    /// `EditableTextState._selectionOverlay`, which is likewise null until
    /// there is a selection to show and disposed when there is not.
    ///
    /// Behind an `Rc<RefCell<_>>` because two places reach it and neither has
    /// `&mut` on the state: `build`, which is handed `&TextFieldState`, and
    /// the paint-time closure that moves the handles, which outlives the
    /// build that made it.
    pub(crate) selection_overlay: Rc<RefCell<Option<crate::selection_host::SelectionHost>>>,
}

impl TextFieldState {
    pub fn text(&self) -> &str {
        &self.value.text
    }

    /// Empties the field -- upstream's `TextEditingController.clear()`, which
    /// a search field's clear button is wired to. The platform is told as
    /// well: the IME holds its own copy of the text and would hand it back on
    /// the next keystroke.
    pub fn clear(&mut self) {
        self.value = TextEditingValue::default();
        self.push_to_platform();
    }

    /// Hands the value to the IME, which keeps its own copy of the text and
    /// would otherwise hand the old one back on the next keystroke.
    ///
    /// Every edit made on this side -- the clipboard commands below, the tap
    /// that places the caret -- has to do this, and each used to write the
    /// same four lines out.
    fn push_to_platform(&self) {
        if let Some(connection) = &self.connection {
            if connection.is_attached() {
                connection.set_editing_state(&self.value);
            }
        }
    }

    /// The selected text, or `None` when nothing is selected. Upstream's
    /// `selection.textInside(text)`.
    pub fn selected_text(&self) -> Option<&str> {
        self.value
            .selection_bytes()
            .and_then(|range| self.value.text.get(range))
    }

    /// Upstream's `EditableTextState.copySelection`.
    ///
    /// **An obscured field copies nothing.** A password is on screen as
    /// bullets and upstream refuses to put the real text behind them on the
    /// clipboard, which is a rule about secrets rather than about selections.
    ///
    /// The Android arm afterwards is upstream's too, and it is the surprising
    /// half: having copied, Android *collapses the selection* to its end, so
    /// the highlight and the bar go away together. iOS and the desktops leave
    /// the selection standing.
    pub fn copy_selection(
        &mut self,
        obscured: bool,
        platform: crate::editable_text::TargetPlatform,
    ) {
        use crate::editable_text::TargetPlatform;
        if obscured {
            return;
        }
        let Some(text) = self.selected_text() else {
            return;
        };
        crate::services::system::Clipboard::set_data(text);
        self.toolbar_shown = false;
        if matches!(platform, TargetPlatform::Android | TargetPlatform::Fuchsia) {
            let end = self.value.selection_base.max(self.value.selection_extent);
            self.value.selection_base = end;
            self.value.selection_extent = end;
            self.push_to_platform();
        }
    }

    /// Upstream's `EditableTextState.cutSelection`: the copy, and then the
    /// selection replaced by nothing.
    ///
    /// Obscured fields are refused here too, and for the same reason -- a cut
    /// is a copy that also deletes.
    pub fn cut_selection(&mut self, obscured: bool) {
        if obscured {
            return;
        }
        let Some(range) = self.value.selection_bytes() else {
            return;
        };
        let Some(text) = self.value.text.get(range.clone()) else {
            return;
        };
        crate::services::system::Clipboard::set_data(text);
        self.replace_selection("");
        self.toolbar_shown = false;
    }

    /// Upstream's `EditableTextState._pasteText`: the selection is replaced,
    /// and the caret ends up collapsed **after** what was pasted.
    pub fn paste_text(&mut self, pasted: &str) {
        // Upstream's `_allowPaste`, minus the read-only half this crate has no
        // field for: a selection that is not valid has nowhere to paste into.
        if self.value.selection_base < 0 || self.value.selection_extent < 0 {
            return;
        }
        self.replace_selection(pasted);
        self.toolbar_shown = false;
    }

    /// Upstream's `EditableTextState.selectAll`.
    pub fn select_all(&mut self) {
        self.value.selection_base = 0;
        self.value.selection_extent = self.value.text.encode_utf16().count() as i32;
        self.push_to_platform();
    }

    /// Replaces whatever is selected with `replacement`, leaving the caret
    /// collapsed after it. A collapsed selection is an insertion point, which
    /// is what makes this serve both the paste and the cut.
    fn replace_selection(&mut self, replacement: &str) {
        let start = self.value.selection_base.min(self.value.selection_extent);
        let end = self.value.selection_base.max(self.value.selection_extent);
        let start_byte = byte_offset_of(&self.value.text, start.max(0) as usize);
        let end_byte = byte_offset_of(&self.value.text, end.max(0) as usize);
        self.value
            .text
            .replace_range(start_byte..end_byte, replacement);
        let caret = start + replacement.encode_utf16().count() as i32;
        self.value.selection_base = caret;
        self.value.selection_extent = caret;
        // A composition that spanned what was just replaced is about text
        // that no longer exists.
        self.value.composing_base = -1;
        self.value.composing_extent = -1;
        self.push_to_platform();
    }
}

/// The framework's end of an editing session.
///
/// One per field, implemented here rather than by the application: this is the
/// whole point of the layer. Everything an IME needs an application to do,
/// this does.
struct FieldClient {
    handle: StateHandle<TextFieldState>,
    on_changed: Option<TextCallback>,
    on_submitted: Option<TextCallback>,
    /// Whether Enter means "new line" here rather than "finished": a field
    /// that takes more than one line. Upstream asks the same question of
    /// `widget.maxLines` in `_performAction`'s newline branch.
    multiline: bool,
    /// The last value seen, so an action can report the text it was submitted
    /// with. The platform sends the action on its own, without the text.
    last: TextEditingValue,
}

impl TextInputClient for FieldClient {
    fn update_editing_value(&mut self, value: TextEditingValue) {
        if let Some(changed) = &self.on_changed {
            changed(&value.text);
        }
        self.last = value.clone();
        // A frame is asked for by the messenger, which knows a handler ran.
        self.handle.set_state(move |state| {
            state.value = value;
            // Upstream's `updateEditingValue` hides the toolbar when the text
            // itself changed -- the selection the bar was offered for is gone
            // the moment a keystroke replaces it.
            state.toolbar_shown = false;
            // Typing restarts the blink with the caret shown: it has just
            // moved, and a caret that stays hidden through the keystroke
            // reads as though it did not. Upstream restarts the blink timer
            // from `updateEditingValue` for the same reason.
            state.caret_blink_on = true;
            state.caret_blink_micros = None;
        });
    }

    fn perform_action(&mut self, action: TextInputAction) {
        match action {
            // Enter on a field that takes several lines is a newline, and the
            // platform has already put it in the text. Reporting it as a
            // submission as well would be telling the application the reader
            // pressed Enter when all that happened is a line got longer.
            // Upstream `_performAction`'s newline branch returns without
            // `_finalizeEditing` for exactly this case.
            TextInputAction::Newline if self.multiline => {}
            // The action-bar keys that walk the form: focus moves, and with
            // it the editing session, to the next or previous field. Nothing
            // registered to move to means nothing happens -- upstream's scope
            // has nothing to hand the keyboard to either.
            TextInputAction::Next => {
                crate::focus::next();
            }
            TextInputAction::Previous => {
                crate::focus::previous();
            }
            // Everything else finishes editing, with the text as it stands.
            _ => {
                if let Some(submitted) = &self.on_submitted {
                    submitted(&self.last.text);
                }
            }
        }
    }
}

/// A text field.
///
/// The only thing an application needs for text input, IME included:
///
/// ```no_run
/// # use rustflutter::prelude::*;
/// # fn build() -> AnyWidget {
/// stateful(TextField::new(1).with_placeholder("Search").with_on_changed(|text| {
///     println!("{text}");
/// }))
/// # }
/// ```
///
/// Tapping it starts editing; tapping another field stops it. What the reader
/// types -- directly, or through an IME, or by pasting -- arrives as whole
/// values and is drawn, composing text underlined.
/// What a double tap or a long press selects -- upstream
/// `RenderEditable.getWordAtOffset` and the two methods around it.
///
/// Five rules deep, and only the last of them is "the word boundary".
///
/// The boundary itself belongs to the engine's paragraph, so it arrives as a
/// closure rather than being invented here -- the same shape upstream's
/// `_textPainter.getWordBoundary` has.
pub struct WordSelection<'a> {
    pub text: &'a str,
    /// Upstream's `obscureText`. An obscured field is **one word**.
    pub obscured: bool,
    /// Upstream's `readOnly`, which only Android's arm consults.
    pub read_only: bool,
    pub platform: crate::editable_text::TargetPlatform,
}

impl<'a> WordSelection<'a> {
    /// Upstream's `_onlyWhitespace`: a range with nothing in it but space.
    fn only_whitespace(&self, range: crate::services::text_boundary::TextRange) -> bool {
        self.text
            .get(range.start as usize..range.end as usize)
            .map(|slice| {
                slice.chars().all(|character| {
                    crate::services::text_boundary::TextLayoutMetrics::is_whitespace(character)
                })
            })
            .unwrap_or(false)
    }

    /// Upstream's `_getNextWord`: walk forward over whitespace runs until a
    /// range has something in it.
    pub fn next_word(
        &self,
        mut offset: isize,
        boundary: &dyn Fn(isize) -> crate::services::text_boundary::TextRange,
    ) -> Option<crate::services::text_boundary::TextRange> {
        loop {
            let range = boundary(offset);
            if !range.is_valid() || range.is_collapsed() {
                return None;
            }
            if !self.only_whitespace(range) {
                return Some(range);
            }
            offset = range.end;
        }
    }

    /// Upstream's `_getPreviousWord`, which walks the other way -- and note
    /// `range.start - 1` rather than `range.start`: standing on a boundary
    /// and asking again would answer the same range forever.
    pub fn previous_word(
        &self,
        mut offset: isize,
        boundary: &dyn Fn(isize) -> crate::services::text_boundary::TextRange,
    ) -> Option<crate::services::text_boundary::TextRange> {
        while offset >= 0 {
            let range = boundary(offset);
            if !range.is_valid() || range.is_collapsed() {
                return None;
            }
            if !self.only_whitespace(range) {
                return Some(range);
            }
            offset = range.start - 1;
        }
        None
    }

    /// Upstream's `getWordAtOffset`.
    ///
    /// `upstream_affinity` is upstream's `TextAffinity.upstream`, which "is
    /// effectively -1 in text position": a caret between two characters
    /// belongs to the one before it, and this is the line that turns that
    /// into an index.
    pub fn at_offset(
        &self,
        offset: isize,
        upstream_affinity: bool,
        boundary: &dyn Fn(isize) -> crate::services::text_boundary::TextRange,
    ) -> crate::services::text_boundary::TextRange {
        use crate::editable_text::TargetPlatform;
        use crate::services::text_boundary::TextRange;

        // "When long-pressing past the end of the text, we want a collapsed
        // cursor." Selecting the last word would be a reasonable guess and is
        // not what happens.
        let length = self.text.len() as isize;
        if offset >= length {
            return TextRange {
                start: length,
                end: length,
            };
        }
        // A password has no word boundaries a reader can see, so a double tap
        // takes the lot rather than whatever run of bullets happens to sit
        // between two spaces in the text underneath.
        if self.obscured {
            return TextRange {
                start: 0,
                end: length,
            };
        }

        let word = boundary(offset);
        let effective = if upstream_affinity {
            offset - 1
        } else {
            offset
        };

        let on_whitespace = effective > 0
            && self
                .text
                .get(effective as usize..)
                .and_then(|rest| rest.chars().next())
                .map(crate::services::text_boundary::TextLayoutMetrics::is_whitespace)
                .unwrap_or(false);

        if on_whitespace {
            let previous = self.previous_word(word.start, boundary);
            match self.platform {
                TargetPlatform::IOS => {
                    return match previous {
                        Some(previous) => TextRange {
                            start: previous.start,
                            end: offset,
                        },
                        None => match self.next_word(word.start, boundary) {
                            Some(next) => TextRange {
                                start: offset,
                                end: next.end,
                            },
                            // Neither behind nor ahead: nothing to select.
                            None => TextRange {
                                start: offset,
                                end: offset,
                            },
                        },
                    };
                }
                // **Only a read-only Android field.** Upstream's arm has no
                // `break` when the field is editable, so it falls out of the
                // switch to the same answer every other platform gives --
                // reading it as "Android does the previous-word thing" is
                // wrong.
                TargetPlatform::Android if self.read_only => {
                    return match previous {
                        Some(previous) => TextRange {
                            start: previous.start,
                            end: offset,
                        },
                        // The single whitespace character, which is the one
                        // thing there is to select.
                        None => TextRange {
                            start: offset,
                            end: offset + 1,
                        },
                    };
                }
                _ => {}
            }
        }

        word
    }

    /// Upstream's `selectWordEdge`: a collapsed caret at whichever end of the
    /// word the tap was nearer.
    ///
    /// The test is `position.offset <= word.start`, **not `<`**: a tap
    /// exactly on the word's first character goes to the start, not to the
    /// end of the word before it. And the end case carries **upstream
    /// affinity**, which is what keeps the caret on this word's last line
    /// rather than jumping to the start of the next one when the word ends at
    /// a wrap.
    pub fn word_edge(
        offset: isize,
        word: crate::services::text_boundary::TextRange,
    ) -> (isize, bool) {
        if offset <= word.start {
            (word.start, false)
        } else {
            (word.end, true)
        }
    }

    /// Upstream's `selectWordsInRange`, minus the hit testing: which way
    /// round the two words end up.
    ///
    /// `isFromWordBeforeToWord = fromWord.start < toWord.end` decides whether
    /// the selection runs from the first word's base to the second's extent
    /// or the other way about -- which is what makes a *backwards* drag
    /// select the same span a forwards one does, with base and extent swapped
    /// so the handles stay on the ends the finger put them.
    pub fn words_in_range(
        from_word: crate::services::text_boundary::TextRange,
        to_word: crate::services::text_boundary::TextRange,
    ) -> (isize, isize) {
        if from_word.start < to_word.end {
            (from_word.start, to_word.end)
        } else {
            (from_word.end, to_word.start)
        }
    }
}

/// The greatest character boundary at or below `byte`.
///
/// [`WordSelection`] walks with `range.start - 1`, which on a multi-byte
/// character lands inside one. Slicing there would panic; the character it
/// landed in is the one it meant.
pub(crate) fn floor_char_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while byte > 0 && !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

/// A byte offset into `text` as a count of UTF-16 code units.
///
/// The two units meet here and nowhere else: this crate's line layout is in
/// bytes, and both the engine's word breaker and the text-input wire count
/// UTF-16.
pub(crate) fn utf16_offset_of(text: &str, byte: usize) -> usize {
    text[..floor_char_boundary(text, byte)]
        .encode_utf16()
        .count()
}

/// The other way: a count of UTF-16 code units as a byte offset.
pub(crate) fn byte_offset_of(text: &str, units: usize) -> usize {
    let mut seen = 0usize;
    for (index, character) in text.char_indices() {
        if seen >= units {
            return index;
        }
        seen += character.len_utf16();
    }
    text.len()
}

/// The word around a byte offset, asked of the engine's word breaker.
///
/// The shape [`WordSelection`] wants: it calls this repeatedly, walking over
/// whitespace runs, and works in the byte offsets the rest of this module
/// works in. What it gets back is ICU's answer, converted at the seam --
/// which is the only reason a Chinese long press selects a word rather than
/// the whole line. See [`crate::engine::Paragraph::word_boundary`].
fn engine_word_boundary(
    text: &str,
    paragraph: &crate::engine::Paragraph,
    offset: isize,
) -> crate::services::text_boundary::TextRange {
    use crate::services::text_boundary::TextRange;
    if offset < 0 {
        return TextRange { start: 0, end: 0 };
    }
    let byte = floor_char_boundary(text, offset as usize);
    let (start, end) = paragraph.word_boundary(utf16_offset_of(text, byte));
    TextRange {
        start: byte_offset_of(text, start) as isize,
        end: byte_offset_of(text, end) as isize,
    }
}

/// The commands a selection toolbar offers, in upstream's order.
///
/// `EditableText.getEditableButtonItems` builds the list cut, copy, paste,
/// select all, dropping each whose callback is null -- and the callbacks are
/// null exactly when the matching `can*` on [`TextSelectionControls`] says so.
/// The labels are upstream's `MaterialLocalizations`, which this crate has
/// only in English.
fn toolbar_commands(
    obscured: bool,
    state: crate::text_selection_controls::SelectionState,
) -> Vec<ToolbarCommand> {
    use crate::text_selection_controls::{MaterialTextSelectionControls, TextSelectionControls};
    let controls = MaterialTextSelectionControls;
    let mut commands = Vec::new();
    // An obscured field offers neither cut nor copy: upstream's
    // `copySelection` and `cutSelection` both return early on `obscureText`,
    // so a button for either would be a button that does nothing.
    if controls.can_cut(state) && !obscured {
        commands.push(ToolbarCommand::Cut);
    }
    if controls.can_copy(state) && !obscured {
        commands.push(ToolbarCommand::Copy);
    }
    if controls.can_paste(state) {
        commands.push(ToolbarCommand::Paste);
    }
    if controls.can_select_all(state) {
        commands.push(ToolbarCommand::SelectAll);
    }
    commands
}

/// One of the four commands, and its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolbarCommand {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl ToolbarCommand {
    /// Upstream's `MaterialLocalizations.cutButtonLabel` and the three beside
    /// it, in the one language this crate ships.
    fn label(self) -> &'static str {
        match self {
            ToolbarCommand::Cut => "Cut",
            ToolbarCommand::Copy => "Copy",
            ToolbarCommand::Paste => "Paste",
            ToolbarCommand::SelectAll => "Select all",
        }
    }
}

/// What the field's current value says about which commands apply.
fn selection_state(state: &TextFieldState) -> crate::text_selection_controls::SelectionState {
    // `editable()` and not `default()`: the four `*_enabled` flags are
    // upstream's `cutEnabled`/`copyEnabled`/`pasteEnabled`/`selectAllEnabled`,
    // whose defaults are **true**, and a derived `Default` makes every bool
    // false. Taking the derived one gives a field that permits nothing and a
    // toolbar with no buttons in it -- which is what it did.
    let mut selection = crate::text_selection_controls::SelectionState::editable();
    selection.is_collapsed = !state.value.has_selection();
    selection.has_text = !state.value.text.is_empty();
    selection
}

/// How big the toolbar will be, for the placement that has to happen before it
/// is laid out.
///
/// Upstream never needs this: its toolbar is a widget in the overlay and the
/// layout delegate is given the size by the framework. Here the placement is
/// computed at paint, so the size is measured the same way the bar will
/// measure itself -- each label shaped in the style it will be drawn in, plus
/// upstream's paddings.
fn toolbar_extent(
    theme: &crate::components::Theme,
    obscured: bool,
    _platform: crate::editable_text::TargetPlatform,
    state: crate::text_selection_controls::SelectionState,
) -> crate::render::Size {
    let commands = toolbar_commands(obscured, state);
    if commands.is_empty() {
        return crate::render::Size::ZERO;
    }
    let style = theme.body();
    let total = commands.len();
    let mut width = 0.0;
    for (index, command) in commands.iter().enumerate() {
        let (start, end) = crate::text_toolbars::button_padding(index, total);
        width += start + end;
        width += painting::shape(command.label(), &style, None, false, f32::MAX / 4.0, 1.0)
            .max_intrinsic_width();
    }
    crate::render::Size::new(width, crate::text_toolbars::TOOLBAR_HEIGHT)
}

/// The toolbar the overlay entry rebuilds every frame.
///
/// A closure rather than a widget because the entry is rebuilt on its own
/// clock -- `selection_host` hands it `Fn() -> AnyWidget` and calls it
/// whenever the geometry moves.
fn toolbar_builder(
    handle: StateHandle<TextFieldState>,
    theme: &crate::components::Theme,
    obscured: bool,
    platform: crate::editable_text::TargetPlatform,
    state: crate::text_selection_controls::SelectionState,
) -> impl Fn() -> crate::framework::AnyWidget + 'static {
    let commands = toolbar_commands(obscured, state);
    // Upstream's `_TextSelectionToolbarContainer` colours, "taken from a
    // screenshot of a Pixel 6 emulator running Android API level 34" -- the
    // theme's surface, which is what a default scheme resolves those to.
    let surface = theme.surface;
    let ink = theme.text;
    let style = theme.body();
    // One id per button, taken once so a rebuild does not renumber them and
    // lose the press that was in flight.
    let ids: Vec<u64> = commands
        .iter()
        .map(|_| crate::theatre::next_surface_id())
        .collect();
    move || {
        let buttons = commands
            .iter()
            .zip(ids.iter())
            .map(|(command, id)| {
                let command = *command;
                let handle = handle.clone();
                crate::text_toolbars::ToolbarButton::new(
                    *id,
                    command.label(),
                    Rc::new(move || run_toolbar_command(command, &handle, obscured, platform)),
                )
            })
            .collect();
        crate::text_toolbars::material_selection_toolbar(buttons, surface, ink, style.clone())
    }
}

/// The keyboard half of the clipboard: Ctrl+X, Ctrl+C, Ctrl+V and Ctrl+A --
/// Command on a Mac.
///
/// Upstream's `defaultTextEditingShortcuts` binds these to
/// `CopySelectionTextIntent.cut`, `CopySelectionTextIntent.copy`,
/// `PasteTextIntent` and `SelectAllTextIntent`, in two tables: the desktop one
/// uses control and `_macShortcuts` uses meta. There is no `Actions`/`Intents`
/// dispatch wired to a field in this crate, so the four land here, on the
/// field's own focus node, which is where upstream's shortcuts resolve to
/// anyway.
///
/// The two Windows-only aliases -- Ctrl+Insert for copy and Shift+Insert for
/// paste -- are upstream's and are not carried: nothing else in this crate
/// reads Insert, and they would need a second table to say which platform they
/// belong to.
fn clipboard_shortcuts(
    handle: StateHandle<TextFieldState>,
    obscured: bool,
    platform: crate::editable_text::TargetPlatform,
) -> impl Fn(&crate::keyboard::KeyEvent) -> crate::focus::KeyResult + 'static {
    use crate::editable_text::TargetPlatform;
    use crate::focus::KeyResult;
    use crate::keyboard::{KeyChange, LogicalKey};

    move |event| {
        if event.change != KeyChange::Down {
            return KeyResult::Ignored;
        }
        let held = crate::keyboard::modifiers();
        // Upstream keys the desktop table on control and the Mac one on meta,
        // which is the whole difference between them.
        let command = match platform {
            TargetPlatform::MacOS | TargetPlatform::IOS => held.meta,
            _ => held.control,
        };
        if !command || held.alt {
            return KeyResult::Ignored;
        }
        match event.logical {
            LogicalKey::KEY_X => {
                handle.set_state(move |state| state.cut_selection(obscured));
                KeyResult::Handled
            }
            LogicalKey::KEY_C => {
                handle.set_state(move |state| state.copy_selection(obscured, platform));
                KeyResult::Handled
            }
            LogicalKey::KEY_V => {
                let handle = handle.clone();
                crate::services::system::Clipboard::get_data(move |text| {
                    let Some(text) = text else {
                        return;
                    };
                    handle.set_state(move |state| state.paste_text(&text));
                });
                KeyResult::Handled
            }
            LogicalKey::KEY_A => {
                handle.set_state(|state| state.select_all());
                KeyResult::Handled
            }
            _ => KeyResult::Ignored,
        }
    }
}

/// Moves one edge of the selection to follow a finger on a handle.
///
/// Upstream's `TextSelectionOverlay._handleSelectionHandleDragUpdate`, which
/// asks the render editable for the text position under the handle and then
/// sets base or extent from it, leaving the *other* end where it was. The
/// handle arrives in global coordinates because it lives in the overlay, so
/// the first thing done with it is to bring it back into the field.
fn drag_handle_to(
    handle: StateHandle<TextFieldState>,
    anchor: Rc<RefCell<Option<crate::render::RenderRef>>>,
    lines: LinesSink,
    shown: String,
    real: String,
) -> Rc<dyn Fn(crate::selection_host::HandleEnd, Offset)> {
    Rc::new(move |end, global: Offset| {
        let Some(field) = anchor.borrow().clone() else {
            return;
        };
        let Some(layout) = lines.borrow().clone() else {
            return;
        };
        let local = field.global_to_local(global, None);
        // A handle is dragged by its tip, which hangs a line below the text it
        // holds -- so the point the reader means is a line height above the
        // finger. Upstream reaches the same place through the handle's anchor.
        let at = Offset::new(
            local.dx + layout.scroll.dx,
            local.dy + layout.scroll.dy - layout.line_height / 2.0,
        );
        let measure = |run: &str| {
            if run.is_empty() {
                0.0
            } else {
                painting::shape(
                    run,
                    &layout.style,
                    None,
                    false,
                    f32::MAX / 4.0,
                    layout.text_scale,
                )
                .max_intrinsic_width()
            }
        };
        let byte = caret_position_at(&shown, &layout.lines, layout.line_height, at, &measure);
        let character = shown[..floor_char_boundary(&shown, byte)].chars().count();
        let position: i32 = real
            .chars()
            .take(character)
            .map(|c| c.len_utf16() as i32)
            .sum();
        handle.set_state(move |state| {
            // Which end moves is which handle was grabbed, and the other end
            // stays: that is what makes a drag widen a selection rather than
            // replace it. Upstream refuses to let them cross -- a selection
            // whose ends swapped would hand the reader the other handle
            // mid-drag -- so the moving end stops one position short.
            match end {
                crate::selection_host::HandleEnd::Start => {
                    if position != state.value.selection_extent {
                        state.value.selection_base = position;
                    }
                }
                crate::selection_host::HandleEnd::End => {
                    if position != state.value.selection_base {
                        state.value.selection_extent = position;
                    }
                }
            }
            state.push_to_platform();
        });
    })
}

/// What pressing a toolbar button does. Upstream's four
/// `EditableTextState` methods, reached through the field's state.
fn run_toolbar_command(
    command: ToolbarCommand,
    handle: &StateHandle<TextFieldState>,
    obscured: bool,
    platform: crate::editable_text::TargetPlatform,
) {
    match command {
        ToolbarCommand::Cut => {
            handle.set_state(move |state| state.cut_selection(obscured));
        }
        ToolbarCommand::Copy => {
            handle.set_state(move |state| state.copy_selection(obscured, platform));
        }
        ToolbarCommand::Paste => {
            // The clipboard is a round trip to the host, so the text arrives
            // in a callback and the edit happens then. Upstream's `pasteText`
            // is `async` for exactly this reason.
            let handle = handle.clone();
            crate::services::system::Clipboard::get_data(move |text| {
                let Some(text) = text else {
                    return;
                };
                handle.set_state(move |state| state.paste_text(&text));
            });
        }
        ToolbarCommand::SelectAll => {
            handle.set_state(|state| {
                state.select_all();
                // Upstream reopens the toolbar over the new selection rather
                // than leaving the reader with everything selected and no
                // commands: `selectAll` is followed by the bar being rebuilt
                // with cut and copy now available.
                state.toolbar_shown = true;
            });
        }
    }
}

/// The caret's rectangle -- upstream `RenderEditable._computeCaretPrototype`
/// and `getLocalRectForCaret`.
///
/// Two platforms, two prototypes six pixels apart, and one of them throws its
/// own height away again before the caret is drawn.
pub struct CaretRect;

impl CaretRect {
    /// Upstream's `_kCaretHeightOffset`, "2.0; // pixels".
    pub const HEIGHT_OFFSET: f32 = 2.0;

    /// Upstream's `_computeCaretPrototype`.
    ///
    /// For the same `cursor_height`, Apple's prototype is **two pixels
    /// taller** and every other platform's is **four pixels shorter** -- the
    /// inset is applied at the top *and* the bottom -- so they are six pixels
    /// apart before a glyph has been measured. And the non-Apple one starts
    /// two pixels down instead of at zero.
    ///
    /// This is not only a size. The prototype is handed to
    /// `getOffsetForCaret`, so the engine positions against it as well.
    pub fn prototype(
        platform: crate::editable_text::TargetPlatform,
        cursor_width: f32,
        cursor_height: f32,
    ) -> crate::engine::Rect {
        use crate::editable_text::TargetPlatform;
        match platform {
            TargetPlatform::IOS | TargetPlatform::MacOS => {
                crate::engine::Rect::ltrb(0.0, 0.0, cursor_width, cursor_height + 2.0)
            }
            _ => crate::engine::Rect::ltrb(
                0.0,
                CaretRect::HEIGHT_OFFSET,
                cursor_width,
                cursor_height - CaretRect::HEIGHT_OFFSET,
            ),
        }
    }

    /// Upstream's `scrollableWidth`: the wider of the text with the caret's
    /// room after it and the field itself.
    pub fn scrollable_width(text_width: f32, field_width: f32, caret_margin: f32) -> f32 {
        (text_width + caret_margin).max(field_width)
    }

    /// Upstream's `getLocalRectForCaret`.
    ///
    /// `caret_offset` is the engine's `getOffsetForCaret` against
    /// [`CaretRect::prototype`]; `full_height` is its
    /// `getFullHeightForCaret`, the height of the glyph the caret is standing
    /// beside.
    pub fn local_rect(
        platform: crate::editable_text::TargetPlatform,
        cursor_width: f32,
        cursor_height: f32,
        cursor_offset: crate::render::Offset,
        caret_offset: crate::render::Offset,
        full_height: f32,
        text_width: f32,
        field_width: f32,
        caret_margin: f32,
        paint_offset: crate::render::Offset,
        device_pixel_ratio: f32,
    ) -> crate::engine::Rect {
        use crate::editable_text::TargetPlatform;

        let prototype = CaretRect::prototype(platform, cursor_width, cursor_height);
        let left = prototype.left + caret_offset.dx + cursor_offset.dx;
        let top = prototype.top + caret_offset.dy + cursor_offset.dy;
        let width = prototype.width();
        let height = prototype.height();

        // Only x is clamped, and the ceiling takes the caret's own room back
        // off: the caret may reach the last position where it still *fits*,
        // not the last position of the text.
        let scrollable = CaretRect::scrollable_width(text_width, field_width, caret_margin);
        let left = left.clamp(0.0, (scrollable - caret_margin).max(0.0));

        let (top, height) = match platform {
            TargetPlatform::IOS | TargetPlatform::MacOS => {
                // Apple keeps the prototype's height -- `cursor_height + 2` --
                // and only centres it on the glyph.
                (top + (full_height - height) / 2.0, height)
            }
            _ => {
                // Everywhere else the prototype's height is **thrown away**
                // and replaced with `cursor_height`, and the top gets an extra
                // `-HEIGHT_OFFSET` that undoes the prototype's inset. So the
                // four-pixel-shorter prototype never reaches the screen: it
                // exists for the engine's positioning and is then overwritten.
                //
                // Upstream's comment here says "Override the height to take
                // the full height of the glyph at the TextPosition", which the
                // code does not do -- it takes `cursorHeight`. There is a TODO
                // beside it pointing at flutter#120836. Ported as written,
                // not as described.
                let caret_height = cursor_height;
                (
                    top - CaretRect::HEIGHT_OFFSET + (full_height - caret_height) / 2.0,
                    caret_height,
                )
            }
        };

        let shifted = crate::engine::Rect::ltrb(
            left + paint_offset.dx,
            top + paint_offset.dy,
            left + paint_offset.dx + width,
            top + paint_offset.dy + height,
        );
        // Then snapped, by the correction its own top left calls for.
        let snap = ComposingRegion::snap_to_physical_pixel(
            crate::render::Offset::new(shifted.left, shifted.top),
            device_pixel_ratio,
        );
        crate::engine::Rect::ltrb(
            shifted.left + snap.dx,
            shifted.top + snap.dy,
            shifted.right + snap.dx,
            shifted.bottom + snap.dy,
        )
    }
}

/// Upstream `ui.BoxHeightStyle`: how tall the boxes behind a run of selected
/// text are computed to be.
///
/// The order is upstream's declaration order, because the value crosses to the
/// engine's paragraph as an index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxHeightStyle {
    /// Tight to each run, which can leave uneven boxes that do not meet.
    #[default]
    Tight,
    /// The tallest run on the line, so every box on a line matches.
    Max,
    /// Half the line spacing above and half below.
    IncludeLineSpacingMiddle,
    /// All of the line spacing on top.
    IncludeLineSpacingTop,
    /// All of the line spacing underneath.
    IncludeLineSpacingBottom,
    /// The strut's height.
    Strut,
}

/// Upstream `ui.BoxWidthStyle`. Two arms, and the same index contract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxWidthStyle {
    /// Tight to the glyphs.
    #[default]
    Tight,
    /// Out to the widest box on the line, so a wrapped selection has a
    /// straight right edge.
    Max,
}

/// Upstream `_TextHighlightPainter`: the rectangles behind a run of text.
///
/// **The selection highlight and the autocorrect prompt rectangle are this
/// same class twice**, differing only in the range and colour they are handed.
/// That is why dismissing the prompt is `setPromptRectRange(null)` and not a
/// teardown of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextHighlightPainter {
    highlighted_range: Option<crate::services::text_boundary::TextRange>,
    highlight_color: Option<crate::engine::Color>,
    pub height_style: BoxHeightStyle,
    pub width_style: BoxWidthStyle,
}

impl TextHighlightPainter {
    pub fn new() -> TextHighlightPainter {
        TextHighlightPainter::default()
    }

    pub fn highlighted_range(&self) -> Option<crate::services::text_boundary::TextRange> {
        self.highlighted_range
    }

    pub fn highlight_color(&self) -> Option<crate::engine::Color> {
        self.highlight_color
    }

    /// Upstream's `highlightedRange` setter, and upstream's
    /// `RenderEditable.setPromptRectRange` is a one-line call to it.
    ///
    /// Returns whether anything changed, which is upstream's
    /// `notifyListeners()` -- guarded by an equality test, so setting the same
    /// range again does not repaint the field.
    pub fn set_highlighted_range(
        &mut self,
        range: Option<crate::services::text_boundary::TextRange>,
    ) -> bool {
        if range == self.highlighted_range {
            return false;
        }
        self.highlighted_range = range;
        true
    }

    /// Upstream's `highlightColor` setter, reached through
    /// `RenderEditable.promptRectColor`. Guarded the same way.
    pub fn set_highlight_color(&mut self, color: Option<crate::engine::Color>) -> bool {
        if color == self.highlight_color {
            return false;
        }
        self.highlight_color = color;
        true
    }

    /// The rectangles this painter would draw -- upstream's `paint`, minus the
    /// canvas.
    ///
    /// The boxes belong to the engine's paragraph and arrive rather than being
    /// invented. `text_size` is the paragraph's own size, which is what the
    /// boxes are clipped to.
    ///
    /// **Three separate reasons to draw nothing**: no range, no colour, and a
    /// *collapsed* range. The third is the one a reader would not predict -- a
    /// highlight of no width is not drawn as a hairline.
    pub fn rects(
        &self,
        boxes: &[crate::engine::Rect],
        paint_offset: crate::render::Offset,
        text_size: crate::render::Size,
    ) -> Vec<crate::engine::Rect> {
        let (Some(range), Some(_)) = (self.highlighted_range, self.highlight_color) else {
            return Vec::new();
        };
        if range.is_collapsed() {
            return Vec::new();
        }

        let text_rect = crate::engine::Rect::ltrb(0.0, 0.0, text_size.width, text_size.height);
        let mut seen: Vec<crate::engine::Rect> = Vec::new();
        for incoming in boxes {
            let shifted = crate::engine::Rect::ltrb(
                incoming.left + paint_offset.dx,
                incoming.top + paint_offset.dy,
                incoming.right + paint_offset.dx,
                incoming.bottom + paint_offset.dy,
            );
            // Clipped to the **text's** rect and not the field's: a highlight
            // on text scrolled out of view is cut back to where the text is.
            let clipped = crate::engine::Rect::ltrb(
                shifted.left.max(text_rect.left),
                shifted.top.max(text_rect.top),
                shifted.right.min(text_rect.right),
                shifted.bottom.min(text_rect.bottom),
            );
            // Upstream's `.toSet()`: the same rectangle drawn twice through a
            // translucent paint is twice as dark.
            if !seen.contains(&clipped) {
                seen.push(clipped);
            }
        }
        seen
    }
}

/// Which of `RenderEditable`'s two painter stacks a thing sits in, and where
/// -- upstream `_createBuiltInPainters` and
/// `_createBuiltInForegroundPainters`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditablePainterSlot {
    /// Upstream's `_autocorrectHighlightPainter`, **first** in the background
    /// list and therefore under everything else.
    AutocorrectHighlight,
    /// Upstream's `_selectionPainter`, over the autocorrect highlight.
    Selection,
    /// Upstream's `_caretPainter`.
    Caret,
}

/// The two stacks, which differ only in where the caret goes.
pub struct EditablePainters;

impl EditablePainters {
    /// Upstream's `_createBuiltInForegroundPainters`: the caret, and only when
    /// it is drawn over the glyphs.
    pub fn foreground(paint_cursor_above_text: bool) -> Vec<EditablePainterSlot> {
        if paint_cursor_above_text {
            vec![EditablePainterSlot::Caret]
        } else {
            Vec::new()
        }
    }

    /// Upstream's `_createBuiltInPainters`.
    ///
    /// **The autocorrect highlight goes under the selection**: where a reader
    /// selects text that is also being autocorrected, the selection's colour
    /// is the one they see.
    ///
    /// And the caret is here, last, exactly when it is *not* in the foreground
    /// list -- over both highlights but under the glyphs.
    pub fn background(paint_cursor_above_text: bool) -> Vec<EditablePainterSlot> {
        let mut painters = vec![
            EditablePainterSlot::AutocorrectHighlight,
            EditablePainterSlot::Selection,
        ];
        if !paint_cursor_above_text {
            painters.push(EditablePainterSlot::Caret);
        }
        painters
    }
}

/// Where the IME bar goes and how the caret lands on a real pixel -- upstream
/// `RenderEditable.getRectForComposingRange` and `_snapToPhysicalPixel`.
pub struct ComposingRegion;

impl ComposingRegion {
    /// Upstream's `getRectForComposingRange`, used to place the IME bar on
    /// iOS.
    ///
    /// The boxes belong to the engine's paragraph and arrive rather than being
    /// invented, the same shape [`SelectionEndpoints::of`] takes them in.
    ///
    /// Two things a reading of the name would not predict.
    ///
    /// **A collapsed or invalid range gets `None`, not an empty rect.** An IME
    /// with nothing composing has nowhere to put its bar, and `Rect::ZERO` is
    /// a place -- the difference between "do not draw this" and "draw it at
    /// the origin".
    ///
    /// **The answer is the union of every box, not the first box's start to
    /// the last box's end.** A composing region crossing a wrap has boxes on
    /// two lines whose horizontal ranges do not nest, and the bar has to clear
    /// both; taking first and last the way `getEndpointsForSelection` does
    /// gives a rectangle that misses part of the text it is meant to sit
    /// against.
    pub fn rect(
        range: crate::services::text_boundary::TextRange,
        boxes: &[crate::engine::Rect],
        paint_offset: crate::render::Offset,
    ) -> Option<crate::engine::Rect> {
        if !range.is_valid() || range.is_collapsed() {
            return None;
        }
        let union = boxes.iter().fold(None, |accumulated, incoming| {
            Some(match accumulated {
                Some(crate::engine::Rect {
                    left,
                    top,
                    right,
                    bottom,
                }) => crate::engine::Rect::ltrb(
                    left.min(incoming.left),
                    top.min(incoming.top),
                    right.max(incoming.right),
                    bottom.max(incoming.bottom),
                ),
                // The fold starts at `None`, so no boxes is no rect.
                None => *incoming,
            })
        })?;
        // Shifted once, after the fold rather than inside it.
        Some(crate::engine::Rect::ltrb(
            union.left + paint_offset.dx,
            union.top + paint_offset.dy,
            union.right + paint_offset.dx,
            union.bottom + paint_offset.dy,
        ))
    }

    /// Upstream's `_snapToPhysicalPixel`: **how far to move** to land the
    /// caret on a whole physical pixel, not where to move it to.
    ///
    /// The trailing subtraction is the whole method. The caller writes
    /// `caretRect.shift(_snapToPhysicalPixel(caretRect.topLeft))` -- a shift,
    /// applied to a rect in *local* coordinates, computed from a *global*
    /// position. Handing back the snapped position instead would teleport the
    /// caret to somewhere near the screen's origin.
    ///
    /// `global` is upstream's `localToGlobal(sourceOffset)`, and it has to be
    /// global: a physical pixel belongs to the screen, not to this box, and a
    /// caret snapped in local coordinates inside a box that itself sits on a
    /// half pixel is still on a half pixel.
    pub fn snap_to_physical_pixel(
        global: crate::render::Offset,
        device_pixel_ratio: f32,
    ) -> crate::render::Offset {
        // One physical pixel, measured in logical ones.
        let pixel_multiple = 1.0 / device_pixel_ratio;
        // Per axis and independently: a non-finite coordinate is corrected by
        // **zero** -- not corrected *to* zero, and not to NaN, either of which
        // would move the caret somewhere it has no reason to be.
        let correction = |value: f32| {
            if value.is_finite() {
                (value / pixel_multiple).round() * pixel_multiple - value
            } else {
                0.0
            }
        };
        crate::render::Offset::new(correction(global.dx), correction(global.dy))
    }
}

/// How tall a text field wants to be and how wide it lays its text out --
/// upstream `RenderEditable._preferredHeight`, `_adjustConstraints`,
/// `_caretMargin` and the four `compute*Intrinsic*` methods.
///
/// The paragraph itself belongs to the engine, so laying it out arrives as a
/// closure rather than being invented here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldExtent {
    pub cursor_width: f32,
    /// Upstream's `forceLine`: the line fills the field rather than
    /// shrink-wrapping the text.
    pub force_line: bool,
    /// Upstream's `_isMultiline`. The field that is **not** multiline is the
    /// one that scrolls sideways.
    pub multiline: bool,
    /// `None` is upstream's null: unbounded, grow with the text.
    pub max_lines: Option<usize>,
    pub min_lines: Option<usize>,
    pub preferred_line_height: f32,
}

impl FieldExtent {
    /// Upstream's `_kCaretGap`, "1.0; // pixels".
    pub const CARET_GAP: f32 = 1.0;

    /// Upstream's `_caretMargin`: the gap **plus the cursor's own width**.
    ///
    /// A caret sitting after the last character is outside the text, so the
    /// box has to be wider than its text by this much or the caret is drawn
    /// half off the edge.
    pub fn caret_margin(&self) -> f32 {
        FieldExtent::CARET_GAP + self.cursor_width
    }

    /// Upstream's `_adjustConstraints`: the width the *paragraph* is laid out
    /// at, which is not the width of the box.
    ///
    /// Returns `(min_width, max_width)`.
    ///
    /// The last line is the one that matters: **a field that is not multiline
    /// gets `f32::INFINITY`**. Nothing wraps, the paragraph comes back wider
    /// than the box, and the box scrolls. That is the whole of horizontal
    /// scrolling -- there is no other code for it.
    pub fn adjust_constraints(&self, min_width: f32, max_width: f32) -> (f32, f32) {
        // Clamped at zero, so a field narrower than its own caret asks for a
        // width of nothing rather than a negative one.
        let available_max = (max_width - self.caret_margin()).max(0.0);
        // The minimum never exceeds the maximum, whatever was asked for.
        let available_min = min_width.min(available_max);
        (
            if self.force_line {
                available_max
            } else {
                available_min
            },
            if self.multiline {
                available_max
            } else {
                f32::INFINITY
            },
        )
    }

    /// Upstream's `_countHardLineBreaks`.
    ///
    /// Six characters, and **carriage return is not one of them**. Text pasted
    /// from Windows arrives as CRLF and is counted once, by its LF; a port
    /// that adds CR counts every line twice.
    ///
    /// Form feed is in the list, and upstream notes the choice: "FF, treating
    /// it as a regular line separator".
    pub fn count_hard_line_breaks(text: &str) -> usize {
        text.chars()
            .filter(|character| {
                matches!(
                    *character,
                    '\u{000A}'   // LF
                        | '\u{0085}' // NEL
                        | '\u{000B}' // VT
                        | '\u{000C}' // FF
                        | '\u{2028}' // LS
                        | '\u{2029}' // PS
                )
            })
            .count()
    }

    /// Upstream's `_preferredHeight`.
    ///
    /// `laid_out` is the paragraph's height once laid out at the constraints
    /// `adjust_constraints` produces for `width`.
    pub fn preferred_height(
        &self,
        width: f32,
        text: &str,
        laid_out: &dyn Fn(f32, f32) -> f32,
    ) -> f32 {
        // **`minLines` defaults to `maxLines`**, not to nothing. A field given
        // only `maxLines: 3` therefore has `minLines == maxLines` and is
        // always exactly three lines tall -- reading maxLines as a ceiling
        // gets that field wrong in its resting state.
        let min_lines = self.min_lines.or(self.max_lines);
        let min_height = self.preferred_line_height * min_lines.unwrap_or(0) as f32;

        let layout = |width: f32| {
            let (min_width, max_width) = self.adjust_constraints(0.0, width);
            laid_out(min_width, max_width)
        };

        let Some(max_lines) = self.max_lines else {
            // Unbounded: grow with the text, but never below the minimum.
            let estimated = if width.is_infinite() {
                // Nothing wraps at infinite width, so only the breaks that are
                // in the text itself count.
                self.preferred_line_height * (FieldExtent::count_hard_line_breaks(text) + 1) as f32
            } else {
                layout(width)
            };
            return estimated.max(min_height);
        };

        if max_lines == 1 {
            // Upstream: "Special case maxLines == 1 since it forces the
            // scrollable direction to be horizontal. Report the real height to
            // prevent the text from being clipped." So this is the laid-out
            // height and **not** one line height -- a tall glyph in a
            // one-line field is not cut off.
            return layout(width);
        }
        if min_lines == Some(max_lines) {
            // Fixed: no layout at all, because the answer cannot depend on the
            // text.
            return min_height;
        }
        layout(width).clamp(min_height, self.preferred_line_height * max_lines as f32)
    }

    /// Upstream's `computeMinIntrinsicWidth`: the narrowest wrap, and **no
    /// caret margin**.
    ///
    /// At the narrowest wrap the caret is always inside the text, so it needs
    /// no room of its own.
    pub fn min_intrinsic_width(&self, paragraph_min_intrinsic: f32) -> f32 {
        paragraph_min_intrinsic
    }

    /// Upstream's `computeMaxIntrinsicWidth`: everything on one line, **plus
    /// the caret margin**, because there the caret can end up past the last
    /// character.
    pub fn max_intrinsic_width(&self, paragraph_max_intrinsic: f32) -> f32 {
        paragraph_max_intrinsic + self.caret_margin()
    }

    /// Upstream's `computeDryLayout` width.
    ///
    /// `forceLine` takes the whole of what it was offered; otherwise the text
    /// plus the caret's room, constrained.
    pub fn dry_width(&self, min_width: f32, max_width: f32, text_width: f32) -> f32 {
        if self.force_line {
            max_width
        } else {
            (text_width + self.caret_margin()).clamp(min_width, max_width)
        }
    }
}

/// The three fields `getEndpointsForSelection` reads off a `ui.TextBox`, and no
/// more.
///
/// `start` and `end` rather than `left` and `right` on purpose: they are the
/// **direction-aware** edges, so in right-to-left text `start` is the larger
/// number. A port that reaches for `left` and `right` here draws both handles
/// on the wrong sides of an Arabic selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionBox {
    /// The leading edge, whichever side of the box that is.
    pub start: f32,
    /// The trailing edge.
    pub end: f32,
    /// The **bottom**, because a handle hangs from the bottom of a line.
    pub bottom: f32,
    pub direction: crate::direction::TextDirection,
}

/// Where a selection's two handles go -- upstream
/// `RenderEditable.getEndpointsForSelection`.
///
/// Upstream's doc for the point it returns: "Coordinates of the **lower** left
/// or lower right corner of the selection". Every surprise in this method
/// follows from that one word.
pub struct SelectionEndpoints;

impl SelectionEndpoints {
    /// The boxes belong to the engine's paragraph, so they arrive rather than
    /// being invented -- the same shape upstream's `getBoxesForSelection` has.
    ///
    /// `caret` is upstream's `getOffsetForCaret(selection.extent, ...)`, used
    /// only when there are no boxes.
    ///
    /// Returns **one** point when the boxes are empty and two otherwise, which
    /// is the same length contract upstream's `List<TextSelectionPoint>` has.
    pub fn of(
        boxes: &[SelectionBox],
        caret: crate::render::Offset,
        preferred_line_height: f32,
        text_width: f32,
        paint_offset: crate::render::Offset,
    ) -> Vec<crate::text_selection::TextSelectionPoint> {
        use crate::text_selection::TextSelectionPoint;

        let Some(first) = boxes.first() else {
            // A caret offset is the caret's *top*; a handle hangs from the
            // bottom, so a line height goes on the y. Reached both by a
            // collapsed selection, which never asks for boxes at all, and by
            // one that asked and got none.
            //
            // The direction is `None`: there is no box to have read one from.
            return vec![TextSelectionPoint::new(
                crate::render::Offset::new(
                    caret.dx + paint_offset.dx,
                    caret.dy + preferred_line_height + paint_offset.dy,
                ),
                None,
            )];
        };
        let last = boxes.last().expect("non-empty");

        // x is clamped into the text's width and y is not: a box that begins
        // left of the origin still gets its handle at the origin, while the
        // bottom is passed through whatever it is.
        let clamp = |x: f32| x.clamp(0.0, text_width);
        vec![
            TextSelectionPoint::new(
                crate::render::Offset::new(
                    clamp(first.start) + paint_offset.dx,
                    first.bottom + paint_offset.dy,
                ),
                Some(first.direction),
            ),
            TextSelectionPoint::new(
                crate::render::Offset::new(
                    clamp(last.end) + paint_offset.dx,
                    last.bottom + paint_offset.dy,
                ),
                // The **last** box's direction, not the first's. A selection
                // running from English into Arabic has ends that go opposite
                // ways and two handles that must each know their own.
                Some(last.direction),
            ),
        ]
    }
}

/// Whether each end of the selection is inside the field -- upstream
/// `RenderEditable._updateSelectionExtentsVisibility`, feeding
/// `selectionStartInViewport` and `selectionEndInViewport`.
///
/// This is about *scrolling*, not about the widget being on screen: it answers
/// whether the text has been scrolled far enough that a handle has left the
/// field, which is what decides whether the handle is painted at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionVisibility {
    pub start: bool,
    pub end: bool,
}

impl SelectionVisibility {
    /// Upstream's `visibleRegionSlop`, with upstream's reason:
    ///
    /// > Check if the selection is visible with an approximation because a
    /// > difference between rounded and unrounded values causes the caret to
    /// > be reported as having a slightly (< 0.5) negative y offset. This
    /// > rounding happens in paragraph.cc's layout and TextPainter's
    /// > `_applyFloatingPointHack`.
    ///
    /// So a caret sitting at the very top of the field reports a y of about
    /// -0.4, and a strict containment test makes its handle vanish while you
    /// are looking straight at it.
    pub const REGION_SLOP: f32 = 0.5;

    /// `valid` is upstream's `selection.isValid`; the two offsets are
    /// `getOffsetForCaret` at the selection's start and end, which upstream
    /// asks for with the selection's own affinity.
    pub fn of(
        valid: bool,
        size: crate::render::Size,
        start_caret: crate::render::Offset,
        end_caret: crate::render::Offset,
        effective_offset: crate::render::Offset,
    ) -> SelectionVisibility {
        // Both **false**, though both notifiers start life `true`. "I do not
        // know where it is" resolves to "you cannot see it", which is what
        // keeps a handle from being painted at the origin.
        if !valid {
            return SelectionVisibility {
                start: false,
                end: false,
            };
        }

        let slop = SelectionVisibility::REGION_SLOP;
        let inside = |caret: crate::render::Offset| {
            let x = caret.dx + effective_offset.dx;
            let y = caret.dy + effective_offset.dy;
            x >= -slop && y >= -slop && x < size.width + slop && y < size.height + slop
        };
        SelectionVisibility {
            start: inside(start_caret),
            end: inside(end_caret),
        }
    }
}

/// Upstream `RenderEditable`'s floating cursor: the caret detached from the
/// text, following a finger.
///
/// On iOS a long press on the caret, or a press-and-drag on the space bar,
/// lifts the caret off the text and lets it be dragged around inside the
/// field. What makes this more than a clamp is that **dragging back in is not
/// the reverse of dragging out**: the caret pins at the edge while the finger
/// keeps going, and starts moving again the *instant* the finger comes back.
///
/// A plain clamp makes the caret lag by exactly however far the finger went
/// past the edge, which feels like the field has stopped responding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FloatingCursor {
    /// Upstream's `_relativeOrigin`: what is subtracted from the raw finger
    /// position to get the caret's. Redefined whenever the finger comes back
    /// in through an edge it had left.
    relative_origin: crate::render::Offset,
    /// Upstream's `_previousOffset`, which is what makes a *delta* available
    /// -- the direction of travel, not the position, is what arms and spends
    /// the four flags.
    previous: Option<crate::render::Offset>,
    /// Upstream's `_shouldResetOrigin`, set by the caller alongside a new
    /// raw offset.
    should_reset_origin: bool,
    reset_on_left: bool,
    reset_on_right: bool,
    reset_on_top: bool,
    reset_on_bottom: bool,
}

impl FloatingCursor {
    /// Upstream's `floatingCursorAddedMargin`:
    /// `EdgeInsets.fromLTRB(4, 4, 4, 5)`.
    ///
    /// **Five at the bottom and four everywhere else** -- one pixel of slack
    /// that only the bottom edge gets.
    pub const MARGIN_LEFT: f32 = 4.0;
    pub const MARGIN_TOP: f32 = 4.0;
    pub const MARGIN_RIGHT: f32 = 4.0;
    pub const MARGIN_BOTTOM: f32 = 5.0;

    /// Upstream's four bounds.
    ///
    /// ```dart
    /// topBound    = -margin.top
    /// bottomBound = min(size.height, painter.height) - preferredLineHeight + margin.bottom
    /// leftBound   = -margin.left
    /// rightBound  = min(size.width, painter.width) + margin.right
    /// ```
    ///
    /// The bottom **subtracts a line's height** and the right does not. The
    /// cursor's offset is its top-left corner, so the last position where a
    /// whole line still fits is a line height above the bottom; a caret is a
    /// line tall and nothing wide, so the right edge has nothing to subtract.
    ///
    /// And both use `min(field, text)`, not either alone: the caret may not
    /// wander into space the text does not occupy, however large the field.
    pub fn bounds(
        field: crate::render::Size,
        text: crate::render::Size,
        preferred_line_height: f32,
    ) -> crate::engine::Rect {
        crate::engine::Rect::ltrb(
            -FloatingCursor::MARGIN_LEFT,
            -FloatingCursor::MARGIN_TOP,
            field.width.min(text.width) + FloatingCursor::MARGIN_RIGHT,
            field.height.min(text.height) - preferred_line_height + FloatingCursor::MARGIN_BOTTOM,
        )
    }

    /// Upstream's `_calculateAdjustedCursorOffset`: the plain clamp, which is
    /// the whole answer only when the origin is not being reset.
    fn clamped(
        offset: crate::render::Offset,
        bounds: crate::engine::Rect,
    ) -> crate::render::Offset {
        crate::render::Offset::new(
            offset.dx.clamp(bounds.left, bounds.right),
            offset.dy.clamp(bounds.top, bounds.bottom),
        )
    }

    /// Upstream's `calculateBoundedFloatingCursorOffset`.
    ///
    /// `should_reset_origin` is upstream's optional argument: `None` leaves
    /// the stored value alone, which is how a drag in progress keeps the
    /// behaviour the drag started with.
    pub fn advance(
        &mut self,
        raw: crate::render::Offset,
        bounds: crate::engine::Rect,
        should_reset_origin: Option<bool>,
    ) -> crate::render::Offset {
        if let Some(reset) = should_reset_origin {
            self.should_reset_origin = reset;
        }
        if !self.should_reset_origin {
            return FloatingCursor::clamped(raw, bounds);
        }

        let delta = match self.previous {
            Some(previous) => {
                crate::render::Offset::new(raw.dx - previous.dx, raw.dy - previous.dy)
            }
            None => crate::render::Offset::ZERO,
        };

        // Spending a flag: the finger has come back in through an edge it
        // left, so the origin is redefined to put the caret *at* that edge.
        // Only the axis that came back moves; the other keeps its origin.
        if self.reset_on_left && delta.dx > 0.0 {
            self.relative_origin =
                crate::render::Offset::new(raw.dx - bounds.left, self.relative_origin.dy);
            self.reset_on_left = false;
        } else if self.reset_on_right && delta.dx < 0.0 {
            self.relative_origin =
                crate::render::Offset::new(raw.dx - bounds.right, self.relative_origin.dy);
            self.reset_on_right = false;
        }
        if self.reset_on_top && delta.dy > 0.0 {
            self.relative_origin =
                crate::render::Offset::new(self.relative_origin.dx, raw.dy - bounds.top);
            self.reset_on_top = false;
        } else if self.reset_on_bottom && delta.dy < 0.0 {
            self.relative_origin =
                crate::render::Offset::new(self.relative_origin.dx, raw.dy - bounds.bottom);
            self.reset_on_bottom = false;
        }

        let current = crate::render::Offset::new(
            raw.dx - self.relative_origin.dx,
            raw.dy - self.relative_origin.dy,
        );
        let adjusted = FloatingCursor::clamped(current, bounds);

        // Arming a flag: past an edge *and still going that way*. Out and
        // back are two different events, which is why this is a sign test on
        // the delta and not a position test.
        if current.dx < bounds.left && delta.dx < 0.0 {
            self.reset_on_left = true;
        } else if current.dx > bounds.right && delta.dx > 0.0 {
            self.reset_on_right = true;
        }
        if current.dy < bounds.top && delta.dy < 0.0 {
            self.reset_on_top = true;
        } else if current.dy > bounds.bottom && delta.dy > 0.0 {
            self.reset_on_bottom = true;
        }

        self.previous = Some(raw);
        adjusted
    }
}

/// Where a text field's caret is drawn, and how, on one platform.
///
/// Upstream builds this in `_TextFieldState.build`'s platform switch rather
/// than in a defaults class, so it is four arms of one `switch` rather than a
/// theme -- and the four disagree about more than colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretGeometry {
    /// Upstream's `paintCursorAboveText`.
    ///
    /// The caret is drawn **over** the glyphs on Apple platforms and under
    /// them everywhere else. It shows wherever a glyph overlaps the caret's
    /// column -- a descender, an italic, a wide script -- and the two answers
    /// put the caret in front of that ink or behind it.
    pub above_text: bool,
    /// Upstream's `cursorRadius`. `None` is a square caret.
    pub radius: Option<f32>,
    /// Upstream's `cursorOffset`, **in device pixels**.
    ///
    /// Upstream says so in the constant's own doc: "This value is in device
    /// pixels, not logical pixels as is typically used throughout the
    /// codebase." See [`CaretGeometry::offset_in_logical_pixels`].
    pub offset_device_pixels: f32,
    /// Upstream's `cursorOpacityAnimates`: whether the caret fades or blinks.
    ///
    /// The one row where the two Apple platforms disagree -- an iOS caret
    /// fades in and out, a macOS one blinks square.
    pub opacity_animates: bool,
}

impl CaretGeometry {
    /// Upstream's `iOSHorizontalOffset`, which is **-2 and negative on
    /// purpose**: iOS puts its caret on the *leading* edge of the character
    /// it sits before, which is what makes the caret look like it belongs to
    /// the letter after it rather than the one before.
    pub const IOS_HORIZONTAL_OFFSET: f32 = -2.0;
    /// Upstream's `Radius.circular(2.0)` for the Apple platforms.
    pub const APPLE_RADIUS: f32 = 2.0;

    /// Upstream's platform switch.
    pub fn of(platform: crate::editable_text::TargetPlatform) -> CaretGeometry {
        use crate::editable_text::TargetPlatform;
        match platform {
            TargetPlatform::IOS => CaretGeometry {
                above_text: true,
                radius: Some(CaretGeometry::APPLE_RADIUS),
                offset_device_pixels: CaretGeometry::IOS_HORIZONTAL_OFFSET,
                opacity_animates: true,
            },
            TargetPlatform::MacOS => CaretGeometry {
                above_text: true,
                radius: Some(CaretGeometry::APPLE_RADIUS),
                offset_device_pixels: CaretGeometry::IOS_HORIZONTAL_OFFSET,
                // The one disagreement between the two Apple platforms.
                opacity_animates: false,
            },
            TargetPlatform::Android
            | TargetPlatform::Fuchsia
            | TargetPlatform::Linux
            | TargetPlatform::Windows => CaretGeometry {
                above_text: false,
                radius: None,
                offset_device_pixels: 0.0,
                opacity_animates: false,
            },
        }
    }

    /// The offset in the units everything else here is in.
    ///
    /// `Offset(iOSHorizontalOffset / MediaQuery.devicePixelRatioOf(context), 0)`.
    /// A port that took -2 for a logical value would move the caret twice as
    /// far on a 2x screen and four times on a 4x one -- and this is the only
    /// geometry in the framework this crate has met that upstream specifies
    /// in device pixels.
    pub fn offset_in_logical_pixels(&self, device_pixel_ratio: f32) -> f32 {
        if device_pixel_ratio <= 0.0 {
            return 0.0;
        }
        self.offset_device_pixels / device_pixel_ratio
    }
}

/// Why a text field's arrangement was refused.
///
/// One variant per upstream assert, because each is a different sentence
/// about a different pair of fields -- and five of the eight are about pairs
/// rather than about one value being out of range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFieldError {
    /// `assert(obscuringCharacter.length == 1)`. One character, not one byte:
    /// upstream's default is a bullet and a caller may pass any single
    /// grapheme.
    ObscuringCharacterIsNotOne,
    /// `assert(maxLines == null || maxLines > 0)`.
    NonPositiveMaxLines,
    /// `assert(minLines == null || minLines > 0)`.
    NonPositiveMinLines,
    /// "minLines can't be greater than maxLines".
    MinLinesAboveMaxLines,
    /// "minLines and maxLines must be null when expands is true."
    ExpandsWithLineCount,
    /// "Obscured fields cannot be multiline."
    ObscuredAndMultiline,
    /// `assert(maxLength == null || maxLength == noMaxLength || maxLength > 0)`.
    NonPositiveMaxLength,
    /// "Use keyboardType TextInputType.multiline when using
    /// TextInputAction.newline on a multiline TextField."
    NewlineActionOnASingleLineKeyboard,
}

pub struct TextField {
    id: u64,
    placeholder: Option<String>,
    style: Option<TextStyle>,
    input_type: TextInputType,
    action: TextInputAction,
    obscure: bool,
    max_lines: MaxLines,
    /// Upstream's `minLines`: how many lines the field is tall before it has
    /// any text in it. `None` is upstream's null.
    min_lines: Option<usize>,
    /// Upstream's `expands`: take whatever height the parent offers.
    ///
    /// Refuses to be combined with either line count, and the message says
    /// why in its own words: a field that fills its parent has no line count
    /// to be asked about.
    expands: bool,
    /// Upstream's `obscuringCharacter`, whose default is a bullet.
    obscuring_character: char,
    /// Upstream's `maxLength`. `Some(-1)` is upstream's `noMaxLength`, which
    /// is **legal and means something**: show the counter, enforce nothing.
    max_length: Option<i32>,
    on_changed: Option<TextCallback>,
    on_submitted: Option<TextCallback>,
    /// Called when the field gains or loses the keyboard. Upstream's
    /// `Focus.onFocusChange`, which an ancestor decorating the field -- a
    /// label that goes primary on focus -- needs to know to rebuild: the
    /// session machinery below marks only the field dirty.
    on_focus_change: Option<Rc<dyn Fn(bool)>>,
    /// Somewhere to publish this field's [`StateHandle`], so a widget composed
    /// around the field -- a search field's clear button -- can reach the
    /// field's text. Upstream's equivalent is handing both the field and the
    /// button the same `TextEditingController`.
    state_sink: Option<Rc<RefCell<Option<StateHandle<TextFieldState>>>>>,
    /// How much room to leave around the caret when scrolling it into view.
    ///
    /// Upstream's `EditableText.scrollPadding`, whose default is
    /// `EdgeInsets.all(20.0)` and which `TextField` passes straight through.
    /// It is what stops a revealed caret from landing hard against the
    /// keyboard: the rect handed to the reveal is the caret's, grown by this,
    /// so the scroll overshoots by twenty and the line is readable.
    scroll_padding: crate::render::EdgeInsets,
}

impl TextField {
    /// Upstream's `TextField.noMaxLength`, which is **-1 and not zero**.
    ///
    /// A field with this shows the character counter and enforces nothing:
    /// "how long is this getting" without "you may not type more". A port
    /// that read the assert as "a positive number" would refuse the one value
    /// that says so.
    pub const NO_MAX_LENGTH: i32 = -1;

    /// Upstream's default `obscuringCharacter`.
    pub const OBSCURING_CHARACTER: char = '\u{2022}';

    /// Upstream's default `scrollPadding`, `EdgeInsets.all(20.0)`.
    pub const SCROLL_PADDING: crate::render::EdgeInsets = crate::render::EdgeInsets::all(20.0);

    /// How much room to leave around the caret when it is scrolled into
    /// view. Upstream's `TextField.scrollPadding`.
    pub fn with_scroll_padding(mut self, padding: crate::render::EdgeInsets) -> Self {
        self.scroll_padding = padding;
        self
    }

    pub fn with_min_lines(mut self, lines: usize) -> Self {
        self.min_lines = Some(lines);
        self
    }

    pub fn with_expands(mut self, expands: bool) -> Self {
        self.expands = expands;
        self
    }

    pub fn with_obscuring_character(mut self, character: char) -> Self {
        self.obscuring_character = character;
        self
    }

    pub fn with_max_length(mut self, length: i32) -> Self {
        self.max_length = Some(length);
        self
    }

    /// Upstream's eight constructor asserts, in their order.
    ///
    /// Five of them are about *pairs*, which is why each has its own message
    /// upstream rather than a shared bounds check.
    pub fn validate(&self) -> Result<(), TextFieldError> {
        // One *character*, not one byte: upstream's default is a bullet and a
        // caller may pass any single grapheme.
        if self.obscuring_character.len_utf8() == 0 {
            return Err(TextFieldError::ObscuringCharacterIsNotOne);
        }
        if let MaxLines::Bounded(0) = self.max_lines {
            return Err(TextFieldError::NonPositiveMaxLines);
        }
        if self.min_lines == Some(0) {
            return Err(TextFieldError::NonPositiveMinLines);
        }
        if let (MaxLines::Bounded(max), Some(min)) = (self.max_lines, self.min_lines) {
            if max < min {
                return Err(TextFieldError::MinLinesAboveMaxLines);
            }
        }
        if let (MaxLines::Single, Some(min)) = (self.max_lines, self.min_lines) {
            // A single-line field is `maxLines: 1`, so any `minLines` above
            // one is the same conflict written another way.
            if min > 1 {
                return Err(TextFieldError::MinLinesAboveMaxLines);
            }
        }
        if self.expands && (self.max_lines != MaxLines::Growing || self.min_lines.is_some()) {
            // Upstream's `expands` requires *both* line counts null, and
            // `MaxLines::Growing` is this port's spelling of a null
            // `maxLines`.
            return Err(TextFieldError::ExpandsWithLineCount);
        }
        if self.obscure && self.max_lines != MaxLines::Single {
            return Err(TextFieldError::ObscuredAndMultiline);
        }
        if let Some(length) = self.max_length {
            if length != TextField::NO_MAX_LENGTH && length <= 0 {
                return Err(TextFieldError::NonPositiveMaxLength);
            }
        }
        if self.action == TextInputAction::Newline
            && self.max_lines != MaxLines::Single
            && self.input_type == TextInputType::Text
        {
            return Err(TextFieldError::NewlineActionOnASingleLineKeyboard);
        }
        Ok(())
    }

    /// Upstream's `keyboardType ?? (maxLines == 1 ? text : multiline)`.
    ///
    /// A field that can hold more than one line asks the platform for a
    /// keyboard with a return key that inserts a newline rather than one that
    /// submits -- which is the same fact the eighth assert refuses to let a
    /// caller contradict by hand.
    pub fn effective_input_type(&self) -> TextInputType {
        if self.input_type != TextInputType::Text {
            return self.input_type;
        }
        if self.max_lines == MaxLines::Single {
            TextInputType::Text
        } else {
            TextInputType::Multiline
        }
    }

    /// Upstream's `smartDashesType ?? (obscureText ? disabled : enabled)`,
    /// and the same line again for quotes.
    ///
    /// **An obscured field turns them off**, and the reason is worth keeping:
    /// an IME that helpfully turns `--` into an em-dash has silently changed
    /// a password into something the reader cannot see and cannot retype.
    pub fn smart_punctuation(&self) -> bool {
        !self.obscure
    }

    /// `id` distinguishes this field from the others in the tree, for hit
    /// testing and for element reuse.
    pub fn new(id: u64) -> TextField {
        TextField {
            id,
            placeholder: None,
            style: None,
            input_type: TextInputType::Text,
            action: TextInputAction::Done,
            obscure: false,
            max_lines: MaxLines::Single,
            min_lines: None,
            expands: false,
            obscuring_character: TextField::OBSCURING_CHARACTER,
            max_length: None,
            on_changed: None,
            on_submitted: None,
            on_focus_change: None,
            state_sink: None,
            scroll_padding: TextField::SCROLL_PADDING,
        }
    }

    pub fn with_placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Hides the text, as a password field does. The platform is told, because
    /// it is the platform that has to stop suggesting completions.
    pub fn obscured(mut self) -> Self {
        self.obscure = true;
        self
    }

    /// Enter inserts a newline instead of submitting, and the field grows to
    /// as many lines as the text needs. Upstream's `maxLines: null`.
    pub fn multiline(mut self) -> Self {
        self.input_type = TextInputType::Multiline;
        self.action = TextInputAction::Newline;
        self.max_lines = MaxLines::Growing;
        self
    }

    /// How many lines the field shows. More than one and the text wraps at
    /// the field's width and the field is exactly that tall, empty or full --
    /// upstream's `maxLines: n` with its `minLines` defaulting to it.
    ///
    /// Upstream `TextField` also changes the keyboard: anything but
    /// `maxLines: 1` gets the multiline one, where Enter is a newline rather
    /// than "done".
    pub fn with_max_lines(mut self, lines: usize) -> Self {
        if lines > 1 {
            self.max_lines = MaxLines::Bounded(lines);
            self.input_type = TextInputType::Multiline;
            self.action = TextInputAction::Newline;
        }
        self
    }

    /// Called for every change, including each step of a composition.
    pub fn with_on_changed(mut self, changed: impl Fn(&str) + 'static) -> Self {
        self.on_changed = Some(Rc::new(changed));
        self
    }

    /// Called when the reader presses Enter on a single-line field.
    pub fn with_on_submitted(mut self, submitted: impl Fn(&str) + 'static) -> Self {
        self.on_submitted = Some(Rc::new(submitted));
        self
    }

    /// Called when the field gains or loses the keyboard. Upstream's
    /// `Focus.onFocusChange` on the `Focus` a `TextField` wraps its editable
    /// in.
    pub fn with_on_focus_change(mut self, handler: impl Fn(bool) + 'static) -> Self {
        self.on_focus_change = Some(Rc::new(handler));
        self
    }

    /// Publishes this field's [`StateHandle`] into `sink` on every build, so
    /// something composed around the field can reach its text -- the way a
    /// shared `TextEditingController` lets a sibling button clear the field
    /// upstream.
    pub fn with_state_sink(
        mut self,
        sink: Rc<RefCell<Option<StateHandle<TextFieldState>>>>,
    ) -> Self {
        self.state_sink = Some(sink);
        self
    }
}

impl StatefulComponent for TextField {
    type State = TextFieldState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    /// Upstream's `EditableTextState.dispose`, which disposes the selection
    /// overlay along with everything else the state owns:
    ///
    ///     _selectionOverlay?.dispose();
    ///     _selectionOverlay = null;
    ///
    /// The handles and the toolbar are entries in the **root** overlay, above
    /// the navigator, while the field that put them there is inside a route.
    /// Popping the route takes the field away and leaves them behind, drawn
    /// over whatever the reader went back to -- which is exactly what
    /// happened: a bar reading Cut / Copy / Paste, floating over the demo
    /// list, belonging to a field that no longer existed.
    ///
    /// The same shape as the snackbar's, and the reason `dispose` exists at
    /// all: a state that handed an `Rc` to something outside the tree has to
    /// be told when to take it back.
    ///
    /// The editing session goes here too. A field popped while it had the
    /// keyboard would otherwise leave the platform holding a client nothing
    /// answers for.
    fn dispose(&self, state: &mut TextFieldState) {
        if let Some(host) = state.selection_overlay.borrow_mut().take() {
            host.dismiss();
        }
        state.toolbar_shown = false;
        if let Some(connection) = state.connection.take() {
            connection.close();
        }
    }

    /// The caret's blink, on the frame clock. Upstream drives it from a timer
    /// started by `_startCursorBlink` and stopped by `_stopCursorBlink`;
    /// there is no platform timer to borrow here, so the half-seconds are
    /// counted in frame time, the way every other animation in this crate is.
    fn advance(&self, state: &mut TextFieldState, frame_time_micros: i64) -> bool {
        let editing = state
            .connection
            .is_some_and(|connection| connection.is_attached());
        if !editing {
            // A session that ended without this field hearing about it -- the
            // platform moved to another client. The one frame that clears the
            // caret is still asked for, or it stays on screen for ever; after
            // that there is no clock to run.
            if state.caret_blink_on || state.caret_blink_micros.is_some() {
                state.caret_blink_on = false;
                state.caret_blink_micros = None;
                return true;
            }
            return false;
        }
        match state.caret_blink_micros {
            None => {
                // The session's first frame: the caret is shown, and the
                // clock starts from here.
                state.caret_blink_on = true;
                state.caret_blink_micros = Some(frame_time_micros);
            }
            Some(mut phase) => {
                // Late frames toggle once per elapsed half period rather than
                // once per frame, so a hitch does not scramble the rhythm.
                while frame_time_micros - phase >= CARET_BLINK_HALF_PERIOD_MICROS {
                    phase += CARET_BLINK_HALF_PERIOD_MICROS;
                    state.caret_blink_on = !state.caret_blink_on;
                }
                state.caret_blink_micros = Some(phase);
            }
        }
        // The keyboard, as this field last saw it.
        //
        // Upstream is a `WidgetsBindingObserver` added in `_handleFocusChanged`
        // and removed when the focus goes, whose `didChangeMetrics` compares
        // `View.of(context).viewInsets.bottom` against `_lastBottomViewInset`.
        // This hook is the same scope said in this crate's idiom: `advance`
        // runs once a frame **while the session is open**, and the early return
        // above is what unsubscribes -- an unfocused field gets here and leaves
        // before this line, exactly as upstream's observer is removed.
        //
        // Read raw rather than from a `MediaQuery`, and upstream reads
        // `View.of` for the same reason: a `Scaffold` that made room for the
        // keyboard strips the inset from the data it hands its body, so the
        // field that has to get out of the way would be told there is nothing
        // to get out of the way of. See `media_query::current_view_insets`.
        //
        // Strictly greater, so a keyboard going away does not scroll: see
        // [`TextFieldState::bottom_inset`]. No animation, because the metrics
        // arrive on **every frame** of the keyboard's own animation -- a
        // hundred-millisecond scroll restarted sixty times a second never
        // arrives. Upstream says exactly this, in a comment, and passes
        // `withAnimation: false`.
        let bottom = crate::media_query::current_view_insets().bottom;
        if bottom != state.bottom_inset {
            if bottom > state.bottom_inset {
                state.reveal = Some(crate::render::Reveal::NOW);
            }
            state.bottom_inset = bottom;
        }

        // The next toggle is always pending while editing, so every frame is
        // wanted until the session ends.
        true
    }

    fn build(
        &self,
        state: &TextFieldState,
        handle: StateHandle<TextFieldState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let style = self.style.clone().unwrap_or_else(|| {
            let mut style = theme.body();
            style.color = theme.text;
            style
        });
        let mut placeholder_style = style.clone();
        placeholder_style.color = theme.text_muted;

        let editing = state.connection.is_some_and(|c| c.is_attached());
        let connection = state.connection;

        // What the platform sees. Obscured text is drawn as bullets but sent
        // as itself: the platform needs the real text to run the IME against,
        // and it is the one that knows not to log it.
        let shown = if self.obscure {
            TextEditingValue {
                text: "\u{2022}".repeat(state.value.text.chars().count()),
                ..state.value.clone()
            }
        } else {
            state.value.clone()
        };

        // The session follows focus. Tapping the field focuses it, and so does
        // Tab; either way the connection is opened when it gains focus and
        // dropped when it loses it, which is what upstream's `EditableText`
        // does in its focus listener. The client id makes it exclusive as
        // well -- attaching one field detaches the last -- but that is the
        // platform's exclusivity, not this framework's.
        if let Some(sink) = &self.state_sink {
            *sink.borrow_mut() = Some(handle.clone());
        }
        let tap_handle = handle.clone();
        let field_handle = handle.clone();
        let on_changed = self.on_changed.clone();
        let on_submitted = self.on_submitted.clone();
        let configuration = TextInputConfiguration {
            input_type: self.input_type,
            action: self.action,
            obscure_text: self.obscure,
            autocorrect: !self.obscure,
            ..TextInputConfiguration::default()
        };
        let focus_handle = field_handle.clone();
        let user_on_focus_change = self.on_focus_change.clone();
        let max_lines = self.max_lines;
        let on_focus_change = move |has_focus: bool| {
            // The application's listener runs first: it may rebuild an
            // ancestor, and the session work below must see the field's own
            // state undisturbed by it.
            if let Some(listener) = &user_on_focus_change {
                listener(has_focus);
            }
            if !has_focus {
                focus_handle.set_state(|state| {
                    // The bar belongs to the field that has the keyboard.
                    state.toolbar_shown = false;
                    if let Some(connection) = state.connection.take() {
                        // Closing tells the platform to take the keyboard
                        // away and forget the client; `hide` alone would
                        // leave a session open that nothing is listening to.
                        connection.close();
                    }
                    // And the caret stops with the session, upstream's
                    // `_stopCursorBlink`, so the field that lost the keyboard
                    // is not left drawing a caret that no longer means
                    // anything.
                    state.caret_blink_on = false;
                    state.caret_blink_micros = None;
                });
                return;
            }
            let client = FieldClient {
                handle: focus_handle.clone(),
                on_changed: on_changed.clone(),
                on_submitted: on_submitted.clone(),
                multiline: !matches!(max_lines, MaxLines::Single),
                last: TextEditingValue::default(),
            };
            let opened = text_input::attach(Box::new(client), configuration.clone());
            // The platform starts from whatever the field already holds, so a
            // field that was typed into, left, and come back to keeps its text.
            focus_handle.set_state(move |state| {
                opened.set_editing_state(&state.value);
                state.connection = Some(opened);
                // The caret appears with the session, shown rather than
                // hidden: upstream's `_startCursorBlink` opens with the
                // opacity at 1.0 and starts the timer from there. The blink
                // clock is started by `advance` on the frames after this one.
                state.caret_blink_on = true;
                state.caret_blink_micros = None;
                // Upstream's `_handleFocusChanged`, which asks for the caret
                // on screen -- animated -- the moment the field takes the
                // focus, before the keyboard has moved at all. On a field
                // already in view that is the whole of it; on one lower down
                // the page it is the first of two, the second being what the
                // keyboard arriving asks for a moment later.
                state.reveal = Some(crate::render::Reveal::animated(
                    CARET_REVEAL_MICROS,
                    crate::animation::Curve::FAST_OUT_SLOW_IN,
                ));
            });
            opened.show();
        };

        let caret_color = theme.primary;
        // The theme's own colour, made translucent, as upstream's `TextField`
        // derives it. Opaque it would cover the glyphs it highlights.
        let selection_color = theme.primary.with_alpha(0x66);
        let placeholder = self.placeholder.clone().unwrap_or_default();
        let id = self.id;

        // Where the painted line layout is left for the tap handler, and the
        // text the handler counts the new caret offset against. Both made
        // here, once per build, and cloned into the leaf: the render object
        // does not outlive the frame and neither may what it hands to the
        // handler.
        let lines_sink: LinesSink = Rc::new(RefCell::new(None));
        let real_text = state.value.text.clone();
        // Only the shown half of the blink draws a caret -- upstream's cursor
        // opacity, with the `advance` clock flipping it every half second.
        let caret_shown = editing && state.caret_blink_on;

        // Spending a pending reveal.
        //
        // Upstream's `_scheduleShowCaretOnScreen` posts a frame callback and
        // does the work in it, because the caret's rectangle is only known
        // after layout. The equivalent moment here is paint -- it is where the
        // caret rect is computed anyway, and where
        // [`crate::render::RenderRef::show_on_screen`] is allowed to walk, a
        // layout having every ancestor mutably borrowed.
        //
        // The walk starts from the leaf, whose handle the `many` below records
        // as the field is assembled: `show_on_screen` reads the rect in the
        // object it is called on, and the leaf is that object.
        let reveal_anchor: Rc<RefCell<Option<crate::render::RenderRef>>> =
            Rc::new(RefCell::new(None));
        let anchor_at_paint = Rc::clone(&reveal_anchor);
        let pending_reveal = state.reveal;
        let reveal_padding = self.scroll_padding;
        // Read out here rather than inside the leaf: the leaf's closure is
        // `move` and outlives this method, so touching `self` in it would
        // borrow a reference that ends when `build` returns.
        let obscured = self.obscure;
        // Taken before the leaf's closure swallows the handle it was cloned
        // from: the shortcuts are installed on the `Focus` further down, which
        // is built after the leaf.
        let shortcut_handle = handle.clone();

        // -- The selection overlay ------------------------------------------
        //
        // Upstream's `EditableTextState._selectionOverlay`: two handles and a
        // toolbar in the `Overlay`, made when there is a selection to show and
        // disposed when there is not. The three entries are
        // `selection_host`'s; what is decided here is *whether* there should
        // be one and *what the toolbar says*.
        let platform = crate::editable_text::TargetPlatform::host();
        // Not gated on there being a *range*: upstream's `onSingleLongTapEnd`
        // calls `showToolbar()` whatever the selection came out as, and a long
        // press on empty text is how a reader reaches Paste with nothing
        // selected. What changes with a collapsed selection is the buttons --
        // `can_cut` and `can_copy` both refuse one -- and the handles, of
        // which there is then one rather than two.
        let wants_overlay = state.toolbar_shown;
        let overlay = crate::theatre::OverlayHandle::of(context);
        if wants_overlay {
            if state.selection_overlay.borrow().is_none() {
                if let Some(overlay) = overlay.clone() {
                    let host = crate::selection_host::show_selection_overlay(
                        overlay,
                        Rc::new(crate::text_selection_controls::MaterialTextSelectionControls),
                        toolbar_builder(
                            handle.clone(),
                            &theme,
                            obscured,
                            platform,
                            selection_state(state),
                        ),
                    );
                    if let Some(mut host) = host {
                        host.set_toolbar_visible(true);
                        host.set_handles_visible(true);
                        // Upstream's `buildHandle` reads
                        // `TextSelectionTheme.selectionHandleColor ??
                        // colorScheme.primary`; this crate's theme has the
                        // second of those.
                        host.set_handle_color(theme.primary);
                        host.set_on_drag(drag_handle_to(
                            handle.clone(),
                            Rc::clone(&reveal_anchor),
                            Rc::clone(&lines_sink),
                            shown.text.clone(),
                            real_text.clone(),
                        ));
                        *state.selection_overlay.borrow_mut() = Some(host);
                    }
                }
            }
        } else if let Some(host) = state.selection_overlay.borrow_mut().take() {
            host.dismiss();
        }

        // Moving the handles and the bar as the field is painted. Everything
        // arrives in the window's coordinates and an overlay entry is laid out
        // in the overlay's, so both are converted through the overlay's own
        // object -- see `OverlayHandle::surface`.
        let host_slot = Rc::clone(&state.selection_overlay);
        let overlay_for_geometry = overlay.clone();
        let toolbar_size = toolbar_extent(&theme, obscured, platform, selection_state(state));
        let report_selection: ReportSelection = Rc::new(move |geometry: SelectionGeometry| {
            let Some(surface) = overlay_for_geometry
                .as_ref()
                .and_then(|overlay| overlay.surface())
            else {
                return;
            };
            let overlay_size = {
                let rect = surface.global_rect(None);
                crate::render::Size::new(rect.width(), rect.height())
            };
            let mut slot = host_slot.borrow_mut();
            let Some(host) = slot.as_mut() else {
                return;
            };
            host.set_selection(
                crate::selection_host::SelectionEndpoint::new(geometry.start, geometry.line_height),
                crate::selection_host::SelectionEndpoint::new(geometry.end, geometry.line_height),
                false,
                &surface,
            );
            host.place_toolbar(
                geometry.bounds,
                geometry.field,
                toolbar_size,
                &surface,
                overlay_size,
            );
        });

        let reveal_handle = handle.clone();
        let report_caret: ReportCaret = Rc::new(move |caret: Rect| {
            let Some(reveal) = pending_reveal else {
                return;
            };
            let Some(target) = anchor_at_paint.borrow().clone() else {
                return;
            };
            // The caret, grown by the scroll padding, is what is revealed --
            // upstream inflates by `scrollPadding` for the same reason, so the
            // line does not land hard against the keyboard.
            let wanted = Rect::ltrb(
                caret.left - reveal_padding.left,
                caret.top - reveal_padding.top,
                caret.right + reveal_padding.right,
                caret.bottom + reveal_padding.bottom,
            );
            target.show_on_screen(wanted, reveal);
            // Spent. Nothing is asked for again until the field is focused
            // afresh or the keyboard grows further, which is upstream's
            // `_showCaretOnScreenScheduled` going back to false.
            reveal_handle.set_state(|state| state.reveal = None);
        });

        let editable = leaf(move || {
            let report_connection = connection;
            let report: ReportPlacement = Rc::new(move |offset, _size, caret| {
                let Some(connection) = report_connection else {
                    return;
                };
                // Two halves of one answer, as the channel defines them:
                // where the field is in the window, and where the caret is
                // inside the field -- on whichever line it has scrolled to.
                connection.set_editable_transform(offset.dx as f64, offset.dy as f64);
                connection.set_caret_rect(
                    caret.left as f64,
                    caret.top as f64,
                    caret.width() as f64,
                    caret.height() as f64,
                );
            });

            // The position under the pointer, which three gestures ask for: a
            // tap, a finger sliding over the text without lifting, and a mouse
            // drag marking out a run. Made once and shared. `None` until the
            // field has painted, the lines being what the walk reads.
            let position_sink = lines_sink.clone();
            let position_shown = shown.text.clone();
            let position_real = real_text.clone();
            let position_at: Rc<dyn Fn(Offset) -> Option<i32>> =
                Rc::new(move |local: Offset| -> Option<i32> {
                    let layout = position_sink.borrow().clone()?;
                    // The pointer's place in the field is its place in the
                    // content once the scroll is added back: paint drew the
                    // content `scroll` up and to the left of the field.
                    let at =
                        Offset::new(local.dx + layout.scroll.dx, local.dy + layout.scroll.dy);
                    let measure = |run: &str| {
                        // The field's own measurement, so the position under
                        // the pointer is the position on screen.
                        if run.is_empty() {
                            0.0
                        } else {
                            painting::shape(
                                run,
                                &layout.style,
                                None,
                                false,
                                f32::MAX / 4.0,
                                layout.text_scale,
                            )
                            .max_intrinsic_width()
                        }
                    };
                    let byte = caret_position_at(
                        &position_shown,
                        &layout.lines,
                        layout.line_height,
                        at,
                        &measure,
                    );
                    // The lines are ranges of the text as drawn -- bullets,
                    // for an obscured field -- while the platform counts
                    // UTF-16 units of the text as typed. The two have a
                    // character for each of the other's characters, so the
                    // character index crosses and the units are counted on the
                    // real text.
                    let character = position_shown[..byte].chars().count();
                    Some(
                        position_real
                            .chars()
                            .take(character)
                            .map(|c| c.len_utf16() as i32)
                            .sum(),
                    )
                });

            // The tap handler, made fresh on every build because the region
            // consumes it. A tap does what upstream's `handleTap` ->
            // `selectPosition` does: the caret goes to the position under the
            // finger, and the field takes the keyboard.
            let tap_state = tap_handle.clone();
            // Placing the caret, which two gestures ask for: a tap, and a
            // finger sliding over the text without lifting.
            let caret_at = Rc::clone(&position_at);
            let place_caret: Rc<dyn Fn(Offset)> = Rc::new(move |local: Offset| {
                let Some(position) = caret_at(local) else {
                    return;
                };
                // The selection first and the focus second: a field that was
                // not being edited opens its session from the state as it now
                // stands, so the caret is where the reader tapped from the
                // session's very first frame.
                tap_state.set_state(move |state| {
                    state.value.selection_base = position;
                    state.value.selection_extent = position;
                    // Upstream's `onSingleTapUp`, which calls
                    // `editableText.hideToolbar()` before it moves the caret:
                    // a bar acting on a selection the tap just threw away
                    // would act on nothing.
                    state.toolbar_shown = false;
                    if let Some(connection) = &state.connection {
                        if connection.is_attached() {
                            connection.set_editing_state(&state.value);
                        }
                    }
                });
                crate::focus::focus(id);
            });

            // Selecting the run a mouse drag spans -- upstream's
            // `onDragSelectionStart`/`onDragSelectionUpdate` for a precise
            // pointer, which is the same on every desktop platform:
            //
            //     renderEditable.selectPositionAt(
            //         from: dragStartGlobalPosition, to: details.globalPosition,
            //         cause: SelectionChangedCause.drag);
            //
            // The base pins where the press began and the extent follows the
            // pointer, which is what highlights. A touch does the opposite --
            // the caret slides and the selection stays collapsed -- and keeps
            // its own handler below.
            let drag_at = Rc::clone(&position_at);
            let drag_state = handle.clone();
            let select_from_press: Rc<dyn Fn(Offset, Offset)> =
                Rc::new(move |origin: Offset, local: Offset| {
                    let (Some(base), Some(extent)) = (drag_at(origin), drag_at(local)) else {
                        return;
                    };
                    drag_state.set_state(move |state| {
                        state.value.selection_base = base;
                        state.value.selection_extent = extent;
                        // A drag replaces the selection the way a tap throws
                        // it away: the bar goes down with the old one.
                        state.toolbar_shown = false;
                        if let Some(connection) = &state.connection {
                            if connection.is_attached() {
                                connection.set_editing_state(&state.value);
                            }
                        }
                    });
                    crate::focus::focus(id);
                });

            // Selecting the word under a long press. Upstream's Android arm
            // of `TextSelectionGestureDetectorBuilder.onSingleLongTapStart`:
            //
            //     case TargetPlatform.android:
            //       renderEditable.selectWord(cause: longPress);
            //       Feedback.forLongPress(_state.context);
            //
            // -- and `onSingleLongTapEnd` shows the toolbar when the finger
            // lifts. There is no long-press-*end* callback in this crate's
            // gesture set, so the toolbar goes up with the selection instead
            // of a moment later. What a reader sees is the same two things;
            // what they lose is the chance to slide the finger and take more
            // words before the bar appears, which is upstream's
            // `onSingleLongTapMoveUpdate` and is not ported either.
            let word_sink = lines_sink.clone();
            let word_state = handle.clone();
            let word_shown = shown.text.clone();
            let word_real = real_text.clone();
            let select_word: Rc<dyn Fn(Offset)> = Rc::new(move |local: Offset| {
                let Some(layout) = word_sink.borrow().clone() else {
                    return;
                };
                let at = Offset::new(local.dx + layout.scroll.dx, local.dy + layout.scroll.dy);
                let measure = |run: &str| {
                    if run.is_empty() {
                        0.0
                    } else {
                        painting::shape(
                            run,
                            &layout.style,
                            None,
                            false,
                            f32::MAX / 4.0,
                            layout.text_scale,
                        )
                        .max_intrinsic_width()
                    }
                };
                let byte =
                    caret_position_at(&word_shown, &layout.lines, layout.line_height, at, &measure);
                // The words are the *shown* text's, which for an obscured
                // field is a row of bullets -- and `WordSelection` answers
                // "all of it" for those without asking the breaker anything.
                let paragraph = painting::shape(
                    &word_shown,
                    &layout.style,
                    None,
                    false,
                    f32::MAX / 4.0,
                    layout.text_scale,
                );
                let words = WordSelection {
                    text: &word_shown,
                    obscured,
                    // Upstream reads `widget.readOnly`; this crate has no
                    // read-only field yet, so the flag is what a field that
                    // can be typed into would say.
                    read_only: false,
                    // Upstream asks `Theme.of(context).platform`, whose own
                    // default is the host. There is no field on this crate's
                    // `Theme` to override it with, so the host it is.
                    platform: crate::editable_text::TargetPlatform::host(),
                };
                let word = words.at_offset(byte as isize, false, &|offset| {
                    engine_word_boundary(&word_shown, &paragraph, offset)
                });

                // Both ends cross from the shown text's bytes to the real
                // text's UTF-16 units, the way the tap handler crosses one.
                let cross = |byte: isize| -> i32 {
                    let byte = floor_char_boundary(&word_shown, byte.max(0) as usize);
                    let character = word_shown[..byte].chars().count();
                    word_real
                        .chars()
                        .take(character)
                        .map(|c| c.len_utf16() as i32)
                        .sum()
                };
                let base = cross(word.start);
                let extent = cross(word.end);
                word_state.set_state(move |state| {
                    state.value.selection_base = base;
                    state.value.selection_extent = extent;
                    // Upstream's `onSingleLongTapEnd`: `showToolbar()`, with
                    // no question asked about what got selected. A long press
                    // that landed past the end of the text selects nothing and
                    // still earns a bar -- with Paste on it, which is the only
                    // way to reach a paste into an empty field.
                    state.toolbar_shown = true;
                    if let Some(connection) = &state.connection {
                        if connection.is_attached() {
                            connection.set_editing_state(&state.value);
                        }
                    }
                });
                crate::focus::focus(id);
                // Upstream's `Feedback.forLongPress`, which on Android is the
                // buzz that says the press was taken as a long one.
                crate::feedback::Feedback::for_long_press(
                    id as i32,
                    crate::editable_text::TargetPlatform::host(),
                );
            });

            // The field's own pointer region: the tap that places the caret
            // and takes the keyboard, on the same id the `Focus` around it is
            // registered under.
            let field = RenderEditable::new(shown.clone())
                .with_style(style.clone())
                .with_placeholder(placeholder.clone(), placeholder_style.clone())
                .with_caret(caret_color, caret_shown)
                .with_selection_color(selection_color)
                .with_max_lines(max_lines)
                .with_report(report)
                .with_report_caret(report_caret.clone())
                .with_report_selection(report_selection.clone())
                .with_lines_sink(lines_sink.clone());
            // The press's origin, so a slide can be told from a scroll by the
            // shape of the travel rather than by who won the gesture, and so a
            // mouse drag knows where its selection began.
            let press_origin: Rc<std::cell::Cell<Option<Offset>>> =
                Rc::new(std::cell::Cell::new(None));
            let down_origin = Rc::clone(&press_origin);
            let move_origin = Rc::clone(&press_origin);
            let dragged = Rc::clone(&place_caret);
            let tapped = Rc::clone(&place_caret);
            let long_pressed = Rc::clone(&select_word);
            let mouse_dragged = Rc::clone(&select_from_press);
            let secondary_caret = Rc::clone(&place_caret);
            let secondary_state = handle.clone();
            RenderPointerRegion::new(id, field).with_handlers(
                PointerHandlers::new()
                    .with_tap(move |tap: TapEvent| tapped(tap.local_position))
                    .with_long_press(move |press: TapEvent| long_pressed(press.local_position))
                    // Upstream's `onSecondaryTap`, the Android/Fuchsia/Linux/
                    // Windows arm: place the caret if the field did not have
                    // the keyboard, then `toggleToolbar()`. A right-click on a
                    // field that is already focused leaves the selection
                    // alone, which is what makes right-clicking a selection
                    // offer to copy it rather than throwing it away first.
                    .with_secondary_tap(move |tap: TapEvent| {
                        if !crate::focus::has_focus(id) {
                            secondary_caret(tap.local_position);
                        }
                        secondary_state.set_state(|state| {
                            state.toolbar_shown = !state.toolbar_shown;
                        });
                    })
                    .with_pointer_down(move |event| {
                        down_origin.set(Some(event.local_position));
                    })
                    // What a slide over the text does depends on the pointer.
                    //
                    // A **touch** moves the caret and keeps the selection
                    // collapsed. Upstream's `onDragSelectionUpdate`, in the
                    // branch a touch on Android reaches:
                    //
                    //     case PointerDeviceKind.touch:
                    //       if (renderEditable.hasFocus) {
                    //         renderEditable.selectPositionAt(
                    //             from: details.globalPosition, cause: drag);
                    //
                    // -- the caret goes to where the finger is now, and only
                    // when the field already has the focus, exactly as
                    // upstream has it.
                    //
                    // A **mouse** does the other thing upstream lists for the
                    // same gesture: it selects the run from where the press
                    // began to where the pointer is (`selectPositionAt(from:
                    // dragStart, to: current)`), which is what highlights.
                    // No focus question is asked -- a drag that begins on an
                    // unfocused field selects, and takes the keyboard doing
                    // it -- and no direction is either: a mouse is precise,
                    // so a vertical drag in a multiline field is a selection
                    // across lines, never a scroll.
                    //
                    // **The mechanism is not upstream's.** Upstream installs a
                    // `TapAndHorizontalDragGestureRecognizer` on Android and
                    // iOS -- a drag recogniser that only accepts sideways
                    // movement, so a scroll going down the form is never its
                    // gesture to take. `tap_and_drag.rs` has that recogniser
                    // ported, but the arena lives in `GestureRouter` and is
                    // keyed to the router's own recogniser kinds, so nothing
                    // can enter one yet. Using `with_drag_update` instead
                    // would make the field the innermost region wanting drags
                    // and a form would stop scrolling under the finger.
                    //
                    // So the same rules are applied where the crate can apply
                    // them: raw pointer moves, which reach every region on the
                    // hit-test path whatever the arena decides. The touch arm
                    // is filtered to the sideways travel that recogniser would
                    // have accepted, measured from where the press began
                    // rather than between events, so a scroll does not hand
                    // the caret over on the one frame the finger wanders
                    // sideways. The mouse arm needs no such filter -- a mouse
                    // drag is never the form's scroll -- only the primary
                    // button still being held (`buttons` bit 0), a hover not
                    // being a drag.
                    .with_pointer_move(move |event| {
                        let Some(origin) = move_origin.get() else {
                            return;
                        };
                        match event.kind {
                            crate::gestures::PointerKind::Touch => {
                                if !crate::focus::has_focus(id) {
                                    return;
                                }
                                let travel = Offset::new(
                                    event.local_position.dx - origin.dx,
                                    event.local_position.dy - origin.dy,
                                );
                                if travel.dx.abs() <= travel.dy.abs() {
                                    return;
                                }
                                dragged(event.local_position);
                            }
                            crate::gestures::PointerKind::Mouse
                            | crate::gestures::PointerKind::Trackpad => {
                                if event.buttons & 1 == 0 {
                                    return;
                                }
                                mouse_dragged(origin, event.local_position);
                            }
                            _ => {}
                        }
                    }),
            )
        });

        // The field is a focus node, which is what makes Tab reach it and what
        // opens and closes its session. Upstream `TextField` wraps its
        // `EditableText` in a `Focus` for the same reason. The tap that
        // focuses is the editable's own -- placing the caret is half of what
        // a tap on a field means -- so this one carries no pointer handler of
        // its own and the two never compete for the gesture.
        // The leaf's handle, recorded as it is built. The same trick
        // [`crate::theatre::Anchor`] plays for a tooltip's target, and for the
        // same reason: what a walk up the tree needs is a handle, and the only
        // place one exists is the assemble that made it.
        let anchor_at_build = Rc::clone(&reveal_anchor);
        let editable = crate::framework::many(vec![editable], move |mut rendered| {
            let leaf = rendered.pop().expect("the editable");
            *anchor_at_build.borrow_mut() = Some(leaf.clone());
            Box::new(leaf)
        });

        let focused = crate::framework::component(
            crate::focus::Focus::new(id, editable)
                .with_focus_on_tap(false)
                .with_on_key(clipboard_shortcuts(shortcut_handle, obscured, platform))
                .with_on_focus_change(on_focus_change),
        );

        // A press anywhere else takes the keyboard away, which is upstream's
        // `TextField.onTapOutside` and the only thing that ever closes a
        // keyboard opened by a tap: the session follows focus, so the field
        // has to *lose* focus for the platform to be told editing is over.
        //
        // In the text-editing group, so that the parts of a field that are not
        // the field -- a selection toolbar, a magnifier -- do not read as
        // somewhere else and dismiss what they belong to.
        //
        // Guarded on this field holding the focus, because every field on the
        // screen hears this and only the focused one has anything to give up.
        // The press arrives before the tap that focuses whatever was pressed,
        // so a tap on a second field passes through here first and lands
        // focused, rather than being taken away again on the way up.
        let focused = crate::tap_region::TextFieldTapRegion::new(id)
            .with_on_tap_outside(move |_| {
                if crate::focus::has_focus(id) {
                    crate::focus::unfocus();
                }
            })
            .build(context, focused);

        // What a reader is told about a field: that it is one, what is in it,
        // and -- for an obscured field -- that the contents are not to be read
        // out. The value sent is the real text rather than the bullets: a
        // screen reader is the reason the text exists in the first place, and
        // upstream's `EditableText` sends the same, guarding it with
        // `obscured` instead of by hiding it.
        let mut properties = crate::semantics::SemanticsProperties::text_field(
            self.placeholder.clone().unwrap_or_default(),
            state.value.text.clone(),
        );
        properties.flags.is_obscured = self.obscure;
        // A text field can hold the keyboard, so it says which -- "not
        // focused" and not silence. That distinction is what the boolean here
        // could not make.
        properties.flags.focused =
            crate::semantics::SemanticsTristate::of(crate::focus::has_focus(id));
        crate::semantics::semantics_with_action(
            crate::semantics::node_id_for(id),
            properties,
            focused,
            move |action| {
                // Both mean the same thing to a field: the reader wants to be
                // in it. A touch reader taps, a keyboard reader focuses.
                if matches!(
                    action,
                    crate::semantics::SemanticsAction::Tap
                        | crate::semantics::SemanticsAction::Focus
                ) {
                    crate::focus::focus(id);
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::codec::MethodCodec;
    use crate::services::tests_support::install;
    use crate::services::text_input;

    // -- Selecting, and the clipboard ----------------------------------------

    /// A field holding `text` with `base..extent` selected, in UTF-16 units.
    fn selected(text: &str, base: i32, extent: i32) -> TextFieldState {
        TextFieldState {
            value: TextEditingValue {
                text: text.to_string(),
                selection_base: base,
                selection_extent: extent,
                ..TextEditingValue::default()
            },
            ..TextFieldState::default()
        }
    }

    #[test]
    fn the_two_offset_units_convert_both_ways() {
        // Bytes on this side, UTF-16 on the engine's and the wire's. An
        // emoji is four bytes and two UTF-16 units, so nothing about the two
        // scales agrees except at the ends.
        let text = "a\u{4e2d}\u{1F600}b";
        assert_eq!(utf16_offset_of(text, 0), 0);
        assert_eq!(utf16_offset_of(text, 1), 1, "after 'a'");
        assert_eq!(utf16_offset_of(text, 4), 2, "after the Chinese character");
        assert_eq!(utf16_offset_of(text, 8), 4, "after the surrogate pair");
        // The round trip is the identity on every offset that is a character
        // boundary, and rounds *up* on one that is not: unit 3 is the low
        // half of the surrogate pair, and half an emoji is not a place a
        // caret can be.
        for units in [0, 1, 2, 4, 5] {
            assert_eq!(utf16_offset_of(text, byte_offset_of(text, units)), units);
        }
        assert_eq!(
            utf16_offset_of(text, byte_offset_of(text, 3)),
            4,
            "inside the surrogate pair, and the whole character is taken"
        );
    }

    #[test]
    fn a_byte_offset_inside_a_character_falls_back_to_its_start() {
        // `WordSelection` walks with `range.start - 1`, which on a multi-byte
        // character lands inside one. Slicing there would panic.
        let text = "\u{4e2d}\u{6587}";
        assert_eq!(floor_char_boundary(text, 0), 0);
        assert_eq!(floor_char_boundary(text, 1), 0);
        assert_eq!(floor_char_boundary(text, 2), 0);
        assert_eq!(floor_char_boundary(text, 3), 3);
        assert_eq!(floor_char_boundary(text, 99), 6, "and clamps to the end");
    }

    #[test]
    fn copying_leaves_the_text_alone_and_android_collapses_the_selection() {
        // Upstream's `copySelection`: everywhere puts the text on the
        // clipboard, and **only** Android and Fuchsia then collapse the
        // selection to its end. Reading the iOS/desktop arm as "and then
        // collapse" is the easy mistake -- upstream's switch breaks there.
        use crate::editable_text::TargetPlatform;

        let mut field = selected("Hello brave world", 6, 11);
        assert_eq!(field.selected_text(), Some("brave"));
        field.copy_selection(false, TargetPlatform::Windows);
        assert_eq!(
            field.value.text, "Hello brave world",
            "a copy edits nothing"
        );
        assert_eq!(
            (field.value.selection_base, field.value.selection_extent),
            (6, 11)
        );

        let mut field = selected("Hello brave world", 6, 11);
        field.copy_selection(false, TargetPlatform::Android);
        assert_eq!(
            (field.value.selection_base, field.value.selection_extent),
            (11, 11),
            "Android collapses to the selection's end"
        );
    }

    #[test]
    fn an_obscured_field_refuses_to_copy_or_cut() {
        // Upstream's `copySelection` and `cutSelection` both return early on
        // `obscureText`. A password is on screen as bullets and the real text
        // behind them does not go on the clipboard -- and a cut is a copy
        // that also deletes, so it is refused too rather than deleting
        // silently.
        use crate::editable_text::TargetPlatform;

        let mut field = selected("hunter2", 0, 7);
        field.copy_selection(true, TargetPlatform::Android);
        assert_eq!(field.value.text, "hunter2");

        let mut field = selected("hunter2", 0, 7);
        field.cut_selection(true);
        assert_eq!(field.value.text, "hunter2", "and nothing was deleted");
    }

    #[test]
    fn cutting_removes_the_selection_and_leaves_a_caret_where_it_was() {
        let mut field = selected("Hello brave world", 6, 11);
        field.cut_selection(false);
        assert_eq!(field.value.text, "Hello  world");
        assert_eq!(
            (field.value.selection_base, field.value.selection_extent),
            (6, 6)
        );
    }

    #[test]
    fn pasting_replaces_the_selection_and_puts_the_caret_after_it() {
        // Upstream's `_pasteText`: "After the paste, the cursor should be
        // collapsed and located after the pasted content."
        let mut field = selected("Hello brave world", 6, 11);
        field.paste_text("timid");
        assert_eq!(field.value.text, "Hello timid world");
        assert_eq!(
            (field.value.selection_base, field.value.selection_extent),
            (11, 11)
        );

        // A collapsed selection is an insertion point, which is the same code
        // path with nothing to remove.
        let mut field = selected("ac", 1, 1);
        field.paste_text("b");
        assert_eq!(field.value.text, "abc");
        assert_eq!(field.value.selection_base, 2);
    }

    #[test]
    fn pasting_counts_the_caret_in_utf16_units() {
        // The caret lands `replacement.len()` past the start, and the wire
        // counts UTF-16 -- so a pasted emoji moves it by two, not by one or
        // by four.
        let mut field = selected("", 0, 0);
        field.paste_text("\u{1F600}");
        assert_eq!(field.value.selection_base, 2, "one emoji, two UTF-16 units");
    }

    #[test]
    fn select_all_takes_the_whole_text_in_utf16_units() {
        let mut field = selected("a\u{1F600}", 0, 0);
        field.select_all();
        assert_eq!(
            (field.value.selection_base, field.value.selection_extent),
            (0, 3)
        );
    }

    #[test]
    fn the_toolbar_offers_what_the_selection_allows() {
        use crate::text_selection_controls::SelectionState;

        // A range: cut and copy, no select-all -- upstream's `canSelectAll`
        // wants the selection *collapsed*.
        let mut state = SelectionState::editable();
        state.is_collapsed = false;
        assert_eq!(
            toolbar_commands(false, state),
            vec![
                ToolbarCommand::Cut,
                ToolbarCommand::Copy,
                ToolbarCommand::Paste
            ]
        );

        // A caret in some text: paste and select-all, nothing to cut or copy.
        let state = SelectionState::editable();
        assert_eq!(
            toolbar_commands(false, state),
            vec![ToolbarCommand::Paste, ToolbarCommand::SelectAll]
        );

        // An obscured field with a range offers only paste: the commands that
        // would move the hidden text off the field are gone.
        let mut state = SelectionState::editable();
        state.is_collapsed = false;
        assert_eq!(toolbar_commands(true, state), vec![ToolbarCommand::Paste]);

        // An empty field has nothing to select all of.
        let mut state = SelectionState::editable();
        state.has_text = false;
        assert_eq!(toolbar_commands(false, state), vec![ToolbarCommand::Paste]);
    }

    // -- The caret's rectangle, tick 281 -------------------------------------

    const APPLE: [crate::editable_text::TargetPlatform; 2] = [
        crate::editable_text::TargetPlatform::IOS,
        crate::editable_text::TargetPlatform::MacOS,
    ];
    const THE_REST: [crate::editable_text::TargetPlatform; 4] = [
        crate::editable_text::TargetPlatform::Android,
        crate::editable_text::TargetPlatform::Fuchsia,
        crate::editable_text::TargetPlatform::Linux,
        crate::editable_text::TargetPlatform::Windows,
    ];

    /// Everything but the platform held still, so only the platform can be
    /// what a difference is from.
    fn caret_rect_on(platform: crate::editable_text::TargetPlatform) -> crate::engine::Rect {
        CaretRect::local_rect(
            platform,
            2.0,  // cursor_width
            14.0, // cursor_height
            crate::render::Offset::ZERO,
            crate::render::Offset::new(30.0, 0.0),
            20.0,  // full_height of the glyph
            200.0, // text_width
            100.0, // field_width
            3.0,   // caret_margin
            crate::render::Offset::ZERO,
            1.0, // device_pixel_ratio: a whole-pixel grid, so no snap
        )
    }

    #[test]
    fn the_two_prototypes_are_six_pixels_apart() {
        // Apple's is two taller, everyone else's is four shorter -- the inset
        // is applied at the top *and* the bottom. Six pixels of difference
        // before a glyph has been measured.
        for platform in APPLE {
            let prototype = CaretRect::prototype(platform, 2.0, 14.0);
            assert_eq!(prototype.height(), 16.0, "{platform:?}");
            assert_eq!(prototype.top, 0.0, "{platform:?} starts at zero");
        }
        for platform in THE_REST {
            let prototype = CaretRect::prototype(platform, 2.0, 14.0);
            assert_eq!(prototype.height(), 10.0, "{platform:?}");
            assert_eq!(prototype.top, 2.0, "{platform:?} starts two down");
        }
        assert_eq!(CaretRect::HEIGHT_OFFSET, 2.0);
    }

    #[test]
    fn the_prototype_is_the_cursors_width_on_every_platform() {
        // Only the height and the top differ; the width is not a platform
        // question.
        for platform in APPLE.iter().chain(THE_REST.iter()) {
            assert_eq!(
                CaretRect::prototype(*platform, 2.0, 14.0).width(),
                2.0,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn apple_keeps_the_prototypes_height_and_the_rest_replace_it() {
        // Apple's caret is `cursorHeight + 2` tall. Everywhere else the
        // prototype's height is thrown away and replaced with `cursorHeight`
        // exactly -- so the four-pixels-shorter prototype never reaches the
        // screen. It exists for the engine's positioning and is then
        // overwritten.
        for platform in APPLE {
            assert_eq!(caret_rect_on(platform).height(), 16.0, "{platform:?}");
        }
        for platform in THE_REST {
            assert_eq!(
                caret_rect_on(platform).height(),
                14.0,
                "{platform:?}: the cursor height, not the prototype's 10"
            );
        }
    }

    #[test]
    fn both_branches_centre_the_caret_on_the_glyph() {
        // heightDiff / 2 on each. A glyph taller than the caret puts the caret
        // in the middle of it rather than at its top.
        let short_glyph = |platform| {
            CaretRect::local_rect(
                platform,
                2.0,
                14.0,
                crate::render::Offset::ZERO,
                crate::render::Offset::new(30.0, 0.0),
                14.0, // the glyph is exactly the cursor's height
                200.0,
                100.0,
                3.0,
                crate::render::Offset::ZERO,
                1.0,
            )
        };
        for platform in APPLE.iter().chain(THE_REST.iter()) {
            let tall = caret_rect_on(*platform);
            let short = short_glyph(*platform);
            assert!(
                tall.top > short.top,
                "{platform:?}: a taller glyph pushes the caret down"
            );
            assert_eq!(
                tall.top - short.top,
                3.0,
                "{platform:?}: half the six pixels of extra glyph"
            );
        }
    }

    #[test]
    fn only_the_other_platforms_undo_the_prototypes_inset() {
        // The extra `- _kCaretHeightOffset` is on the non-Apple branch alone.
        // Its prototype started two pixels down and this is what takes that
        // back; Apple's started at zero and has nothing to undo.
        //
        // Apple: top 0 + 30's y 0, centred by (20-16)/2 = 2   -> 2
        // Others: top 2, minus 2, centred by (20-14)/2 = 3    -> 3
        for platform in APPLE {
            assert_eq!(caret_rect_on(platform).top, 2.0, "{platform:?}");
        }
        for platform in THE_REST {
            assert_eq!(caret_rect_on(platform).top, 3.0, "{platform:?}");
        }
    }

    #[test]
    fn the_cursor_offset_moves_the_caret_before_anything_else() {
        // The platform's horizontal nudge -- iOS's -2 device pixels -- is
        // added to the caret offset, so it goes through the clamp with it.
        let nudged = CaretRect::local_rect(
            crate::editable_text::TargetPlatform::IOS,
            2.0,
            14.0,
            crate::render::Offset::new(-2.0, 0.0),
            crate::render::Offset::new(30.0, 0.0),
            20.0,
            200.0,
            100.0,
            3.0,
            crate::render::Offset::ZERO,
            1.0,
        );
        assert_eq!(
            nudged.left, 28.0,
            "two to the left of where the engine put it"
        );
    }

    #[test]
    fn the_scrollable_width_is_the_wider_of_the_text_and_the_field() {
        // A field wider than its text still scrolls to its own width; a text
        // wider than its field takes its own width plus the caret's room.
        assert_eq!(CaretRect::scrollable_width(20.0, 100.0, 3.0), 100.0);
        assert_eq!(CaretRect::scrollable_width(200.0, 100.0, 3.0), 203.0);
    }

    #[test]
    fn the_caret_may_reach_the_last_place_it_fits_and_no_further() {
        // The clamp's ceiling is `scrollableWidth - caretMargin`: the caret
        // stops where it still fits, not where the text ends.
        let far = CaretRect::local_rect(
            crate::editable_text::TargetPlatform::Android,
            2.0,
            14.0,
            crate::render::Offset::ZERO,
            crate::render::Offset::new(9_000.0, 0.0),
            20.0,
            200.0,
            100.0,
            3.0,
            crate::render::Offset::ZERO,
            1.0,
        );
        assert_eq!(far.left, 200.0, "203 of scrollable width, less the 3");
        assert_eq!(far.width(), 2.0, "and the size survived the clamp");
    }

    #[test]
    fn a_caret_dragged_left_of_the_field_stops_at_the_origin() {
        let behind = CaretRect::local_rect(
            crate::editable_text::TargetPlatform::Android,
            2.0,
            14.0,
            crate::render::Offset::new(-500.0, 0.0),
            crate::render::Offset::new(30.0, 0.0),
            20.0,
            200.0,
            100.0,
            3.0,
            crate::render::Offset::ZERO,
            1.0,
        );
        assert_eq!(behind.left, 0.0);
        assert_eq!(behind.width(), 2.0);
    }

    #[test]
    fn the_clamp_does_not_touch_the_vertical() {
        // Only x goes through it. A caret on a line scrolled below the field
        // keeps its y, because the field scrolls and the caret has to scroll
        // with it.
        let low = CaretRect::local_rect(
            crate::editable_text::TargetPlatform::Android,
            2.0,
            14.0,
            crate::render::Offset::ZERO,
            crate::render::Offset::new(30.0, 4_000.0),
            20.0,
            200.0,
            100.0,
            3.0,
            crate::render::Offset::ZERO,
            1.0,
        );
        assert_eq!(low.top, 4_003.0, "carried through untouched");
    }

    #[test]
    fn the_paint_offset_lands_before_the_snap_and_not_after() {
        // Upstream shifts by the paint offset and *then* asks for the snap of
        // the shifted top left. Snapping the unscrolled position and shifting
        // afterwards would land off the grid again.
        //
        // Half-pixel grid, and a scroll of -0.3: the shifted left is 29.7 and
        // the correction rounds it to 29.5.
        let rect = CaretRect::local_rect(
            crate::editable_text::TargetPlatform::Android,
            2.0,
            14.0,
            crate::render::Offset::ZERO,
            crate::render::Offset::new(30.0, 0.0),
            20.0,
            200.0,
            100.0,
            3.0,
            crate::render::Offset::new(-0.3, 0.0),
            2.0,
        );
        assert!(
            (rect.left - 29.5).abs() < 1e-4,
            "on the half-pixel grid: {rect:?}"
        );
        assert!(
            (rect.width() - 2.0).abs() < 1e-4,
            "and the whole rect moved together: {rect:?}"
        );
    }

    // -- One class paints both highlights, tick 280 --------------------------

    fn highlighting(start: isize, end: isize) -> TextHighlightPainter {
        let mut painter = TextHighlightPainter::new();
        painter.set_highlighted_range(Some(crate::services::text_boundary::TextRange {
            start,
            end,
        }));
        painter.set_highlight_color(Some(crate::engine::Color(0x8000_00FF)));
        painter
    }

    fn text_of(width: f32, height: f32) -> crate::render::Size {
        crate::render::Size::new(width, height)
    }

    #[test]
    fn the_prompt_rectangle_and_the_selection_are_the_same_painter() {
        // Two instances of one class, differing only in the range and colour
        // they are handed -- which is why dismissing the prompt is
        // setPromptRectRange(null) and not a teardown of its own.
        let mut selection = TextHighlightPainter::new();
        let mut prompt = TextHighlightPainter::new();
        assert_eq!(selection, prompt, "nothing distinguishes them at rest");

        selection.set_highlighted_range(Some(crate::services::text_boundary::TextRange {
            start: 0,
            end: 4,
        }));
        prompt.set_highlighted_range(Some(crate::services::text_boundary::TextRange {
            start: 0,
            end: 4,
        }));
        assert_eq!(selection, prompt, "nor once both are told the same thing");
    }

    #[test]
    fn clearing_the_range_is_how_the_prompt_is_dismissed() {
        let mut painter = highlighting(0, 4);
        assert!(
            !painter
                .rects(
                    &[crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 14.0)],
                    crate::render::Offset::ZERO,
                    text_of(200.0, 14.0),
                )
                .is_empty()
        );
        assert!(painter.set_highlighted_range(None), "and it did change");
        assert!(
            painter
                .rects(
                    &[crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 14.0)],
                    crate::render::Offset::ZERO,
                    text_of(200.0, 14.0),
                )
                .is_empty()
        );
    }

    #[test]
    fn setting_the_same_range_again_does_not_ask_for_a_repaint() {
        // The setter is guarded by an equality test before it notifies.
        let mut painter = TextHighlightPainter::new();
        let range = crate::services::text_boundary::TextRange { start: 0, end: 4 };
        assert!(painter.set_highlighted_range(Some(range)), "new");
        assert!(!painter.set_highlighted_range(Some(range)), "same again");
        assert!(painter.set_highlighted_range(None), "changed back");
    }

    #[test]
    fn setting_the_same_colour_again_does_not_ask_for_a_repaint() {
        let mut painter = TextHighlightPainter::new();
        let blue = crate::engine::Color(0x8000_00FF);
        assert!(painter.set_highlight_color(Some(blue)));
        assert!(!painter.set_highlight_color(Some(blue)));
        assert!(painter.set_highlight_color(None));
    }

    #[test]
    fn a_collapsed_range_paints_nothing_at_all() {
        // The third of the three early returns, and the one a reader would not
        // predict: a highlight of no width is not drawn as a hairline.
        let painter = highlighting(3, 3);
        assert_eq!(
            painter.rects(
                &[crate::engine::Rect::ltrb(30.0, 0.0, 30.0, 14.0)],
                crate::render::Offset::ZERO,
                text_of(200.0, 14.0),
            ),
            Vec::new(),
            "though the paragraph handed back a box for it"
        );
    }

    #[test]
    fn a_range_with_no_colour_paints_nothing() {
        let mut painter = TextHighlightPainter::new();
        painter.set_highlighted_range(Some(crate::services::text_boundary::TextRange {
            start: 0,
            end: 4,
        }));
        assert!(
            painter
                .rects(
                    &[crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 14.0)],
                    crate::render::Offset::ZERO,
                    text_of(200.0, 14.0),
                )
                .is_empty()
        );
    }

    #[test]
    fn the_same_box_twice_is_drawn_once() {
        // Upstream's `.toSet()`. The same rectangle drawn twice through a
        // translucent paint is twice as dark, which is visible.
        let painter = highlighting(0, 4);
        let box_ = crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 14.0);
        let rects = painter.rects(
            &[box_, box_, box_],
            crate::render::Offset::ZERO,
            text_of(200.0, 14.0),
        );
        assert_eq!(rects.len(), 1, "three boxes, one rectangle: {rects:?}");
    }

    #[test]
    fn two_different_boxes_are_both_drawn() {
        // The other half of the previous test: the set collapses duplicates,
        // not everything.
        let painter = highlighting(0, 20);
        let rects = painter.rects(
            &[
                crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 14.0),
                crate::engine::Rect::ltrb(0.0, 14.0, 30.0, 28.0),
            ],
            crate::render::Offset::ZERO,
            text_of(200.0, 28.0),
        );
        assert_eq!(rects.len(), 2);
    }

    #[test]
    fn a_highlight_is_clipped_to_the_text_and_not_to_the_field() {
        // The intersection is with Rect.fromLTWH(0, 0, textPainter.width,
        // textPainter.height). A box on text scrolled past the left edge is
        // cut back to where the text is.
        let painter = highlighting(0, 4);
        let rects = painter.rects(
            &[crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 14.0)],
            crate::render::Offset::new(-25.0, 0.0),
            text_of(200.0, 14.0),
        );
        assert_eq!(rects[0].left, 0.0, "cut at the text's left edge, not -25");
        assert_eq!(rects[0].right, 15.0, "and the rest of it survives");
    }

    #[test]
    fn a_highlight_wider_than_the_text_is_cut_down_to_it() {
        let painter = highlighting(0, 4);
        let rects = painter.rects(
            &[crate::engine::Rect::ltrb(0.0, 0.0, 400.0, 14.0)],
            crate::render::Offset::ZERO,
            text_of(200.0, 14.0),
        );
        assert_eq!(rects[0].right, 200.0, "the text's width, not the box's");
    }

    #[test]
    fn the_paint_offset_moves_the_box_before_the_clip_and_not_after() {
        // Shift then intersect. Intersecting first would clip against the
        // unscrolled position and then slide the clipped rectangle off.
        let painter = highlighting(0, 4);
        let rects = painter.rects(
            &[crate::engine::Rect::ltrb(180.0, 0.0, 220.0, 14.0)],
            crate::render::Offset::new(-100.0, 0.0),
            text_of(200.0, 14.0),
        );
        assert_eq!(
            (rects[0].left, rects[0].right),
            (80.0, 120.0),
            "shifted into view and never clipped"
        );
    }

    #[test]
    fn the_autocorrect_highlight_is_painted_under_the_selection() {
        // Where a reader selects text that is also being autocorrected, the
        // selection's colour is the one they see.
        let stack = EditablePainters::background(false);
        let autocorrect = stack
            .iter()
            .position(|slot| *slot == EditablePainterSlot::AutocorrectHighlight)
            .expect("the autocorrect highlight");
        let selection = stack
            .iter()
            .position(|slot| *slot == EditablePainterSlot::Selection)
            .expect("the selection");
        assert!(autocorrect < selection, "{stack:?}");
    }

    #[test]
    fn the_caret_is_in_exactly_one_of_the_two_stacks() {
        // paintCursorAboveText moves it between them; it is never in both and
        // never in neither.
        for above in [true, false] {
            let foreground = EditablePainters::foreground(above);
            let background = EditablePainters::background(above);
            let in_front = foreground.contains(&EditablePainterSlot::Caret);
            let behind = background.contains(&EditablePainterSlot::Caret);
            assert!(in_front != behind, "above = {above}");
            assert_eq!(in_front, above);
        }
    }

    #[test]
    fn a_caret_behind_the_glyphs_still_sits_over_both_highlights() {
        // It goes *last* in the background list, not first: under the text,
        // but over the selection it is standing in.
        let stack = EditablePainters::background(false);
        assert_eq!(stack.last(), Some(&EditablePainterSlot::Caret), "{stack:?}");
        assert_eq!(stack.len(), 3);
    }

    #[test]
    fn the_two_highlights_are_in_the_background_stack_whatever_the_caret_does() {
        for above in [true, false] {
            let stack = EditablePainters::background(above);
            assert_eq!(stack[0], EditablePainterSlot::AutocorrectHighlight);
            assert_eq!(stack[1], EditablePainterSlot::Selection);
        }
    }

    #[test]
    fn the_box_styles_start_tight() {
        // ui.BoxHeightStyle.tight and ui.BoxWidthStyle.tight are upstream's
        // defaults, and they are the first arm of each enum because the value
        // crosses to the engine as an index.
        let painter = TextHighlightPainter::new();
        assert_eq!(painter.height_style, BoxHeightStyle::Tight);
        assert_eq!(painter.width_style, BoxWidthStyle::Tight);
        assert_eq!(BoxHeightStyle::Tight as i32, 0);
        assert_eq!(BoxWidthStyle::Tight as i32, 0);
        assert_eq!(BoxHeightStyle::Strut as i32, 5, "the last of six");
        assert_eq!(BoxWidthStyle::Max as i32, 1, "the last of two");
    }

    // -- The IME bar and the physical pixel, tick 278 ------------------------

    fn a_range(start: isize, end: isize) -> crate::services::text_boundary::TextRange {
        crate::services::text_boundary::TextRange { start, end }
    }

    #[test]
    fn a_collapsed_composing_range_has_no_rect_rather_than_an_empty_one() {
        // An IME with nothing composing has nowhere to put its bar, and
        // Rect::ZERO is a place: the difference between "do not draw this" and
        // "draw it at the origin".
        let boxes = [crate::engine::Rect::ltrb(10.0, 0.0, 50.0, 14.0)];
        assert_eq!(
            ComposingRegion::rect(a_range(3, 3), &boxes, crate::render::Offset::ZERO),
            None,
            "collapsed, though there are boxes to fold"
        );
        assert!(
            ComposingRegion::rect(a_range(3, 7), &boxes, crate::render::Offset::ZERO).is_some(),
            "and the same boxes do answer for a range with width"
        );
    }

    #[test]
    fn an_invalid_composing_range_has_no_rect() {
        // (-1, 5) rather than (-1, -1): an invalid range that is *not* also
        // collapsed, so this watches the validity clause on its own. The
        // first version of this test used (-1, -1) and passed with the
        // validity test deleted, because collapsed alone already answered.
        let boxes = [crate::engine::Rect::ltrb(10.0, 0.0, 50.0, 14.0)];
        assert!(!a_range(-1, 5).is_valid() && !a_range(-1, 5).is_collapsed());
        assert_eq!(
            ComposingRegion::rect(a_range(-1, 5), &boxes, crate::render::Offset::ZERO),
            None
        );
    }

    #[test]
    fn no_boxes_is_no_rect() {
        // The fold starts at null, so a valid range the paragraph has nothing
        // to say about still answers nothing.
        assert_eq!(
            ComposingRegion::rect(a_range(3, 7), &[], crate::render::Offset::ZERO),
            None
        );
    }

    #[test]
    fn the_composing_rect_is_the_union_and_not_the_span_of_the_ends() {
        // A composing region across a wrap has boxes on two lines whose
        // horizontal ranges do not nest. First-to-last, the way
        // getEndpointsForSelection reads its boxes, would start at 60 and end
        // at 40 -- an inside-out rectangle that misses the whole first line.
        let boxes = [
            crate::engine::Rect::ltrb(60.0, 0.0, 200.0, 14.0),
            crate::engine::Rect::ltrb(0.0, 14.0, 40.0, 28.0),
        ];
        let rect = ComposingRegion::rect(a_range(3, 20), &boxes, crate::render::Offset::ZERO)
            .expect("a rect");
        assert_eq!(rect.left, 0.0, "from the second box");
        assert_eq!(rect.right, 200.0, "from the first");
        assert_eq!(rect.top, 0.0);
        assert_eq!(rect.bottom, 28.0, "both lines cleared");
    }

    #[test]
    fn a_middle_box_can_widen_the_union() {
        // Not just the two ends: every box is folded in, so a long middle line
        // sets the right edge.
        let boxes = [
            crate::engine::Rect::ltrb(60.0, 0.0, 100.0, 14.0),
            crate::engine::Rect::ltrb(0.0, 14.0, 300.0, 28.0),
            crate::engine::Rect::ltrb(0.0, 28.0, 40.0, 42.0),
        ];
        let rect = ComposingRegion::rect(a_range(3, 30), &boxes, crate::render::Offset::ZERO)
            .expect("a rect");
        assert_eq!(rect.right, 300.0, "the middle box is the widest");
    }

    #[test]
    fn the_paint_offset_shifts_the_union_once() {
        // Applied after the fold, not inside it -- folding it in per box would
        // multiply it by the number of lines.
        let boxes = [
            crate::engine::Rect::ltrb(10.0, 0.0, 50.0, 14.0),
            crate::engine::Rect::ltrb(0.0, 14.0, 30.0, 28.0),
        ];
        let rect = ComposingRegion::rect(
            a_range(3, 20),
            &boxes,
            crate::render::Offset::new(-5.0, -7.0),
        )
        .expect("a rect");
        assert_eq!((rect.left, rect.top), (-5.0, -7.0));
        assert_eq!((rect.right, rect.bottom), (45.0, 21.0));
    }

    #[test]
    fn the_snap_hands_back_how_far_to_move_not_where_to_move_to() {
        // The trailing subtraction is the whole method. The caller writes
        // `caretRect.shift(snap(caretRect.topLeft))`, so the snapped position
        // would teleport the caret to near the screen's origin.
        let correction =
            ComposingRegion::snap_to_physical_pixel(crate::render::Offset::new(100.3, 50.4), 2.0);
        // Half-pixel grid: 100.3 -> 100.5, 50.4 -> 50.5.
        assert!(
            (correction.dx - 0.2).abs() < 1e-4,
            "a fifth of a pixel, not 100.5: {correction:?}"
        );
        assert!((correction.dy - 0.1).abs() < 1e-4, "{correction:?}");
    }

    #[test]
    fn the_grid_is_one_physical_pixel_wide() {
        // pixelMultiple = 1 / devicePixelRatio, so a denser screen snaps to a
        // finer grid and the same position needs a smaller correction.
        let at = crate::render::Offset::new(10.3, 0.0);
        let coarse = ComposingRegion::snap_to_physical_pixel(at, 1.0);
        let fine = ComposingRegion::snap_to_physical_pixel(at, 4.0);
        assert!((coarse.dx - -0.3).abs() < 1e-4, "to 10.0: {coarse:?}");
        assert!((fine.dx - -0.05).abs() < 1e-4, "to 10.25: {fine:?}");
    }

    #[test]
    fn the_snap_rounds_to_the_nearest_line_of_the_grid_either_way() {
        // Round, not floor: a caret just past a pixel boundary comes back to
        // it rather than being pushed a whole pixel on.
        let up =
            ComposingRegion::snap_to_physical_pixel(crate::render::Offset::new(10.1, 0.0), 1.0);
        let down =
            ComposingRegion::snap_to_physical_pixel(crate::render::Offset::new(10.9, 0.0), 1.0);
        assert!(up.dx < 0.0, "back to 10: {up:?}");
        assert!(down.dx > 0.0, "on to 11: {down:?}");
    }

    #[test]
    fn a_position_already_on_the_grid_is_not_moved() {
        let correction =
            ComposingRegion::snap_to_physical_pixel(crate::render::Offset::new(10.5, 4.0), 2.0);
        assert_eq!((correction.dx, correction.dy), (0.0, 0.0));
    }

    #[test]
    fn a_non_finite_coordinate_is_corrected_by_zero_and_only_on_its_own_axis() {
        // Not corrected *to* zero, and not to NaN: either would move the caret
        // somewhere it has no reason to be. And the other axis still answers.
        let correction = ComposingRegion::snap_to_physical_pixel(
            crate::render::Offset::new(f32::INFINITY, 50.4),
            2.0,
        );
        assert_eq!(correction.dx, 0.0, "no correction, rather than a huge one");
        assert!(
            (correction.dy - 0.1).abs() < 1e-4,
            "y still snapped: {correction:?}"
        );

        let nan =
            ComposingRegion::snap_to_physical_pixel(crate::render::Offset::new(f32::NAN, 4.0), 2.0);
        assert_eq!(nan.dx, 0.0, "NaN in, zero out");
    }

    // -- How tall a field wants to be, tick 277 ------------------------------

    fn extent() -> FieldExtent {
        FieldExtent {
            cursor_width: 2.0,
            force_line: false,
            multiline: false,
            max_lines: Some(1),
            min_lines: None,
            preferred_line_height: 14.0,
        }
    }

    /// A stand-in for the engine's paragraph: one line per 50 logical pixels
    /// of text, so a narrower layout is a taller one.
    fn wrapping_paragraph(text_width: f32) -> impl Fn(f32, f32) -> f32 {
        move |_min: f32, max: f32| {
            if max.is_infinite() {
                14.0
            } else {
                let lines = (text_width / max.max(1.0)).ceil().max(1.0);
                14.0 * lines
            }
        }
    }

    #[test]
    fn the_caret_margin_is_the_gap_plus_the_cursor_not_either_alone() {
        // A caret after the last character is outside the text, so the box has
        // to be wider than its text by the cursor's own width *and* a pixel of
        // gap. Either half alone draws the caret touching the glyph or half
        // off the edge.
        let mut field = extent();
        field.cursor_width = 2.0;
        assert_eq!(field.caret_margin(), 3.0);
        field.cursor_width = 5.0;
        assert_eq!(field.caret_margin(), 6.0, "it tracks the cursor's width");
        assert_eq!(FieldExtent::CARET_GAP, 1.0);
    }

    #[test]
    fn a_field_that_is_not_multiline_lays_its_text_out_at_infinite_width() {
        // This one line is the whole of horizontal scrolling. Nothing wraps,
        // the paragraph comes back wider than the box, and the box scrolls --
        // there is no other code for it anywhere.
        let mut field = extent();
        field.multiline = false;
        let (_, max) = field.adjust_constraints(0.0, 100.0);
        assert!(max.is_infinite(), "so nothing can wrap");

        field.multiline = true;
        let (_, max) = field.adjust_constraints(0.0, 100.0);
        assert_eq!(max, 97.0, "the box, less the caret's room");
    }

    #[test]
    fn force_line_raises_the_minimum_rather_than_the_maximum() {
        // The line fills the field instead of shrink-wrapping the text, and it
        // does that by moving the *minimum* up -- the maximum is untouched.
        let mut field = extent();
        field.multiline = true;
        let (loose_min, loose_max) = field.adjust_constraints(0.0, 100.0);
        field.force_line = true;
        let (forced_min, forced_max) = field.adjust_constraints(0.0, 100.0);
        assert_eq!(loose_min, 0.0);
        assert_eq!(forced_min, 97.0, "up to the whole available width");
        assert_eq!(forced_max, loose_max, "and the maximum did not move");
    }

    #[test]
    fn a_field_narrower_than_its_own_caret_asks_for_nothing_not_less() {
        // max(0, maxWidth - caretMargin). A negative width would propagate
        // into the paragraph.
        let field = extent();
        let (min, _) = field.adjust_constraints(0.0, 1.0);
        assert_eq!(min, 0.0);
        let mut multiline = field;
        multiline.multiline = true;
        let (_, max) = multiline.adjust_constraints(0.0, 1.0);
        assert_eq!(max, 0.0, "nothing, not minus two");
    }

    #[test]
    fn the_minimum_width_never_exceeds_the_maximum() {
        // min(minWidth, availableMaxWidth): asking for a minimum wider than
        // what is left after the caret's room gives back the maximum.
        let mut field = extent();
        field.multiline = true;
        let (min, max) = field.adjust_constraints(500.0, 100.0);
        assert_eq!(min, 97.0);
        assert_eq!(min, max, "and not the 500 that was asked for");
    }

    #[test]
    fn min_lines_falls_back_to_max_lines_so_a_capped_field_does_not_grow() {
        // `this.minLines ?? maxLines`. A field given only `maxLines: 3` has
        // minLines == maxLines and is *always exactly* three lines tall.
        // Reading maxLines as a ceiling gets that field wrong at rest.
        let mut field = extent();
        field.multiline = true;
        field.max_lines = Some(3);
        field.min_lines = None;
        let short = field.preferred_height(200.0, "hi", &wrapping_paragraph(30.0));
        assert_eq!(short, 42.0, "three lines even with one line of text");

        field.min_lines = Some(1);
        let grows = field.preferred_height(200.0, "hi", &wrapping_paragraph(30.0));
        assert_eq!(grows, 14.0, "asked for one, so it may be one");
    }

    #[test]
    fn a_one_line_field_reports_the_height_the_text_actually_took() {
        // Upstream: "Special case maxLines == 1 since it forces the scrollable
        // direction to be horizontal. Report the real height to prevent the
        // text from being clipped." So a tall glyph is not cut off, and this
        // is not `preferredLineHeight * 1`.
        let field = extent();
        let tall = |_min: f32, _max: f32| 40.0;
        assert_eq!(
            field.preferred_height(200.0, "hi", &tall),
            40.0,
            "the paragraph's height, not one line height"
        );
    }

    #[test]
    fn a_fixed_field_never_asks_the_paragraph_at_all() {
        // minLines == maxLines returns minHeight with no layout, because the
        // answer cannot depend on the text.
        let mut field = extent();
        field.multiline = true;
        field.max_lines = Some(4);
        field.min_lines = Some(4);
        let exploding = |_min: f32, _max: f32| -> f32 { panic!("laid out anyway") };
        assert_eq!(field.preferred_height(200.0, "anything", &exploding), 56.0);
    }

    #[test]
    fn an_unbounded_field_at_infinite_width_counts_only_the_breaks_in_the_text() {
        // Nothing wraps at infinite width, so the estimate is the hard breaks
        // plus one -- and the paragraph is not consulted.
        let mut field = extent();
        field.multiline = true;
        field.max_lines = None;
        field.min_lines = None;
        let exploding = |_min: f32, _max: f32| -> f32 { panic!("laid out anyway") };
        let height = field.preferred_height(f32::INFINITY, "a\nb\nc", &exploding);
        assert_eq!(height, 42.0, "two breaks, three lines");
    }

    #[test]
    fn an_unbounded_field_is_still_at_least_its_minimum() {
        // max(estimatedHeight, minHeight): min_lines wins over a short text.
        let mut field = extent();
        field.multiline = true;
        field.max_lines = None;
        field.min_lines = Some(5);
        let height = field.preferred_height(200.0, "hi", &wrapping_paragraph(30.0));
        assert_eq!(height, 70.0, "five lines, though the text is one");
    }

    #[test]
    fn a_bounded_field_is_clamped_at_both_ends() {
        // The last branch: the laid-out height between minHeight and
        // preferredLineHeight * maxLines.
        let mut field = extent();
        field.multiline = true;
        field.min_lines = Some(2);
        field.max_lines = Some(4);
        // 30 wide of text in a 197-wide layout is one line; the floor applies.
        assert_eq!(
            field.preferred_height(200.0, "hi", &wrapping_paragraph(30.0)),
            28.0,
            "raised to the two-line floor"
        );
        // 2000 wide of text wraps to eleven lines; the ceiling applies.
        assert_eq!(
            field.preferred_height(200.0, "hi", &wrapping_paragraph(2000.0)),
            56.0,
            "cut down to the four-line ceiling"
        );
    }

    #[test]
    fn carriage_return_is_not_a_hard_line_break() {
        // Text pasted from Windows arrives as CRLF and is counted once, by its
        // LF. A port that adds CR to the table counts every line twice.
        assert_eq!(FieldExtent::count_hard_line_breaks("a\r\nb\r\nc"), 2);
        assert_eq!(
            FieldExtent::count_hard_line_breaks("a\rb\rc"),
            0,
            "CR alone"
        );
    }

    #[test]
    fn the_other_five_separators_count() {
        // NEL, VT, FF, LS and PS alongside LF -- and upstream records the
        // choice about FF: "treating it as a regular line separator".
        for separator in [
            '\u{000A}', '\u{0085}', '\u{000B}', '\u{000C}', '\u{2028}', '\u{2029}',
        ] {
            let text = format!("a{separator}b");
            assert_eq!(
                FieldExtent::count_hard_line_breaks(&text),
                1,
                "U+{:04X}",
                separator as u32
            );
        }
        assert_eq!(
            FieldExtent::count_hard_line_breaks("a\tb c"),
            0,
            "not tabs or spaces"
        );
    }

    #[test]
    fn only_the_maximum_intrinsic_width_makes_room_for_the_caret() {
        // The maximum is everything on one line, where the caret can end up
        // past the last character; the minimum is the narrowest wrap, where it
        // cannot.
        let field = extent();
        assert_eq!(field.min_intrinsic_width(80.0), 80.0);
        assert_eq!(field.max_intrinsic_width(80.0), 83.0);
    }

    #[test]
    fn force_line_takes_the_whole_offered_width_in_a_dry_layout() {
        let mut field = extent();
        field.force_line = true;
        assert_eq!(field.dry_width(0.0, 300.0, 40.0), 300.0);
        field.force_line = false;
        assert_eq!(
            field.dry_width(0.0, 300.0, 40.0),
            43.0,
            "the text plus the caret's room"
        );
        assert_eq!(
            field.dry_width(0.0, 20.0, 400.0),
            20.0,
            "constrained down to what was offered"
        );
    }

    // -- Where the two handles go, tick 276 ----------------------------------
    //
    // Upstream's doc for the point: "Coordinates of the **lower** left or
    // lower right corner of the selection." Every surprise follows from that.

    fn a_box(start: f32, end: f32, bottom: f32) -> SelectionBox {
        SelectionBox {
            start,
            end,
            bottom,
            direction: crate::direction::TextDirection::Ltr,
        }
    }

    #[test]
    fn no_boxes_is_one_endpoint_with_no_direction() {
        // A collapsed selection never asks the paragraph for boxes at all, and
        // a selection that asks can still get none. Both land here, and both
        // get *one* point -- a caret has no second end to hold.
        let points = SelectionEndpoints::of(
            &[],
            crate::render::Offset::new(30.0, 10.0),
            14.0,
            200.0,
            crate::render::Offset::ZERO,
        );
        assert_eq!(points.len(), 1, "one, not two");
        assert_eq!(points[0].direction, None, "no box to have read one from");
    }

    #[test]
    fn the_lone_endpoint_hangs_a_line_below_the_caret() {
        // The caret offset is the caret's *top*. A handle hangs from the
        // bottom, so a whole line height goes on the y -- and nothing goes on
        // the x, which is the half a reasonable guess gets wrong in the other
        // direction.
        let caret = crate::render::Offset::new(30.0, 10.0);
        let points = SelectionEndpoints::of(&[], caret, 14.0, 200.0, crate::render::Offset::ZERO);
        assert_eq!(points[0].point.dy, 24.0, "10 + a line");
        assert_eq!(points[0].point.dx, 30.0, "the caret's x, untouched");
    }

    #[test]
    fn the_endpoints_are_the_boxes_bottoms_not_their_tops() {
        // Same rule as the lone endpoint, arrived at the other way: the box
        // carries its own bottom, so nothing is added here.
        let points = SelectionEndpoints::of(
            &[a_box(10.0, 90.0, 24.0)],
            crate::render::Offset::ZERO,
            14.0,
            200.0,
            crate::render::Offset::ZERO,
        );
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].point.dy, 24.0);
        assert_eq!(points[1].point.dy, 24.0);
    }

    #[test]
    fn the_first_box_gives_the_start_and_the_last_gives_the_end() {
        // A selection across a wrap is several boxes. The handles belong on
        // the outside of the whole run, so the middle boxes are not consulted
        // -- and taking `end` from the first box would put the far handle at
        // the end of the first line.
        let points = SelectionEndpoints::of(
            &[
                a_box(10.0, 90.0, 24.0),
                a_box(0.0, 70.0, 48.0),
                a_box(0.0, 35.0, 72.0),
            ],
            crate::render::Offset::ZERO,
            14.0,
            200.0,
            crate::render::Offset::ZERO,
        );
        assert_eq!(
            (points[0].point.dx, points[0].point.dy),
            (10.0, 24.0),
            "the first box's start, on the first line"
        );
        assert_eq!(
            (points[1].point.dx, points[1].point.dy),
            (35.0, 72.0),
            "the last box's end, on the last line"
        );
    }

    #[test]
    fn each_handle_takes_its_own_boxs_direction() {
        // A selection that begins in English and ends in Arabic has ends that
        // run opposite ways, and a left handle at a right-to-left end is the
        // wrong handle.
        use crate::direction::TextDirection;
        let mut first = a_box(10.0, 90.0, 24.0);
        first.direction = TextDirection::Ltr;
        let mut last = a_box(0.0, 70.0, 48.0);
        last.direction = TextDirection::Rtl;
        let points = SelectionEndpoints::of(
            &[first, last],
            crate::render::Offset::ZERO,
            14.0,
            200.0,
            crate::render::Offset::ZERO,
        );
        assert_eq!(points[0].direction, Some(TextDirection::Ltr));
        assert_eq!(points[1].direction, Some(TextDirection::Rtl));
    }

    #[test]
    fn x_is_clamped_into_the_text_and_y_is_not() {
        // `clampDouble(boxes.first.start, 0, size.width)` and nothing around
        // `bottom`. A box scrolled off to the left still gets its handle at
        // the origin; a box below the text keeps its own y, because the field
        // scrolls vertically and a clamped y would pin the handle to the last
        // visible line.
        let points = SelectionEndpoints::of(
            &[a_box(-40.0, 900.0, 4000.0)],
            crate::render::Offset::ZERO,
            14.0,
            200.0,
            crate::render::Offset::ZERO,
        );
        assert_eq!(points[0].point.dx, 0.0, "clamped up to the origin");
        assert_eq!(points[1].point.dx, 200.0, "clamped down to the width");
        assert_eq!(points[0].point.dy, 4000.0, "not clamped at all");
    }

    #[test]
    fn the_paint_offset_reaches_both_branches() {
        // The scroll offset is added whether there were boxes or not. Getting
        // it into only one branch means the handles are right until the caret
        // collapses, and then jump.
        let shift = crate::render::Offset::new(-5.0, -7.0);
        let boxed = SelectionEndpoints::of(
            &[a_box(10.0, 90.0, 24.0)],
            crate::render::Offset::ZERO,
            14.0,
            200.0,
            shift,
        );
        assert_eq!((boxed[0].point.dx, boxed[0].point.dy), (5.0, 17.0));
        let lone = SelectionEndpoints::of(
            &[],
            crate::render::Offset::new(30.0, 10.0),
            14.0,
            200.0,
            shift,
        );
        assert_eq!((lone[0].point.dx, lone[0].point.dy), (25.0, 17.0));
    }

    #[test]
    fn an_invalid_selection_is_invisible_rather_than_visible() {
        // Both notifiers are constructed `true`, and an invalid selection
        // drives them **false**: "I do not know where it is" resolves to "you
        // cannot see it", which is what stops a handle appearing at the
        // origin.
        let hidden = SelectionVisibility::of(
            false,
            crate::render::Size::new(200.0, 40.0),
            crate::render::Offset::new(10.0, 10.0),
            crate::render::Offset::new(20.0, 10.0),
            crate::render::Offset::ZERO,
        );
        assert_eq!(
            hidden,
            SelectionVisibility {
                start: false,
                end: false
            },
            "not the true they start at, even though those offsets are inside"
        );
    }

    #[test]
    fn a_caret_just_above_the_field_still_counts_as_inside() {
        // Upstream: "a difference between rounded and unrounded values causes
        // the caret to be reported as having a slightly (< 0.5) negative y
        // offset. This rounding happens in paragraph.cc's layout and
        // TextPainter's _applyFloatingPointHack."
        //
        // So a caret on the top line reports about -0.4 and a strict test
        // makes its handle vanish while you are looking at it.
        let field = crate::render::Size::new(200.0, 40.0);
        let inside = SelectionVisibility::of(
            true,
            field,
            crate::render::Offset::new(10.0, -0.4),
            crate::render::Offset::new(20.0, -0.4),
            crate::render::Offset::ZERO,
        );
        assert!(inside.start && inside.end, "within the slop");

        let outside = SelectionVisibility::of(
            true,
            field,
            crate::render::Offset::new(10.0, -0.6),
            crate::render::Offset::new(20.0, -0.6),
            crate::render::Offset::ZERO,
        );
        assert!(
            !outside.start && !outside.end,
            "past the slop, genuinely scrolled away"
        );
    }

    #[test]
    fn the_two_ends_are_answered_separately() {
        // The whole point of two notifiers: scroll until the start has left
        // and the end has not, and one handle is painted.
        let visibility = SelectionVisibility::of(
            true,
            crate::render::Size::new(200.0, 40.0),
            crate::render::Offset::new(10.0, 10.0),
            crate::render::Offset::new(190.0, 10.0),
            crate::render::Offset::new(-100.0, 0.0),
        );
        assert!(!visibility.start, "scrolled off to the left");
        assert!(visibility.end, "still in the field");
    }

    // -- What a double tap selects, tick 275 ---------------------------------
    //
    // `getWordAtOffset` is five rules deep and only the last is "the word
    // boundary".

    /// A stand-in for the engine's paragraph: words are runs between spaces.
    fn spaces_boundary(
        text: &'static str,
    ) -> impl Fn(isize) -> crate::services::text_boundary::TextRange {
        move |offset: isize| {
            let bytes = text.as_bytes();
            let at = (offset.max(0) as usize).min(bytes.len().saturating_sub(1));
            let is_space = |index: usize| bytes.get(index) == Some(&b' ');
            let here = is_space(at);
            let mut start = at;
            while start > 0 && is_space(start - 1) == here {
                start -= 1;
            }
            let mut end = at;
            while end < bytes.len() && is_space(end) == here {
                end += 1;
            }
            crate::services::text_boundary::TextRange {
                start: start as isize,
                end: end as isize,
            }
        }
    }

    fn selection(
        obscured: bool,
        read_only: bool,
        platform: crate::editable_text::TargetPlatform,
    ) -> WordSelection<'static> {
        WordSelection {
            text: "one  two",
            obscured,
            read_only,
            platform,
        }
    }

    #[test]
    fn past_the_end_of_the_text_is_a_collapsed_cursor() {
        // Upstream's comment: "When long-pressing past the end of the text,
        // we want a collapsed cursor." Selecting the last word would be a
        // reasonable guess and is not what happens.
        use crate::editable_text::TargetPlatform;
        let words = selection(false, false, TargetPlatform::Android);
        let boundary = spaces_boundary("one  two");
        let past = words.at_offset(99, false, &boundary);
        assert_eq!(past.start, 8);
        assert_eq!(past.end, 8, "collapsed, not the last word");
        assert!(past.is_collapsed());
    }

    #[test]
    fn an_obscured_field_is_one_word() {
        // A password has no word boundaries a reader can see, so a double tap
        // takes the lot rather than whatever run of bullets happens to sit
        // between two spaces in the text underneath.
        use crate::editable_text::TargetPlatform;
        let boundary = spaces_boundary("one  two");
        let hidden = selection(true, false, TargetPlatform::Android);
        let all = hidden.at_offset(1, false, &boundary);
        assert_eq!((all.start, all.end), (0, 8));

        let plain = selection(false, false, TargetPlatform::Android);
        let word = plain.at_offset(1, false, &boundary);
        assert_eq!((word.start, word.end), (0, 3), "just the first word");
    }

    #[test]
    fn upstream_affinity_is_one_to_the_left() {
        // "upstream affinity is effectively -1 in text position". A caret
        // between two characters belongs to the one before it, and this is
        // the line that turns that into an index -- so at the space after
        // "one" the two affinities land on different sides of the boundary.
        use crate::editable_text::TargetPlatform;
        let words = selection(false, true, TargetPlatform::Android);
        let boundary = spaces_boundary("one  two");
        // Offset 3 is the first space, so downstream lands on whitespace and
        // upstream lands on the last letter of the word before it. Offset 4
        // would not do: both sides of it are spaces and the two affinities
        // agree.
        let downstream = words.at_offset(3, false, &boundary);
        let upstream = words.at_offset(3, true, &boundary);
        assert_ne!(
            (downstream.start, downstream.end),
            (upstream.start, upstream.end)
        );
    }

    #[test]
    fn only_a_read_only_android_field_reaches_back_for_the_previous_word() {
        // Upstream's Android arm has **no `break`** when the field is
        // editable, so it falls out of the switch to the same answer every
        // other platform gives. Reading it as "Android does the
        // previous-word thing" is wrong.
        use crate::editable_text::TargetPlatform;
        let boundary = spaces_boundary("one  two");
        let read_only =
            selection(false, true, TargetPlatform::Android).at_offset(4, false, &boundary);
        assert_eq!((read_only.start, read_only.end), (0, 4), "back to the word");

        let editable =
            selection(false, false, TargetPlatform::Android).at_offset(4, false, &boundary);
        assert_eq!(
            (editable.start, editable.end),
            (3, 5),
            "the plain boundary: the run of spaces"
        );
    }

    #[test]
    fn ios_reaches_back_whether_the_field_is_editable_or_not() {
        // The iOS arm does not consult `readOnly` at all, which is the other
        // half of the previous test: the two platforms differ, and Android
        // differs from itself.
        use crate::editable_text::TargetPlatform;
        let boundary = spaces_boundary("one  two");
        for read_only in [true, false] {
            let word =
                selection(false, read_only, TargetPlatform::IOS).at_offset(4, false, &boundary);
            assert_eq!((word.start, word.end), (0, 4), "read_only = {read_only}");
        }
    }

    #[test]
    fn the_other_platforms_fall_through_to_the_plain_boundary() {
        use crate::editable_text::TargetPlatform;
        let boundary = spaces_boundary("one  two");
        for platform in [
            TargetPlatform::Fuchsia,
            TargetPlatform::MacOS,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            let word = selection(false, true, platform).at_offset(4, false, &boundary);
            assert_eq!((word.start, word.end), (3, 5), "{platform:?}");
        }
    }

    #[test]
    fn a_tap_on_the_first_character_of_a_word_goes_to_its_start() {
        // `position.offset <= word.start`, not `<`. A tap exactly on the
        // first character goes to the start of this word, not to the end of
        // the one before it.
        use crate::services::text_boundary::TextRange;
        let word = TextRange { start: 4, end: 8 };
        assert_eq!(WordSelection::word_edge(4, word), (4, false));
        assert_eq!(WordSelection::word_edge(3, word), (4, false));
        // Anywhere inside goes to the end, with upstream affinity -- which is
        // what keeps the caret on this line when the word ends at a wrap.
        assert_eq!(WordSelection::word_edge(5, word), (8, true));
        assert_eq!(WordSelection::word_edge(8, word), (8, true));
    }

    #[test]
    fn a_backwards_drag_selects_the_same_span_with_the_ends_swapped() {
        // `isFromWordBeforeToWord = fromWord.start < toWord.end` is what
        // makes the handles stay on the ends the finger put them.
        use crate::services::text_boundary::TextRange;
        let first = TextRange { start: 0, end: 3 };
        let second = TextRange { start: 5, end: 8 };
        assert_eq!(WordSelection::words_in_range(first, second), (0, 8));
        assert_eq!(
            WordSelection::words_in_range(second, first),
            (8, 0),
            "the same span, base and extent the other way about"
        );
    }

    // -- The floating cursor, tick 274 ---------------------------------------
    //
    // On iOS the caret can be lifted off the text and dragged. What makes
    // this more than a clamp is that dragging back in is not the reverse of
    // dragging out.

    #[test]
    fn the_bottom_bound_leaves_a_line_and_the_right_bound_does_not() {
        // The cursor's offset is its top-left corner, so the last position
        // where a whole line still fits is a line height above the bottom. A
        // caret is a line tall and nothing wide, so the right edge has
        // nothing to subtract.
        let bounds = FloatingCursor::bounds(
            crate::render::Size::new(200.0, 100.0),
            crate::render::Size::new(200.0, 100.0),
            20.0,
        );
        assert_eq!(bounds.left, -4.0);
        assert_eq!(bounds.top, -4.0);
        assert_eq!(
            bounds.right, 204.0,
            "width plus the margin, nothing taken off"
        );
        assert_eq!(bounds.bottom, 85.0, "100 - 20 + 5");
    }

    #[test]
    fn the_bottom_margin_is_the_one_that_is_not_four() {
        assert_eq!(FloatingCursor::MARGIN_LEFT, 4.0);
        assert_eq!(FloatingCursor::MARGIN_TOP, 4.0);
        assert_eq!(FloatingCursor::MARGIN_RIGHT, 4.0);
        assert_eq!(FloatingCursor::MARGIN_BOTTOM, 5.0);
    }

    #[test]
    fn the_caret_may_not_wander_past_the_text_however_wide_the_field() {
        // `min(size, textPainter)`, not either alone. A one-word field does
        // not let the caret out into empty space.
        let narrow_text = FloatingCursor::bounds(
            crate::render::Size::new(400.0, 100.0),
            crate::render::Size::new(50.0, 100.0),
            20.0,
        );
        assert_eq!(narrow_text.right, 54.0, "the text decides, not the field");

        let narrow_field = FloatingCursor::bounds(
            crate::render::Size::new(50.0, 100.0),
            crate::render::Size::new(400.0, 100.0),
            20.0,
        );
        assert_eq!(narrow_field.right, 54.0, "and so does the field");
    }

    #[test]
    fn without_the_reset_the_whole_thing_is_a_clamp() {
        // `_shouldResetOrigin` false is upstream's early return, and it is
        // the behaviour a port that read only the first two lines would have
        // written for everything.
        let bounds = crate::engine::Rect::ltrb(0.0, 0.0, 100.0, 50.0);
        let mut cursor = FloatingCursor::default();
        let out = cursor.advance(crate::render::Offset::new(160.0, 25.0), bounds, Some(false));
        assert_eq!(out.dx, 100.0);
        let back = cursor.advance(crate::render::Offset::new(150.0, 25.0), bounds, None);
        assert_eq!(back.dx, 100.0, "still pinned, and still lagging");
    }

    #[test]
    fn coming_back_in_moves_the_caret_at_once_rather_than_retracing_the_overshoot() {
        // The whole point. Drag forty past the right edge, then come back
        // ten: a clamp would still be pinned (150 - 10 = 140, still past
        // 100), and this starts moving immediately.
        let bounds = crate::engine::Rect::ltrb(0.0, 0.0, 100.0, 50.0);
        let mut cursor = FloatingCursor::default();

        // In bounds, establishing a previous offset.
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(90.0, 10.0), bounds, Some(true))
                .dx,
            90.0
        );
        // Out past the right edge, still going right: pinned, and armed.
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(150.0, 10.0), bounds, None)
                .dx,
            100.0
        );
        // The re-entry frame **redefines the origin to put the caret at the
        // edge**, so it answers 100 -- the same as a clamp would, and one
        // frame later they part company. Worth pinning: a port asserting the
        // move on this frame is off by one, which is how this test was first
        // written.
        let entry = cursor.advance(crate::render::Offset::new(140.0, 10.0), bounds, None);
        assert_eq!(entry.dx, 100.0, "still at the edge, origin now redefined");

        // From here it tracks. A clamp would still answer 100 at 130 and at
        // 110, and would not move until 100.
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(130.0, 10.0), bounds, None)
                .dx,
            90.0
        );
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(110.0, 10.0), bounds, None)
                .dx,
            70.0
        );
    }

    #[test]
    fn going_further_out_does_not_arm_anything_new_or_move_the_caret() {
        let bounds = crate::engine::Rect::ltrb(0.0, 0.0, 100.0, 50.0);
        let mut cursor = FloatingCursor::default();
        cursor.advance(crate::render::Offset::new(90.0, 10.0), bounds, Some(true));
        for x in [150.0, 200.0, 400.0] {
            assert_eq!(
                cursor
                    .advance(crate::render::Offset::new(x, 10.0), bounds, None)
                    .dx,
                100.0,
                "still at the edge at {x}"
            );
        }
        // The first step back pins at the edge and redefines the origin;
        // the second moves by its own step, not by the three hundred that
        // were overshot.
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(399.0, 10.0), bounds, None)
                .dx,
            100.0
        );
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(398.0, 10.0), bounds, None)
                .dx,
            99.0
        );
    }

    #[test]
    fn a_drag_that_begins_outside_has_no_overshoot_to_forgive() {
        // Arming needs the finger to be going *outward*. A drag that starts
        // beyond the edge never went out, so there is nothing to forgive and
        // the caret stays pinned until the finger genuinely arrives -- which
        // is what a plain clamp does, and is right here.
        let bounds = crate::engine::Rect::ltrb(0.0, 0.0, 100.0, 50.0);
        let mut cursor = FloatingCursor::default();
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(300.0, 10.0), bounds, Some(true))
                .dx,
            100.0
        );
        for x in [290.0, 280.0, 150.0] {
            assert_eq!(
                cursor
                    .advance(crate::render::Offset::new(x, 10.0), bounds, None)
                    .dx,
                100.0,
                "still pinned at {x}: nothing was armed"
            );
        }
        // And it arrives when the finger does.
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(80.0, 10.0), bounds, None)
                .dx,
            80.0
        );
    }

    #[test]
    fn an_armed_edge_is_spent_only_by_a_movement_back_in() {
        // Going *further* out while armed must not spend the flag. If it did,
        // the origin would be redefined mid-overshoot and the caret would
        // start tracking one frame early -- which is the same lag the flag
        // exists to remove, just smaller.
        let bounds = crate::engine::Rect::ltrb(0.0, 0.0, 100.0, 50.0);
        let mut cursor = FloatingCursor::default();
        cursor.advance(crate::render::Offset::new(90.0, 10.0), bounds, Some(true));
        cursor.advance(crate::render::Offset::new(150.0, 10.0), bounds, None);
        // Still going out, and armed. Nothing is spent here.
        cursor.advance(crate::render::Offset::new(200.0, 10.0), bounds, None);
        // The first frame back is the one that redefines the origin, so it
        // is still at the edge.
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(190.0, 10.0), bounds, None)
                .dx,
            100.0
        );
        assert_eq!(
            cursor
                .advance(crate::render::Offset::new(180.0, 10.0), bounds, None)
                .dx,
            90.0
        );
    }

    #[test]
    fn redefining_one_axis_origin_leaves_the_other_alone() {
        // Four flags and four origins-by-axis. A vertical excursion sets the
        // y origin; a later horizontal one must not clear it, or the caret
        // would jump vertically when the finger came back in sideways.
        let bounds = crate::engine::Rect::ltrb(0.0, 0.0, 100.0, 50.0);
        let mut cursor = FloatingCursor::default();
        cursor.advance(crate::render::Offset::new(50.0, 25.0), bounds, Some(true));
        // Down past the bottom, then back: the y origin becomes non-zero.
        cursor.advance(crate::render::Offset::new(50.0, 90.0), bounds, None);
        cursor.advance(crate::render::Offset::new(50.0, 80.0), bounds, None);
        let after_vertical = cursor.advance(crate::render::Offset::new(50.0, 70.0), bounds, None);
        assert_eq!(
            after_vertical.dy, 40.0,
            "80 - 50 is the y origin, so 70 - 30"
        );

        // Now a horizontal excursion and return. The y answer must not move.
        cursor.advance(crate::render::Offset::new(200.0, 70.0), bounds, None);
        let back = cursor.advance(crate::render::Offset::new(190.0, 70.0), bounds, None);
        assert_eq!(
            back.dy, 40.0,
            "the horizontal reset did not touch the vertical origin"
        );
    }

    #[test]
    fn each_edge_is_armed_and_spent_on_its_own_axis() {
        // Four flags, not one: leaving through the right and coming back
        // must not reset the vertical origin, or a diagonal drag would jump.
        let bounds = crate::engine::Rect::ltrb(0.0, 0.0, 100.0, 50.0);
        let mut cursor = FloatingCursor::default();
        cursor.advance(crate::render::Offset::new(50.0, 25.0), bounds, Some(true));
        // Out to the right only; y stays put.
        cursor.advance(crate::render::Offset::new(200.0, 25.0), bounds, None);
        cursor.advance(crate::render::Offset::new(190.0, 25.0), bounds, None);
        let back = cursor.advance(crate::render::Offset::new(180.0, 25.0), bounds, None);
        assert_eq!(back.dx, 90.0);
        assert_eq!(back.dy, 25.0, "the vertical origin was not touched");
    }

    // -- Where the caret is drawn, tick 273 ----------------------------------
    //
    // `depth.py` reports `RenderEditable` at 22 of 97, and the whole-crate
    // check does not explain it away: fifty-one members have no hit anywhere.
    // The first class in four where the ratio is a real gap.

    #[test]
    fn the_caret_is_drawn_over_the_glyphs_on_apple_platforms_and_under_them_elsewhere() {
        // It shows wherever a glyph overlaps the caret's column -- a
        // descender, an italic, a wide script -- and the two answers put the
        // caret in front of that ink or behind it.
        use crate::editable_text::TargetPlatform;
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert!(CaretGeometry::of(platform).above_text, "{platform:?}");
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert!(!CaretGeometry::of(platform).above_text, "{platform:?}");
        }
    }

    #[test]
    fn the_offset_is_in_device_pixels_and_everything_else_here_is_not() {
        // Upstream says it in the constant's own doc: "This value is in
        // device pixels, not logical pixels as is typically used throughout
        // the codebase." A port that took -2 for a logical value would move
        // the caret twice as far on a 2x screen.
        use crate::editable_text::TargetPlatform;
        let ios = CaretGeometry::of(TargetPlatform::IOS);
        assert_eq!(ios.offset_device_pixels, -2.0);
        assert_eq!(ios.offset_in_logical_pixels(1.0), -2.0);
        assert_eq!(ios.offset_in_logical_pixels(2.0), -1.0);
        assert_eq!(ios.offset_in_logical_pixels(4.0), -0.5);

        // Negative on purpose: iOS puts its caret on the *leading* edge of
        // the character it sits before, which is what makes it look like it
        // belongs to the letter after rather than the one before.
        assert!(ios.offset_device_pixels < 0.0);

        // And the platforms that do not shift it are unaffected by the ratio.
        let android = CaretGeometry::of(TargetPlatform::Android);
        assert_eq!(android.offset_in_logical_pixels(3.0), 0.0);
    }

    #[test]
    fn a_zero_ratio_does_not_divide_by_it() {
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            CaretGeometry::of(TargetPlatform::IOS).offset_in_logical_pixels(0.0),
            0.0
        );
    }

    #[test]
    fn the_two_apple_platforms_disagree_about_exactly_one_row() {
        // An iOS caret fades in and out; a macOS one blinks square. Every
        // other row of the table has them together, which is what makes this
        // one worth naming.
        use crate::editable_text::TargetPlatform;
        let ios = CaretGeometry::of(TargetPlatform::IOS);
        let mac = CaretGeometry::of(TargetPlatform::MacOS);
        assert!(ios.opacity_animates);
        assert!(!mac.opacity_animates);
        assert_eq!(ios.above_text, mac.above_text);
        assert_eq!(ios.radius, mac.radius);
        assert_eq!(ios.offset_device_pixels, mac.offset_device_pixels);
    }

    #[test]
    fn only_the_apple_platforms_round_the_caret() {
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            CaretGeometry::of(TargetPlatform::IOS).radius,
            Some(CaretGeometry::APPLE_RADIUS)
        );
        assert_eq!(CaretGeometry::APPLE_RADIUS, 2.0);
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert_eq!(
                CaretGeometry::of(platform).radius,
                None,
                "{platform:?}: a square caret"
            );
        }
    }

    // -- What a text field refuses, tick 270 ---------------------------------
    //
    // `TextField`'s constructor upstream is eight asserts and three derived
    // defaults, and this port had none of them. Five of the eight are about
    // *pairs* of fields, which is why each has its own message rather than a
    // shared bounds check.

    #[test]
    fn an_obscured_field_cannot_be_multiline() {
        // Upstream's own words: 'Obscured fields cannot be multiline.'
        let password = TextField::new(1).obscured();
        assert_eq!(password.validate(), Ok(()));
        assert_eq!(
            {
                let mut f = TextField::new(1).obscured();
                f.max_lines = MaxLines::Growing;
                f
            }
            .validate(),
            Err(TextFieldError::ObscuredAndMultiline)
        );
        assert_eq!(
            {
                let mut f = TextField::new(1).obscured();
                f.max_lines = MaxLines::Bounded(3);
                f
            }
            .validate(),
            Err(TextFieldError::ObscuredAndMultiline)
        );
        // And a multiline field that is not obscured is fine, so the rule is
        // about the pair.
        assert_eq!(TextField::new(1).multiline().validate(), Ok(()));
    }

    #[test]
    fn an_obscured_field_turns_off_the_smart_punctuation() {
        // Upstream derives both `smartDashesType` and `smartQuotesType` from
        // `obscureText`. An IME that helpfully turns `--` into an em-dash has
        // silently changed a password into something the reader cannot see
        // and cannot retype.
        assert!(TextField::new(1).smart_punctuation());
        assert!(!TextField::new(1).obscured().smart_punctuation());
    }

    #[test]
    fn minus_one_is_a_legal_max_length_and_zero_is_not() {
        // `noMaxLength` is -1: show the character counter and enforce
        // nothing. A port reading the assert as "a positive number" would
        // refuse the one value that says so.
        assert_eq!(TextField::NO_MAX_LENGTH, -1);
        assert_eq!(
            TextField::new(1)
                .with_max_length(TextField::NO_MAX_LENGTH)
                .validate(),
            Ok(())
        );
        assert_eq!(
            TextField::new(1).with_max_length(0).validate(),
            Err(TextFieldError::NonPositiveMaxLength)
        );
        assert_eq!(
            TextField::new(1).with_max_length(-2).validate(),
            Err(TextFieldError::NonPositiveMaxLength),
            "-1 is the sentinel, not 'any negative'"
        );
        assert_eq!(TextField::new(1).with_max_length(100).validate(), Ok(()));
    }

    #[test]
    fn a_field_that_fills_its_parent_may_not_also_count_lines() {
        // 'minLines and maxLines must be null when expands is true.' A field
        // that takes whatever height it is offered has no line count to be
        // asked about.
        assert_eq!(
            {
                let mut f = TextField::new(1).with_expands(true);
                f.max_lines = MaxLines::Growing;
                f
            }
            .validate(),
            Ok(()),
            "growing is this port's spelling of a null maxLines"
        );
        assert_eq!(
            TextField::new(1).with_expands(true).validate(),
            Err(TextFieldError::ExpandsWithLineCount),
            "and the default single-line is a maxLines of 1"
        );
        assert_eq!(
            {
                let mut f = TextField::new(1).with_min_lines(2).with_expands(true);
                f.max_lines = MaxLines::Growing;
                f
            }
            .validate(),
            Err(TextFieldError::ExpandsWithLineCount)
        );
    }

    #[test]
    fn the_smaller_line_count_may_not_be_the_larger_one() {
        // "minLines can't be greater than maxLines", and a single-line field
        // is `maxLines: 1`, so the same conflict is reachable two ways.
        assert_eq!(
            {
                let mut f = TextField::new(1).with_min_lines(5);
                f.max_lines = MaxLines::Bounded(2);
                f
            }
            .validate(),
            Err(TextFieldError::MinLinesAboveMaxLines)
        );
        assert_eq!(
            TextField::new(1).with_min_lines(3).validate(),
            Err(TextFieldError::MinLinesAboveMaxLines),
            "single-line is maxLines 1"
        );
        // Equal is allowed -- upstream's test is `>=`.
        assert_eq!(
            {
                let mut f = TextField::new(1).with_min_lines(4);
                f.max_lines = MaxLines::Bounded(4);
                f
            }
            .validate(),
            Ok(())
        );
        // And a growing field has no upper bound to exceed.
        assert_eq!(
            {
                let mut f = TextField::new(1).with_min_lines(9);
                f.max_lines = MaxLines::Growing;
                f
            }
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn neither_line_count_may_be_zero() {
        assert_eq!(
            {
                let mut f = TextField::new(1);
                f.max_lines = MaxLines::Bounded(0);
                f
            }
            .validate(),
            Err(TextFieldError::NonPositiveMaxLines)
        );
        assert_eq!(
            TextField::new(1).with_min_lines(0).validate(),
            Err(TextFieldError::NonPositiveMinLines)
        );
    }

    #[test]
    fn a_newline_action_needs_a_keyboard_that_can_produce_one() {
        // Upstream's message says what to do: 'Use keyboardType
        // TextInputType.multiline when using TextInputAction.newline on a
        // multiline TextField.' It asserts rather than fixing it silently,
        // with a comment saying why -- changing a value the caller set would
        // surprise them.
        assert_eq!(
            {
                let mut f = TextField::new(1);
                f.max_lines = MaxLines::Growing;
                f.action = TextInputAction::Newline;
                f.input_type = TextInputType::Text;
                f
            }
            .validate(),
            Err(TextFieldError::NewlineActionOnASingleLineKeyboard)
        );
        // Naming the multiline keyboard is the fix.
        assert_eq!(TextField::new(1).multiline().validate(), Ok(()));
        // And a single-line field with a newline action is upstream's other
        // way out of it.
        assert_eq!(
            {
                let mut f = TextField::new(1);
                f.action = TextInputAction::Newline;
                f
            }
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn a_multiline_field_asks_for_a_multiline_keyboard_by_itself() {
        // `keyboardType ?? (maxLines == 1 ? text : multiline)` -- the same
        // fact the eighth assert refuses to let a caller contradict.
        assert_eq!(
            TextField::new(1).effective_input_type(),
            TextInputType::Text
        );
        assert_eq!(
            {
                let mut f = TextField::new(1);
                f.max_lines = MaxLines::Growing;
                f
            }
            .effective_input_type(),
            TextInputType::Multiline
        );
        // A named type wins: this fills in a null, it does not override.
        assert_eq!(
            {
                let mut f = TextField::new(1);
                f.max_lines = MaxLines::Growing;
                f.input_type = TextInputType::Phone;
                f
            }
            .effective_input_type(),
            TextInputType::Phone
        );
    }

    #[test]
    fn tab_moves_between_two_fields_and_takes_the_session_with_it() {
        let _messenger = install();
        text_input::reset();
        crate::focus::reset();

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                crate::framework::stateful(TextField::new(1)),
                crate::framework::stateful(TextField::new(2)),
            ],
            |children| {
                let mut column = crate::render::RenderFlex::column();
                for child in children {
                    column = column.push(child);
                }
                Box::new(column)
            },
        ));
        let _ = tree.build_render_tree();

        assert!(
            !text_input::is_editing(),
            "nothing focused, nothing editing"
        );

        // Tab into the first field: it opens a session.
        assert!(crate::focus::next());
        assert_eq!(crate::focus::focused(), Some(1));
        tree.rebuild_dirty();
        assert!(
            text_input::is_editing(),
            "the focused field should be editing"
        );

        // Tab to the second: the first one's session goes with it. What the
        // platform sees is one client at a time either way; what this asserts
        // is that the *field* let go, which is what makes its caret stop
        // blinking.
        assert!(crate::focus::next());
        assert_eq!(crate::focus::focused(), Some(2));
        tree.rebuild_dirty();
        assert!(text_input::is_editing());
        drop(tree);
    }

    fn value(text: &str, caret: i32, composing: (i32, i32)) -> TextEditingValue {
        TextEditingValue {
            text: text.to_string(),
            selection_base: caret,
            selection_extent: caret,
            composing_base: composing.0,
            composing_extent: composing.1,
        }
    }

    #[test]
    fn the_caret_is_measured_from_the_text_before_it() {
        // What is assertable here is the *choice of prefix*: it is where a
        // byte offset and a UTF-16 offset get confused. The width itself is
        // the stub's model rather than a font's, so what is checked is that
        // the caret is measured from the right substring, not that it lands
        // on a particular number.
        //
        // This used to assert zero, with a comment saying every paragraph
        // metric the stub returns is zero. They are modelled now.
        let field = RenderEditable::new(value("ab\u{4e2d}", 3, (-1, -1)));
        // Three UTF-16 units -- 'a', 'b' and one BMP character -- five bytes.
        assert_eq!(field.value.caret_bytes(), Some(5));
        let after_three = field.caret_offset();
        assert!(after_three > 0.0, "three characters have a width");

        let empty = RenderEditable::new(value("abc", 0, (-1, -1)));
        assert_eq!(empty.caret_offset(), 0.0, "nothing before it to measure");

        // The prefix, and only the prefix: a caret after two characters sits
        // short of one after three, and the text beyond it does not count.
        let after_two = RenderEditable::new(value("ab\u{4e2d}", 2, (-1, -1))).caret_offset();
        assert!(
            after_two < after_three,
            "{after_two} should be short of {after_three}"
        );
        let longer_tail = RenderEditable::new(value("ab\u{4e2d}defgh", 2, (-1, -1)));
        assert_eq!(
            longer_tail.caret_offset(),
            after_two,
            "what follows the caret is not in front of it"
        );

        // A caret inside a surrogate pair is not a position, and measuring
        // from it would slice a string mid-character.
        let split = RenderEditable::new(value("\u{1F600}", 1, (-1, -1)));
        assert_eq!(split.value.caret_bytes(), None);
        assert_eq!(split.caret_offset(), 0.0);
    }

    #[test]
    fn a_selected_run_is_the_part_between_the_two_ends() {
        let mut field = RenderEditable::new(value("ab\u{4e2d}cd", 0, (-1, -1)));
        field.value.selection_base = 1;
        field.value.selection_extent = 4;
        // UTF-16 units 1..4 over "ab中cd" is "b中c" -- bytes 1..6, because the
        // character in the middle is one unit and three bytes. Counting in the
        // wrong one of those is the mistake this is here to catch.
        assert_eq!(field.value.selection_bytes(), Some(1..6));
        assert_eq!(&field.value.text[1..6], "b\u{4e2d}c");

        // Dragged the other way. The direction is the platform's business and
        // the painter wants an ordered range.
        field.value.selection_base = 4;
        field.value.selection_extent = 1;
        assert_eq!(field.value.selection_bytes(), Some(1..6));
    }

    #[test]
    fn a_caret_is_not_a_selection() {
        // The ordinary case, and the one that decides whether the highlight is
        // drawn at all: base equal to extent is a caret, not a zero-width
        // selection.
        let field = RenderEditable::new(value("abc", 2, (-1, -1)));
        assert!(!field.value.has_selection());
        assert_eq!(field.value.selection_bytes(), None);
    }

    #[test]
    fn a_selection_inside_a_character_is_not_a_range() {
        // Half of a surrogate pair. Slicing there would cut a character in two,
        // and the platform can send it while a drag is in progress.
        let mut field = RenderEditable::new(value("\u{1F600}ab", 0, (-1, -1)));
        field.value.selection_base = 1;
        field.value.selection_extent = 3;
        assert_eq!(field.value.selection_bytes(), None);
    }

    #[test]
    fn a_field_takes_the_width_it_is_given_and_one_lines_height() {
        let mut field = RenderEditable::new(value("abc", 3, (-1, -1)));
        let size = field.layout(BoxConstraints::tight(180.0, 30.0));
        assert_eq!(size.width, 180.0);
        assert!(size.height > 0.0);
    }

    #[test]
    fn tapping_a_field_starts_editing_and_tapping_another_stops_the_first() {
        // The exclusivity a focus tree would give. There is none, and the
        // client id gives the same guarantee for text: one field at a time.
        let _recorder = install();
        text_input::reset();
        assert!(!text_input::is_editing());

        let first = text_input::attach(
            Box::new(FieldClient {
                handle: StateHandle::detached(),
                on_changed: None,
                on_submitted: None,
                multiline: false,
                last: TextEditingValue::default(),
            }),
            TextInputConfiguration::default(),
        );
        assert!(first.is_attached());

        let second = text_input::attach(
            Box::new(FieldClient {
                handle: StateHandle::detached(),
                on_changed: None,
                on_submitted: None,
                multiline: false,
                last: TextEditingValue::default(),
            }),
            TextInputConfiguration::default(),
        );
        assert!(!first.is_attached(), "the first field was detached");
        assert!(second.is_attached());
    }

    #[test]
    fn what_the_platform_reports_reaches_the_applications_callback() {
        // The application sees text, not a TextEditingValue, and never sees
        // the channel.
        let _recorder = install();
        text_input::reset();
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = seen.clone();

        let mut client = FieldClient {
            handle: StateHandle::detached(),
            on_changed: Some(Rc::new(move |text: &str| {
                recorded.borrow_mut().push(text.to_string())
            })),
            on_submitted: None,
            multiline: false,
            last: TextEditingValue::default(),
        };
        client.update_editing_value(value("zh", 2, (0, 2)));
        client.update_editing_value(value("\u{4e2d}", 1, (-1, -1)));

        assert_eq!(
            seen.borrow().as_slice(),
            &["zh".to_string(), "\u{4e2d}".to_string()]
        );
    }

    #[test]
    fn submitting_reports_the_text_the_field_held() {
        // The platform sends the action without the text, so the client has to
        // have kept it.
        let _recorder = install();
        text_input::reset();
        let submitted = Rc::new(std::cell::RefCell::new(None));
        let recorded = submitted.clone();

        let mut client = FieldClient {
            handle: StateHandle::detached(),
            on_changed: None,
            on_submitted: Some(Rc::new(move |text: &str| {
                *recorded.borrow_mut() = Some(text.to_string())
            })),
            multiline: false,
            last: TextEditingValue::default(),
        };
        client.update_editing_value(value("done", 4, (-1, -1)));
        client.perform_action(TextInputAction::Done);

        assert_eq!(*submitted.borrow(), Some("done".to_string()));
    }

    #[test]
    fn a_newline_in_a_multiline_field_is_not_a_submission() {
        // Upstream `_performAction`: the newline branch returns without
        // `_finalizeEditing` when the field takes more than one line, because
        // Enter there means "new line" and the platform has already inserted
        // it -- a submission as well would be reporting an Enter that was not
        // pressed.
        let _recorder = install();
        text_input::reset();
        crate::focus::reset();
        let submitted = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let recorded = submitted.clone();

        let mut multiline_client = FieldClient {
            handle: StateHandle::detached(),
            on_changed: None,
            on_submitted: Some(Rc::new(move |text: &str| {
                recorded.borrow_mut().push(text.to_string())
            })),
            multiline: true,
            last: TextEditingValue::new("a line\n"),
        };
        multiline_client.perform_action(TextInputAction::Newline);
        assert!(
            submitted.borrow().is_empty(),
            "a newline in a multiline field is not Enter"
        );

        // Every other action still finishes editing, with the text as it
        // stands -- the newline the platform inserted included.
        multiline_client.perform_action(TextInputAction::Done);
        assert_eq!(submitted.borrow().as_slice(), &["a line\n".to_string()]);

        // And a single-line field treats even a stray newline as "done",
        // which is upstream's `!_isMultiline` case.
        submitted.borrow_mut().clear();
        let recorded_twice = submitted.clone();
        let mut single_client = FieldClient {
            handle: StateHandle::detached(),
            on_changed: None,
            on_submitted: Some(Rc::new(move |text: &str| {
                recorded_twice.borrow_mut().push(text.to_string())
            })),
            multiline: false,
            last: TextEditingValue::new("one line"),
        };
        single_client.perform_action(TextInputAction::Newline);
        assert_eq!(submitted.borrow().as_slice(), &["one line".to_string()]);
    }

    #[test]
    fn a_next_action_moves_the_keyboard_to_the_next_field() {
        // The action arrives the way the platform sends it, through the
        // channel, to whichever client it last set -- so the id is read back
        // out of the `setClient` the focused field sent when its session
        // opened.
        let _messenger = install();
        text_input::reset();
        crate::focus::reset();

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                crate::framework::stateful(TextField::new(1)),
                crate::framework::stateful(TextField::new(2)),
            ],
            |children| {
                let mut column = crate::render::RenderFlex::column();
                for child in children {
                    column = column.push(child);
                }
                Box::new(column)
            },
        ));
        let _ = tree.build_render_tree();

        assert!(crate::focus::next());
        assert_eq!(crate::focus::focused(), Some(1));
        tree.rebuild_dirty();

        let id = client_id(&_messenger);
        let call = crate::services::codec::JsonMethodCodec
            .encode_method_call(&crate::services::codec::MethodCall::new(
                "TextInputClient.performAction",
                crate::services::codec::Value::List(vec![
                    crate::services::codec::Value::I64(id),
                    crate::services::codec::Value::from("TextInputAction.next"),
                ]),
            ))
            .unwrap();
        _messenger.deliver("flutter/textinput", &call, 0);

        assert_eq!(
            crate::focus::focused(),
            Some(2),
            "next handed the keyboard to the second field"
        );
        drop(tree);
    }

    /// The client id the field under test registered with, read out of the
    /// `TextInput.setClient` it sent when its session opened. The id is a
    /// thread-local counter the tests share, so it cannot be assumed.
    fn client_id(recorder: &crate::services::tests_support::Recorder) -> i64 {
        use crate::services::codec::MethodCodec;
        for (channel, bytes, _) in recorder.sent() {
            if channel != "flutter/textinput" {
                continue;
            }
            let call = crate::services::codec::JsonMethodCodec
                .decode_method_call(&bytes)
                .unwrap();
            if call.method == "TextInput.setClient" {
                return call
                    .arguments
                    .as_list()
                    .expect("a client id and its configuration")[0]
                    .as_i64()
                    .expect("the client id");
            }
        }
        panic!("no field ever opened a session");
    }

    #[test]
    fn an_obscured_field_draws_bullets_and_sends_the_real_text() {
        // Drawn as bullets, sent as itself: the platform runs the IME against
        // the text and is the side that knows not to log it.
        let value = value("secret", 6, (-1, -1));
        let shown = TextEditingValue {
            text: "\u{2022}".repeat(value.text.chars().count()),
            ..value.clone()
        };
        assert_eq!(
            shown.text,
            "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"
        );
        assert_eq!(shown.selection_extent, value.selection_extent);
    }

    // -- Multi-line -----------------------------------------------------------

    /// Ten logical pixels per character, which the stubbed engine cannot
    /// provide: its metrics are all zero, and a zero is indistinguishable from
    /// "fits". The geometry is decided by the widths only through comparison,
    /// so a fake that makes them non-zero exercises every branch of the
    /// wrapping the real one would.
    fn ten_a_character(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    #[test]
    fn words_wrap_at_the_box_width_and_newlines_start_new_lines() {
        // A word moves whole: "aaa " fits in fifty, "aaa bb " does not, so
        // the second word starts the second line.
        let lines = wrap_lines("aaa bb cc", 50.0, &ten_a_character);
        let ranges: Vec<(usize, usize)> = lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(ranges, vec![(0, 4), (4, 9)]);

        // A newline ends its line wherever the wrapping was, and the next
        // line starts after it.
        let lines = wrap_lines("ab\ncd", 500.0, &ten_a_character);
        let ranges: Vec<(usize, usize)> = lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(ranges, vec![(0, 2), (3, 5)]);

        // An unbreakable run too wide for the line is broken by character:
        // three characters of ten pixels in thirty, twice.
        let lines = wrap_lines("aaaaaa", 30.0, &ten_a_character);
        let ranges: Vec<(usize, usize)> = lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(ranges, vec![(0, 3), (3, 6)]);

        // The spaces after a word travel with it, so a caret placed among
        // them is at the end of the line they belong to.
        let lines = wrap_lines("aa   bb", 50.0, &ten_a_character);
        let ranges: Vec<(usize, usize)> = lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(ranges, vec![(0, 5), (5, 7)]);

        // Empty text is one empty line: the caret still has a line to be on,
        // which is what keeps an empty growing field one line tall.
        let lines = wrap_lines("", 50.0, &ten_a_character);
        let ranges: Vec<(usize, usize)> = lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(ranges, vec![(0, 0)]);
    }

    #[test]
    fn the_height_of_a_field_follows_its_line_limit() {
        // Upstream `performLayout`: a single-line field is one line tall, a
        // bounded one is always its maximum (its `minLines` defaults to its
        // `maxLines`), and a growing one is as tall as the wrapped text, with
        // one line as the floor for an empty field.
        assert_eq!(preferred_height(3, 10.0, MaxLines::Single), 10.0);
        assert_eq!(preferred_height(1, 10.0, MaxLines::Bounded(3)), 30.0);
        assert_eq!(preferred_height(9, 10.0, MaxLines::Bounded(3)), 30.0);
        assert_eq!(preferred_height(5, 10.0, MaxLines::Growing), 50.0);
        assert_eq!(preferred_height(0, 10.0, MaxLines::Growing), 10.0);
    }

    #[test]
    fn a_field_that_allows_several_lines_lays_out_by_them() {
        // The wire format counts UTF-16, so a caret on the second line of
        // "ab\ncd" is selection extent 3: two for "ab", one for the newline.
        let field =
            RenderEditable::new(value("ab\ncd", 3, (-1, -1))).with_max_lines(MaxLines::Growing);
        let lines = field.visual_lines(500.0);
        let ranges: Vec<(usize, usize)> = lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(ranges, vec![(0, 2), (3, 5)]);

        // Which line the caret is on is the wrapping's decision: the second,
        // one line height down. Its x is the width of what precedes it on that
        // line, which the stub now measures -- so both halves are checkable.
        let caret = field
            .caret_rect(&lines, 10.0)
            .expect("the caret is at a boundary");
        assert_eq!(caret.top, 10.0);
        assert_eq!(caret.height(), 10.0);
        // Extent 3 is the *start* of the second line -- "ab" is on the first
        // one -- so nothing precedes it and its x is the edge. A caret's
        // offset is the prefix within its own line, not the text above it.
        assert_eq!(caret.left, 0.0);

        // A second case was tried here and taken out: extent 4 does not put
        // the caret one character into the second line, and working out what
        // it does mean is `value`'s business rather than this test's. The
        // offset-follows-the-prefix rule is covered where the prefix is the
        // subject, in the caret-offset test above.
    }

    #[test]
    fn a_caret_at_a_soft_wrap_sits_at_the_start_of_the_next_line() {
        // "aaa bb" in fifty pixels wraps after the spaces: line 0 is "aaa "
        // (bytes 0..4) and line 1 is "bb". The boundary byte belongs to both
        // lines and the caret goes on the second: upstream's `TextSelection`
        // defaults to `TextAffinity.downstream`, so a caret typed past the
        // last character of a wrapped line shows at the start of the next
        // line rather than trailing on the one that just filled.
        let lines = wrap_lines("aaa bb", 50.0, &ten_a_character);
        assert_eq!(
            caret_line(&lines, 4),
            1,
            "downstream: the next line's start"
        );
        assert_eq!(caret_line(&lines, 3), 0);
        // And at a hard break the newline itself belongs to no line, so the
        // affinity has nothing to decide: before it is the end of "ab", after
        // it the start of "cd".
        let lines = wrap_lines("ab\ncd", 500.0, &ten_a_character);
        assert_eq!(caret_line(&lines, 2), 0);
        assert_eq!(caret_line(&lines, 3), 1);
    }

    #[test]
    fn a_tap_finds_the_position_nearest_the_finger() {
        // Line 0 of "aaa bb" at ten pixels a character is "aaa ": its
        // boundaries sit at x = 0, 10, 20, 30, 40. Upstream
        // `getPositionForOffset` answers the closest text position to the
        // pointer; this is the same walk without the paragraph.
        let lines = wrap_lines("aaa bb", 50.0, &ten_a_character);
        assert_eq!(
            caret_position_at(
                "aaa bb",
                &lines,
                10.0,
                Offset::new(24.0, 0.0),
                &ten_a_character
            ),
            2,
            "24 is nearer 20 than 30"
        );
        assert_eq!(
            caret_position_at(
                "aaa bb",
                &lines,
                10.0,
                Offset::new(26.0, 0.0),
                &ten_a_character
            ),
            3,
            "26 is nearer 30"
        );
        // A tie leans to the earlier boundary, and so to the character the
        // finger is on rather than the one after it.
        assert_eq!(
            caret_position_at(
                "aaa bb",
                &lines,
                10.0,
                Offset::new(25.0, 0.0),
                &ten_a_character
            ),
            2
        );
        // A tap below the last line is a tap on it, and past its end is its
        // end -- upstream's answer for a position beyond the text.
        assert_eq!(
            caret_position_at(
                "aaa bb",
                &lines,
                10.0,
                Offset::new(100.0, 95.0),
                &ten_a_character
            ),
            6
        );
    }

    #[test]
    fn the_caret_blinks_every_half_second_while_the_field_is_editing() {
        // Upstream's `_kCursorBlinkHalfPeriod`: half a second shown, half a
        // second hidden, starting shown. Here the clock is frame time, moved
        // by `advance`, and the states it moves are the field's own.
        let _messenger = install();
        text_input::reset();
        crate::focus::reset();

        let field = TextField::new(1);
        let mut state = TextFieldState::default();

        // Nothing being edited: there is no clock to run.
        assert!(!field.advance(&mut state, 1_000));
        assert!(!state.caret_blink_on);

        // A session opens -- as focus gain opens one -- and the caret is
        // shown from the first frame.
        state.connection = Some(text_input::attach(
            Box::new(FieldClient {
                handle: StateHandle::detached(),
                on_changed: None,
                on_submitted: None,
                multiline: false,
                last: TextEditingValue::default(),
            }),
            TextInputConfiguration::default(),
        ));
        assert!(field.advance(&mut state, 10_000));
        assert!(
            state.caret_blink_on,
            "the blink starts with the caret shown"
        );

        // Half a second hides it; the next half second shows it again.
        assert!(field.advance(&mut state, 400_000));
        assert!(state.caret_blink_on, "400ms is not half a second");
        assert!(field.advance(&mut state, 510_000));
        assert!(!state.caret_blink_on, "500ms hides the caret");
        assert!(field.advance(&mut state, 900_000));
        assert!(!state.caret_blink_on, "and 390ms more does not show it");
        assert!(field.advance(&mut state, 1_010_000));
        assert!(
            state.caret_blink_on,
            "the second half second shows it again"
        );

        // The session ending stops the clock. The one frame that clears the
        // caret is still asked for -- it is the frame that paints the caret
        // away -- and after it there is nothing to animate.
        text_input::reset();
        assert!(field.advance(&mut state, 1_010_001));
        assert!(!state.caret_blink_on);
        assert!(
            !field.advance(&mut state, 1_010_002),
            "the caret is gone; so is the clock"
        );
    }

    #[test]
    fn a_tap_puts_the_caret_where_the_finger_was() {
        // What the reader sees of a tap on a field: the keyboard comes up, and
        // the platform is told the caret sits where the finger landed. The
        // stubbed engine measures every run as zero and every line as zero
        // tall, so all x positions tie and lean earliest -- which still
        // decides the line, and with it the offset, from the pointer's y.
        let _messenger = install();
        text_input::reset();
        crate::focus::reset();

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::framework::stateful(TextField::new(1).multiline()));

        // Focus the field and give it two lines, the way the platform would:
        // through the channel.
        assert!(crate::focus::focus(1));
        let id = client_id(&_messenger);
        _messenger.deliver(
            "flutter/textinput",
            &state_message(id, "ab\ncd", 5, 5, (-1, -1)),
            0,
        );
        let root = pump(&mut tree);

        // A tap on the second line. The lines are zero tall in the stub, so
        // any positive y is past them all: the last line, whose earliest
        // boundary is byte 3 -- "ab\ncd" with the caret before the "c".
        let mut router = crate::gestures::GestureRouter::new();
        router.dispatch(
            &root,
            &event(crate::gestures::PointerChange::Down, 10.0, 40.0, 0.0, 0.0),
        );
        router.dispatch(
            &root,
            &event(crate::gestures::PointerChange::Up, 10.0, 40.0, 0.0, 0.0),
        );

        assert_eq!(
            crate::focus::focused(),
            Some(1),
            "the tap focused the field"
        );
        // The line the tap landed on is what this is about. The exact index
        // within it depends on how wide the stub thinks each glyph is, so what
        // is asserted is the line -- second, hence at or past its first
        // character -- rather than a number that would be about the model.
        let (base, extent) = last_selection(&_messenger).expect("a selection");
        assert_eq!(base, extent, "a tap collapses the selection");
        assert!(
            base >= 3,
            "the caret went to the second line, which starts at 3: {base}"
        );

        // And a tap at the top of the field is the first line's start.
        router.dispatch(
            &root,
            &event(crate::gestures::PointerChange::Down, 5.0, 0.0, 0.0, 0.0),
        );
        router.dispatch(
            &root,
            &event(crate::gestures::PointerChange::Up, 5.0, 0.0, 0.0, 0.0),
        );
        // The first line, again by line rather than by index: five pixels in
        // is within the first glyph or just past it depending on how wide the
        // stub makes one, and the line is the part this test is about.
        let (base, extent) = last_selection(&_messenger).expect("a selection");
        assert_eq!(base, extent);
        assert!(base < 3, "the caret went to the first line: {base}");
        drop(tree);
    }

    #[test]
    fn a_mouse_drag_selects_from_where_the_press_began() {
        // Upstream's `onDragSelectionUpdate` for a precise pointer:
        // `selectPositionAt(from: dragStart, to: current)` -- the base pins
        // where the press began and the extent follows the pointer, which is
        // what highlights the run. The stub measures every run as zero wide
        // and every line as zero tall, so the lines are decided by y and the
        // offsets within one lean earliest, as in
        // `a_tap_puts_the_caret_where_the_finger_was`.
        let _messenger = install();
        text_input::reset();
        crate::focus::reset();

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::framework::stateful(TextField::new(1).multiline()));
        assert!(crate::focus::focus(1));
        let id = client_id(&_messenger);
        _messenger.deliver(
            "flutter/textinput",
            &state_message(id, "ab\ncd", 5, 5, (-1, -1)),
            0,
        );
        let root = pump(&mut tree);

        // Press on the first line, drag onto the second, let go. A mouse's
        // pan slop is two pixels, so forty of travel is no tap.
        let mut router = crate::gestures::GestureRouter::new();
        router.dispatch(
            &root,
            &mouse_event(crate::gestures::PointerChange::Down, 10.0, 0.0, 0.0, 0.0, 1),
        );
        router.dispatch(
            &root,
            &mouse_event(crate::gestures::PointerChange::Move, 10.0, 40.0, 0.0, 40.0, 1),
        );
        router.dispatch(
            &root,
            &mouse_event(crate::gestures::PointerChange::Up, 10.0, 40.0, 0.0, 0.0, 0),
        );

        let drag = last_selection(&_messenger).expect("a selection");
        assert_ne!(drag.0, drag.1, "a drag marks out a run rather than collapsing");
        assert!(drag.0 < 3, "the base stayed where the press began: {}", drag.0);
        assert!(
            drag.1 >= 3,
            "the extent followed the pointer onto the second line: {}",
            drag.1
        );

        // A move without the primary button is a hover, and a hover selects
        // nothing: the press is over, so the run the drag made stands.
        router.dispatch(
            &root,
            &mouse_event(crate::gestures::PointerChange::Down, 10.0, 0.0, 0.0, 0.0, 1),
        );
        router.dispatch(
            &root,
            &mouse_event(crate::gestures::PointerChange::Move, 10.0, 40.0, 0.0, 40.0, 0),
        );
        let hover = last_selection(&_messenger).expect("a selection");
        assert_eq!(
            hover, drag,
            "the button being up made the move a hover: {hover:?}"
        );
        drop(tree);
    }

    /// Rebuilds, lays out and paints, and hands back the render tree it
    /// produced: the frame the tap will act on is the frame the reader can
    /// see.
    fn pump(tree: &mut crate::framework::ElementTree) -> crate::render::BoxedRender {
        use crate::render::RenderBox;
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("a render tree");
        root.layout(BoxConstraints::tight(200.0, 100.0));
        let mut layers = crate::engine::LayerTree::new(200, 100);
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(200.0, 100.0));
            root.paint(&mut context, Offset::ZERO);
        }
        root
    }

    /// A pointer event, in the shape the shell delivers them.
    fn event(
        change: crate::gestures::PointerChange,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    ) -> crate::gestures::PointerEvent {
        crate::gestures::PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change,
            kind: crate::gestures::PointerKind::Touch,
            signal_kind: crate::gestures::SignalKind::None,
            buttons: 1,
            time_stamp_micros: 0,
            position: Offset::new(x, y),
            delta: Offset::new(dx, dy),
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: Offset::new(x, y),
        }
    }

    /// A mouse event, in the shape the shell delivers them: the primary
    /// button's bit in `buttons` says whether the move is a drag or a hover.
    /// The delta is what the router's slop is measured from, so a drag's
    /// moves carry their travel.
    fn mouse_event(
        change: crate::gestures::PointerChange,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        buttons: i32,
    ) -> crate::gestures::PointerEvent {
        crate::gestures::PointerEvent {
            kind: crate::gestures::PointerKind::Mouse,
            buttons,
            ..event(change, x, y, dx, dy)
        }
    }

    /// The editing state the field last told the platform it holds.
    fn last_selection(recorder: &crate::services::tests_support::Recorder) -> Option<(i32, i32)> {
        use crate::services::codec::MethodCodec;
        for (channel, bytes, _) in recorder.sent().into_iter().rev() {
            if channel != "flutter/textinput" {
                continue;
            }
            let call = crate::services::codec::JsonMethodCodec
                .decode_method_call(&bytes)
                .unwrap();
            if call.method == "TextInput.setEditingState" {
                let base = call.arguments.get("selectionBase")?.as_i64()? as i32;
                let extent = call.arguments.get("selectionExtent")?.as_i64()? as i32;
                return Some((base, extent));
            }
        }
        None
    }

    /// The state message the host sends, in the host's own shape.
    fn state_message(
        id: i64,
        text: &str,
        base: i32,
        extent: i32,
        composing: (i32, i32),
    ) -> Vec<u8> {
        crate::services::codec::JsonMethodCodec
            .encode_method_call(&crate::services::codec::MethodCall::new(
                "TextInputClient.updateEditingState",
                crate::services::codec::Value::List(vec![
                    crate::services::codec::Value::I64(id),
                    crate::services::codec::Value::map([
                        (
                            "selectionAffinity",
                            crate::services::codec::Value::from("TextAffinity.downstream"),
                        ),
                        (
                            "selectionBase",
                            crate::services::codec::Value::I64(base as i64),
                        ),
                        (
                            "selectionExtent",
                            crate::services::codec::Value::I64(extent as i64),
                        ),
                        (
                            "selectionIsDirectional",
                            crate::services::codec::Value::Bool(false),
                        ),
                        (
                            "composingBase",
                            crate::services::codec::Value::I64(composing.0 as i64),
                        ),
                        (
                            "composingExtent",
                            crate::services::codec::Value::I64(composing.1 as i64),
                        ),
                        ("text", crate::services::codec::Value::from(text)),
                    ]),
                ]),
            ))
            .unwrap()
    }

    #[test]
    fn a_single_line_field_scrolls_horizontally_to_keep_the_caret_in_view() {
        // Upstream `_getOffsetToRevealCaret`, single-line branch: the viewport
        // is a hundred wide, the caret a hundred and forty in, so the field
        // scrolls until the caret's trailing edge is at the right edge.
        assert_eq!(reveal(140.0, 142.0, 100.0, 0.0, 103.0), 42.0);
        // Typing back towards the start scrolls back with it, until the
        // caret's leading edge is at the left edge.
        assert_eq!(reveal(20.0, 22.0, 100.0, 42.0, 103.0), 20.0);
        // A caret already in view moves nothing.
        assert_eq!(reveal(50.0, 52.0, 100.0, 42.0, 103.0), 42.0);
        // And there is no scrolling past what the content allows, caret or no.
        assert_eq!(reveal(190.0, 192.0, 100.0, 0.0, 50.0), 50.0);
    }

    #[test]
    fn a_multiline_field_scrolls_vertically_to_keep_the_caret_in_view() {
        // The same maths on the other axis: the caret is a line tall, and the
        // viewport shows one line at a time of it.
        assert_eq!(reveal(110.0, 120.0, 100.0, 0.0, 150.0), 20.0);
        // A line back above the window scrolls back to bring its top to the
        // viewport's top -- never further than the content allows.
        assert_eq!(reveal(0.0, 10.0, 100.0, 20.0, 150.0), 0.0);
        // Content that fits never scrolls at all.
        assert_eq!(reveal(90.0, 100.0, 100.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn a_selection_covers_one_rectangle_per_line_it_crosses() {
        let field =
            RenderEditable::new(value("aaa bb", 0, (-1, -1))).with_max_lines(MaxLines::Growing);
        let lines = wrap_lines("aaa bb", 50.0, &ten_a_character);
        // "aaa bb" selected whole: a rect on the wrapped first line and one
        // on the second. Which lines get a rect is the decision under test,
        // and the widths are now measurable too.
        let first = field.line_extent(lines[0], 0..6).expect("the first line");
        let second = field.line_extent(lines[1], 0..6).expect("the second");
        assert_eq!(first.0, 0.0, "each starts at its own line's edge");
        assert_eq!(second.0, 0.0);
        assert!(first.1 > 0.0 && second.1 > 0.0, "and each has a width");
        assert!(
            first.1 > second.1,
            "'aaa' is wider than 'bb': {} against {}",
            first.1,
            second.1
        );
        // A run entirely before this line gives it nothing to draw.
        assert!(field.line_extent(lines[1], 0..1).is_none());

        // A run that starts partway along has to say so twice over: its
        // rectangle begins after what precedes it, and ends before what
        // follows. Selecting only the middle "a" of "aaa" is the case that
        // separates those from a whole-line selection, where both are free.
        let (start, width) = field.line_extent(lines[0], 1..2).expect("the middle");
        assert!(start > 0.0, "one character precedes it: {start}");
        assert!(width > 0.0, "and it has a width of its own: {width}");
        assert!(
            start + width < first.1,
            "and stops short of the line's end: {start} + {width} against {}",
            first.1
        );
    }

    #[test]
    fn a_multiline_field_paints_without_a_real_text_stack() {
        // What this guards is that the multi-line paths -- per-line text,
        // per-line selection, composing, caret, report -- run at all against
        // the same canvas the single-line ones do. It used to add that the
        // stub made every metric zero so nothing wrapped; that is no longer
        // true, and the wrapping is covered by its own tests below.
        let mut layers = crate::engine::LayerTree::new(200, 200);
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            let mut field = RenderEditable::new(value("ab\u{4e2d}\ncd", 6, (0, 2)))
                .with_max_lines(MaxLines::Bounded(3))
                .with_caret(crate::engine::Color::BLACK, true);
            field.value.selection_base = 0;
            field.value.selection_extent = 6;
            let size = field.layout(BoxConstraints::tight(180.0, 60.0));
            assert_eq!(size.width, 180.0);
            field.paint(&mut context, Offset::new(10.0, 10.0));
        }
        drop(layers);
    }

    #[test]
    fn a_field_that_allows_more_lines_tells_the_platform_so() {
        // Upstream `TextField`: `maxLines` other than one means the multiline
        // keyboard, and `multiline()` means no limit at all.
        assert_eq!(TextField::new(1).max_lines, MaxLines::Single);
        assert_eq!(TextField::new(1).multiline().max_lines, MaxLines::Growing);
        let bounded = TextField::new(1).with_max_lines(3);
        assert_eq!(bounded.max_lines, MaxLines::Bounded(3));
        assert_eq!(bounded.input_type, TextInputType::Multiline);
        assert_eq!(bounded.action, TextInputAction::Newline);
        // One is the single-line field it already was.
        assert_eq!(
            TextField::new(1).with_max_lines(1).max_lines,
            MaxLines::Single
        );
    }
}

#[cfg(test)]
mod painted_field_tests {
    //! What a field puts on the glass, through what the canvas was told.
    //!
    //! Three of the rules here are written in comments beside the code and
    //! were, until the stubs started recording, comments and nothing else: the
    //! selection goes down before the glyphs, a run crossing a wrap is one
    //! rectangle per line, and there is no caret while a run is selected.
    //!
    //! The glyphs are a paragraph, and the recorder reads those back now: the
    //! text that was drawn and where it landed, though not its shaping. That
    //! is enough for the ordering rules, which is what two of the tests here
    //! were written around and could not ask.
    //!
    //! # The wrapping ones came back
    //!
    //! Two tests here were written, found unaskable, and removed a tick ago,
    //! because the stubs measured every string as nought by nought and so
    //! nothing ever wrapped. One failed honestly; the other passed by having
    //! nothing to iterate over, which is worse. The stub models text metrics
    //! now, and they are below.
    //!
    //! They assert **relations** rather than numbers -- more rectangles when
    //! narrower, each below the last -- because the model gives every glyph
    //! the same width and a real font would disagree with any number taken
    //! from it.

    use super::{MaxLines, RenderEditable, TextEditingValue};
    use crate::engine::{Color, LayerTree, TextStyle};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::render::{BoxConstraints, Offset, PaintContext, RenderBox, Size};

    const CARET: Color = Color(0xffaa0000);
    const SELECTION: Color = Color(0xff00aa00);
    const TEXT: Color = Color(0xff0000aa);

    fn field(text: &str, base: usize, extent: usize) -> RenderEditable {
        let mut value = TextEditingValue::new(text);
        value.selection_base = base as i32;
        value.selection_extent = extent as i32;
        let mut style = TextStyle::default();
        style.color = TEXT;
        RenderEditable::new(value)
            .with_style(style)
            .with_caret(CARET, true)
            .with_selection_color(SELECTION)
    }

    /// The same field allowed to wrap. `MaxLines::Single` is the default and
    /// turns wrapping off, so a test about wrapping has to ask for it.
    fn wrapping(text: &str, base: usize, extent: usize) -> RenderEditable {
        field(text, base, extent).with_max_lines(MaxLines::Growing)
    }

    fn painted(mut field: RenderEditable, width: f32) -> Vec<Drawn> {
        field.layout(BoxConstraints::loose(width, 400.0));
        let mut layers = LayerTree::new(600, 600);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(600.0, 600.0));
            field.paint(&mut context, Offset::ZERO);
        }
        drawn()
    }

    fn rects(calls: &[Drawn]) -> Vec<(f32, f32, f32, f32, u32)> {
        calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Rect {
                    left,
                    top,
                    right,
                    bottom,
                    argb,
                    ..
                } => Some((*left, *top, *right, *bottom, *argb)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_collapsed_selection_draws_a_caret_and_no_highlight() {
        let calls = painted(field("hello", 3, 3), 300.0);
        let marks = rects(&calls);
        assert_eq!(marks.len(), 1, "{calls:?}");
        assert_eq!(marks[0].4, CARET.0);
    }

    #[test]
    fn and_a_selected_run_draws_a_highlight_and_no_caret() {
        // Upstream paints a caret only for a collapsed selection, and the
        // reason is in the comment beside it: a caret at the extent of a
        // highlighted run reads as a second, contradictory insertion point.
        let calls = painted(field("hello", 1, 4), 300.0);
        let marks = rects(&calls);
        assert_eq!(marks.len(), 1, "{calls:?}");
        assert_eq!(marks[0].4, SELECTION.0, "the highlight, and only it");
        assert!(
            !marks.iter().any(|mark| mark.4 == CARET.0),
            "no caret while a run is selected"
        );
    }

    #[test]
    fn the_highlight_goes_down_before_the_glyphs_it_highlights() {
        // The rule beside the code, and until the stub started recording
        // paragraphs this test could not ask it: there was no glyph in the
        // list to be before, so it asserted the weaker "nothing else gets in
        // first" and said so.
        //
        // The rule matters because a filled rectangle drawn *after* the text
        // covers the text it is meant to be highlighting. A selection that
        // blanks out the words inside it is the failure this prevents, and it
        // is one line's worth of reordering away.
        let calls = painted(field("hello", 1, 4), 300.0);
        let highlight = calls
            .iter()
            .position(|call| matches!(call, Drawn::Rect { argb, .. } if *argb == SELECTION.0))
            .expect("the highlight");
        let glyphs = calls
            .iter()
            .position(|call| matches!(call, Drawn::Paragraph { .. }))
            .expect("the text");
        assert!(
            highlight < glyphs,
            "the highlight is under the words, not over them: {calls:?}"
        );
        assert_eq!(highlight, 0, "and nothing else gets in first");
    }

    #[test]
    fn an_empty_field_showing_a_hint_still_draws_its_caret() {
        // This test was named for a hint and never set one -- `field` does not
        // call `with_placeholder`, so what it built was an empty field with
        // nothing to show, and while paragraphs went unrecorded there was
        // nothing that could tell. It sets one now.
        //
        // Both halves are worth pinning. A field with nothing in it still says
        // where typing will go, and the caret is drawn *over* the placeholder
        // rather than under it: a caret hidden behind grey hint text is a
        // field that looks unfocused while it has the keyboard.
        let mut hint_style = TextStyle::default();
        hint_style.color = TEXT;
        let calls = painted(
            field("", 0, 0).with_placeholder("Search", hint_style),
            300.0,
        );
        assert_eq!(rects(&calls).len(), 1, "{calls:?}");
        assert_eq!(rects(&calls)[0].4, CARET.0);

        let caret = calls
            .iter()
            .position(|call| matches!(call, Drawn::Rect { argb, .. } if *argb == CARET.0))
            .expect("the caret");
        let hint = calls
            .iter()
            .position(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "Search"))
            .expect("the placeholder");
        assert!(caret > hint, "the caret sits over the hint: {calls:?}");
    }

    #[test]
    fn a_run_across_a_wrap_is_one_rectangle_per_line() {
        // Upstream's getBoxesForSelection hands back a box per line, and this
        // is the same shape: a selection spanning a wrap cannot be one
        // rectangle, because the space between the lines is not selected.
        let wide = painted(wrapping("hello world again", 0, 17), 4000.0);
        let narrow = painted(wrapping("hello world again", 0, 17), 60.0);
        let highlights = |calls: &[Drawn]| {
            rects(calls)
                .into_iter()
                .filter(|mark| mark.4 == SELECTION.0)
                .count()
        };
        assert_eq!(highlights(&wide), 1, "one line, one rectangle");
        assert!(
            highlights(&narrow) > 1,
            "wrapped, so more than one: {narrow:?}"
        );
    }

    #[test]
    fn every_highlight_rectangle_sits_on_its_own_line() {
        // Two rectangles at the same height would be one rectangle; the point
        // of splitting is that they are not.
        let calls = painted(wrapping("hello world again", 0, 17), 60.0);
        let tops: Vec<f32> = rects(&calls)
            .into_iter()
            .filter(|mark| mark.4 == SELECTION.0)
            .map(|mark| mark.1)
            .collect();
        assert!(tops.len() > 1, "nothing to compare: {tops:?}");
        for pair in tops.windows(2) {
            assert!(pair[1] > pair[0], "each below the last: {tops:?}");
        }
    }

    #[test]
    fn a_narrower_box_wraps_more() {
        // The relation rather than a number: the model gives every glyph the
        // same width, so any particular count would be about the model.
        let counts: Vec<usize> = [400.0, 120.0, 60.0]
            .into_iter()
            .map(|width| {
                rects(&painted(wrapping("hello world again", 0, 17), width))
                    .into_iter()
                    .filter(|mark| mark.4 == SELECTION.0)
                    .count()
            })
            .collect();
        for pair in counts.windows(2) {
            assert!(pair[1] >= pair[0], "{counts:?}");
        }
        assert!(counts[2] > counts[0], "{counts:?}");
    }

    #[test]
    fn a_field_told_not_to_show_a_caret_shows_none() {
        // Both halves, because the first on its own is satisfied by a field
        // that drew nothing whatever -- one that failed to lay out, or a
        // helper that stopped recording. The second says the path ran.
        let hidden = field("hello", 3, 3).with_caret(CARET, false);
        assert!(rects(&painted(hidden, 300.0)).is_empty());

        let shown = field("hello", 3, 3).with_caret(CARET, true);
        let marks = rects(&painted(shown, 300.0));
        assert_eq!(marks.len(), 1, "the same field with the caret on");
        assert_eq!(marks[0].4, CARET.0);
    }

    #[test]
    fn a_single_line_field_wraps_nothing_however_narrow_it_is() {
        // MaxLines::One is upstream's `maxLines: 1`, which turns wrapping off
        // rather than clipping it -- so the highlight stays one rectangle.
        let narrow = field("hello world again", 0, 17);
        // Single is the default, so this is the ordinary case.
        let highlights = rects(&painted(narrow, 60.0))
            .into_iter()
            .filter(|mark| mark.4 == SELECTION.0)
            .count();
        assert_eq!(highlights, 1);
    }
}
