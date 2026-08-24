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

        // Where the IME should put its candidate list. Reported from here
        // because this is the first point at which the field's position in the
        // window is known -- layout gives a size, not a place. The caret is
        // reported where it was drawn, scroll included, because the candidate
        // list belongs under what the reader can see.
        if let Some(report) = &self.report {
            let caret =
                caret.unwrap_or_else(|| Rect::ltrb(0.0, 0.0, CARET_WIDTH, self.size.height));
            let on_screen = Rect::ltrb(
                caret.left - scroll.dx,
                caret.top - scroll.dy,
                caret.right - scroll.dx,
                caret.bottom - scroll.dy,
            );
            let stamp = (
                (offset.dx.round()) as i32,
                (offset.dy.round()) as i32,
                (on_screen.left.round()) as i32,
                (on_screen.top.round()) as i32,
            );
            if self.reported.get() != Some(stamp) {
                self.reported.set(Some(stamp));
                report(offset, self.size, on_screen);
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
        if let Some(connection) = &self.connection {
            if connection.is_attached() {
                connection.set_editing_state(&self.value);
            }
        }
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
pub struct TextField {
    id: u64,
    placeholder: Option<String>,
    style: Option<TextStyle>,
    input_type: TextInputType,
    action: TextInputAction,
    obscure: bool,
    max_lines: MaxLines,
    on_changed: Option<TextCallback>,
    on_submitted: Option<TextCallback>,
    /// Somewhere to publish this field's [`StateHandle`], so a widget composed
    /// around the field -- a search field's clear button -- can reach the
    /// field's text. Upstream's equivalent is handing both the field and the
    /// button the same `TextEditingController`.
    state_sink: Option<Rc<RefCell<Option<StateHandle<TextFieldState>>>>>,
}

impl TextField {
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
            on_changed: None,
            on_submitted: None,
            state_sink: None,
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
        let field_handle = handle;
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
        let max_lines = self.max_lines;
        let on_focus_change = move |has_focus: bool| {
            if !has_focus {
                focus_handle.set_state(|state| {
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

            // The tap handler, made fresh on every build because the region
            // consumes it. A tap does what upstream's `handleTap` ->
            // `selectPosition` does: the caret goes to the position under the
            // finger, and the field takes the keyboard.
            let tap_sink = lines_sink.clone();
            let tap_state = tap_handle.clone();
            let tapped_shown = shown.text.clone();
            let tapped_real = real_text.clone();
            let on_tap = move |tap: TapEvent| {
                let Some(layout) = tap_sink.borrow().clone() else {
                    return;
                };
                // The pointer's place in the field is its place in the
                // content once the scroll is added back: paint drew the
                // content `scroll` up and to the left of the field.
                let at = Offset::new(
                    tap.local_position.dx + layout.scroll.dx,
                    tap.local_position.dy + layout.scroll.dy,
                );
                let measure = |run: &str| {
                    // The field's own measurement, so the position under the
                    // finger is the position on screen.
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
                    &tapped_shown,
                    &layout.lines,
                    layout.line_height,
                    at,
                    &measure,
                );
                // The lines are ranges of the text as drawn -- bullets, for an
                // obscured field -- while the platform counts UTF-16 units of
                // the text as typed. The two have a character for each of the
                // other's characters, so the character index crosses and the
                // units are counted on the real text.
                let character = tapped_shown[..byte].chars().count();
                let position: i32 = tapped_real
                    .chars()
                    .take(character)
                    .map(|c| c.len_utf16() as i32)
                    .sum();
                // The selection first and the focus second: a field that was
                // not being edited opens its session from the state as it now
                // stands, so the caret is where the reader tapped from the
                // session's very first frame.
                tap_state.set_state(move |state| {
                    state.value.selection_base = position;
                    state.value.selection_extent = position;
                    if let Some(connection) = &state.connection {
                        if connection.is_attached() {
                            connection.set_editing_state(&state.value);
                        }
                    }
                });
                crate::focus::focus(id);
            };

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
                .with_lines_sink(lines_sink.clone());
            RenderPointerRegion::new(id, field)
                .with_handlers(PointerHandlers::new().with_tap(on_tap))
        });

        // The field is a focus node, which is what makes Tab reach it and what
        // opens and closes its session. Upstream `TextField` wraps its
        // `EditableText` in a `Focus` for the same reason. The tap that
        // focuses is the editable's own -- placing the caret is half of what
        // a tap on a field means -- so this one carries no pointer handler of
        // its own and the two never compete for the gesture.
        let focused = crate::framework::component(
            crate::focus::Focus::new(id, editable)
                .with_focus_on_tap(false)
                .with_on_focus_change(on_focus_change),
        );

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
        properties.flags.is_focused = crate::focus::has_focus(id);
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

        // The stubbed engine measures nothing, so the caret's x is zero, but
        // which line it is on is the wrapping's decision, not the metrics':
        // the second, one line height down.
        let caret = field
            .caret_rect(&lines, 10.0)
            .expect("the caret is at a boundary");
        assert_eq!(caret.top, 10.0);
        assert_eq!(caret.height(), 10.0);
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
        // "aaa bb" selected whole: a rect on the wrapped first line and one on
        // the second. The widths are the stub's zeros; which lines get a rect
        // is the decision under test.
        assert!(field.line_extent(lines[0], 0..6).is_some());
        assert!(field.line_extent(lines[1], 0..6).is_some());
        // A run entirely before this line gives it nothing to draw.
        assert!(field.line_extent(lines[1], 0..1).is_none());
    }

    #[test]
    fn a_multiline_field_paints_without_a_real_text_stack() {
        // The stub engine makes every metric zero, so nothing wraps and
        // nothing scrolls; what this guards is that the multi-line paths --
        // per-line text, per-line selection, composing, caret, report -- run
        // at all against the same canvas the single-line ones do.
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
    //! The glyphs themselves are a paragraph, which the recorder does not read
    //! back. So what is pinned is where the rectangles are and in what order,
    //! not what the text looks like.
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
    fn the_highlight_is_the_first_mark_of_the_frame() {
        // As far as this can see. The rule beside the code is that the
        // highlight goes down *before the glyphs*, because a filled rectangle
        // drawn after them would cover the text it is meant to be
        // highlighting -- and that half cannot be checked here, because
        // paragraphs are not recorded and there is no glyph in the list to be
        // before.
        //
        // What is checked is that nothing else gets in first, which is the
        // part that would break if the block moved below the caret or the
        // composing underline.
        let calls = painted(field("hello", 1, 4), 300.0);
        let highlight = calls
            .iter()
            .position(|call| matches!(call, Drawn::Rect { argb, .. } if *argb == SELECTION.0))
            .expect("the highlight");
        assert_eq!(highlight, 0, "the first thing drawn: {calls:?}");
    }

    #[test]
    fn an_empty_field_showing_a_hint_still_draws_its_caret() {
        // The placeholder is a paragraph and the caret is a rectangle, so the
        // caret is the only one of the two this can see -- which is the case
        // worth pinning anyway: a field with nothing in it still says where
        // typing will go.
        let calls = painted(field("", 0, 0), 300.0);
        assert_eq!(rects(&calls).len(), 1, "{calls:?}");
        assert_eq!(rects(&calls)[0].4, CARET.0);
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
        let hidden = field("hello", 3, 3).with_caret(CARET, false);
        assert!(rects(&painted(hidden, 300.0)).is_empty());
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
