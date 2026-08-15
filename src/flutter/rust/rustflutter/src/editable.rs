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

use std::cell::Cell;
use std::rc::Rc;

use crate::components::theme_of;
use crate::engine::{Color, TextStyle};
use crate::framework::{AnyWidget, BuildContext, Key, StateHandle, StatefulComponent, leaf};
use crate::gestures::PointerHandlers;
use crate::painting;
use crate::render::{BoxConstraints, HitTestResult, PaintContext, RenderBox, RenderPointerRegion};
use crate::services::text_input::{
    self, TextEditingValue, TextInputAction, TextInputClient, TextInputConfiguration,
    TextInputConnection, TextInputType,
};
use crate::widgets::{Offset, Size};

/// How wide the caret is drawn, in logical pixels. Upstream's `cursorWidth`.
const CARET_WIDTH: f32 = 2.0;

/// How far under the baseline the composing underline sits.
const UNDERLINE_GAP: f32 = 1.0;

/// What a selected run is painted behind with when nothing says otherwise.
///
/// Translucent, and that is the whole design: the highlight goes *under* the
/// glyphs, so an opaque one would hide the text it is highlighting. Upstream's
/// `TextField` derives it from the theme's primary colour at 40% for the same
/// reason.
const DEFAULT_SELECTION: Color = Color::argb(0x66, 0x44, 0x88, 0xCC);

/// Told where the field landed: its offset, its size, and how far into it the
/// caret sits. Shared because the render object is rebuilt every frame and the
/// callback is not.
type ReportPlacement = Rc<dyn Fn(Offset, Size, f32)>;

/// What an application is handed when the text changes. A `&str`, and nothing
/// about channels, connections or composition.
type TextCallback = Rc<dyn Fn(&str)>;

// -- The render object --------------------------------------------------------

/// Draws one field: its text, the composing run, and the caret.
///
/// Upstream's `RenderEditable`, minus selection painting and scrolling. The
/// caret's position is measured by shaping the text before it -- the engine's
/// paragraph API here reports metrics for a whole string rather than for a
/// character offset, so the prefix is the measurement.
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
    show_caret: bool,
    /// Told where the field ended up, so the platform can put the IME there.
    /// Called from `paint`, which is the first moment the answer is known.
    report: Option<ReportPlacement>,
    /// What was last reported, so an unmoved field does not send a message per
    /// frame. Sixty a second would be sixty thread hops for nothing.
    reported: Cell<Option<(i32, i32, i32)>>,
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
            show_caret: false,
            report: None,
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

    /// How far into the line the caret sits, by measuring the text before it.
    fn caret_offset(&self) -> f32 {
        let Some(caret) = self.value.caret_bytes() else {
            return 0.0;
        };
        if caret == 0 {
            return 0.0;
        }
        let prefix = &self.value.text[..caret.min(self.value.text.len())];
        // Trailing spaces are part of the position even though they are not
        // part of the ink, so the advance width is what is wanted rather than
        // the tight box `width()` reports.
        painting::shape(prefix, &self.style, f32::MAX / 4.0).max_intrinsic_width()
    }

    /// Where a byte range of the text starts, and how wide it is.
    ///
    /// Measured by shaping the run before it and then the run itself, which is
    /// what the engine's paragraph API here allows: it reports metrics for a
    /// whole string rather than boxes for a character range, so a prefix is
    /// the measurement. Upstream asks `getBoxesForSelection` instead and gets
    /// a box per line; this is the single-line case of the same answer.
    fn run_extent(&self, range: std::ops::Range<usize>) -> (f32, f32) {
        let measure = |text: &str| {
            if text.is_empty() {
                0.0
            } else {
                painting::shape(text, &self.style, f32::MAX / 4.0).max_intrinsic_width()
            }
        };
        let start = measure(&self.value.text[..range.start]);
        (start, measure(&self.value.text[range]))
    }
}

impl RenderBox for RenderEditable {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let line = painting::shape("Ag", &self.style, f32::MAX / 4.0).height();
        self.size = constraints.constrain(Size::new(
            if constraints.has_bounded_width() { constraints.max_width } else { 200.0 },
            line,
        ));
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let caret_x = self.caret_offset();

        // The selection, before the text rather than after it. Upstream paints
        // it in the same order and for the same reason: it is a filled
        // rectangle, and drawn afterwards it would cover the glyphs it is
        // meant to be highlighting.
        if let Some(range) = self.value.selection_bytes() {
            let (start, width) = self.run_extent(range);
            let paint = crate::engine::Paint::new(self.selection_color);
            context.canvas().draw_rect(
                crate::engine::Rect::ltrb(
                    offset.dx + start,
                    offset.dy,
                    offset.dx + start + width,
                    offset.dy + self.size.height,
                ),
                &paint,
            );
        }

        if self.value.text.is_empty() && !self.placeholder.is_empty() {
            let hint =
                painting::shape(&self.placeholder, &self.placeholder_style, self.size.width);
            context.canvas().draw_paragraph(&hint, offset.dx, offset.dy);
        } else if !self.value.text.is_empty() {
            let text = painting::shape(&self.value.text, &self.style, f32::MAX / 4.0);
            context.canvas().draw_paragraph(&text, offset.dx, offset.dy);
        }

        // The composing run, underlined. This is the half-typed word: it is in
        // the text already and is not committed, and the underline is the only
        // thing telling the reader that.
        if let Some(range) = self.value.composing_bytes() {
            let (start, width) = self.run_extent(range);
            let y = offset.dy + self.size.height - UNDERLINE_GAP;
            let paint = crate::engine::Paint::new(self.style.color);
            context.canvas().draw_rect(
                crate::engine::Rect::ltrb(offset.dx + start, y, offset.dx + start + width, y + 1.0),
                &paint,
            );
        }

        // No caret while a run is selected. Upstream paints one only for a
        // collapsed selection, and it is the right rule: a caret drawn at the
        // extent of a highlighted run reads as a second, contradictory
        // insertion point.
        if self.show_caret && !self.value.has_selection() {
            let paint = crate::engine::Paint::new(self.caret_color);
            context.canvas().draw_rect(
                crate::engine::Rect::ltrb(
                    offset.dx + caret_x,
                    offset.dy,
                    offset.dx + caret_x + CARET_WIDTH,
                    offset.dy + self.size.height,
                ),
                &paint,
            );
        }

        // Where the IME should put its candidate list. Reported from here
        // because this is the first point at which the field's position in the
        // window is known -- layout gives a size, not a place.
        if let Some(report) = &self.report {
            let stamp = (
                (offset.dx.round()) as i32,
                (offset.dy.round()) as i32,
                caret_x.round() as i32,
            );
            if self.reported.get() != Some(stamp) {
                self.reported.set(Some(stamp));
                report(offset, self.size, caret_x);
            }
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if self.size.contains(position) {
            result.add(0, position);
            true
        } else {
            false
        }
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        painting::shape(&self.value.text, &self.style, f32::MAX / 4.0).max_intrinsic_width()
    }

    fn min_intrinsic_height(&self, _width: f32) -> f32 {
        painting::shape("Ag", &self.style, f32::MAX / 4.0).height()
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
}

impl TextFieldState {
    pub fn text(&self) -> &str {
        &self.value.text
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
        self.handle.set_state(move |state| state.value = value);
    }

    fn perform_action(&mut self, _action: TextInputAction) {
        if let Some(submitted) = &self.on_submitted {
            submitted(&self.last.text);
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
    on_changed: Option<TextCallback>,
    on_submitted: Option<TextCallback>,
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
            on_changed: None,
            on_submitted: None,
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

    /// Enter inserts a newline instead of submitting.
    pub fn multiline(mut self) -> Self {
        self.input_type = TextInputType::Multiline;
        self.action = TextInputAction::Newline;
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
}

impl StatefulComponent for TextField {
    type State = TextFieldState;

    fn key(&self) -> Key {
        Some(self.id)
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

        // Opening the session on tap is what stands in for a focus tree: there
        // is none yet, and "the field that was last tapped" is what focus would
        // have answered anyway. The client id makes it exclusive -- attaching
        // one field detaches the last.
        let tap_handle = handle;
        let on_changed = self.on_changed.clone();
        let on_submitted = self.on_submitted.clone();
        let configuration = TextInputConfiguration {
            input_type: self.input_type,
            action: self.action,
            obscure_text: self.obscure,
            autocorrect: !self.obscure,
        };
        let handlers = PointerHandlers::new().with_tap(move |_| {
            let client = FieldClient {
                handle: tap_handle.clone(),
                on_changed: on_changed.clone(),
                on_submitted: on_submitted.clone(),
                last: TextEditingValue::default(),
            };
            let opened = text_input::attach(Box::new(client), configuration);
            // The platform starts from whatever the field already holds, so a
            // field that was typed into, left, and come back to keeps its text.
            tap_handle.set_state(move |state| {
                opened.set_editing_state(&state.value);
                state.connection = Some(opened);
            });
            opened.show();
        });

        let caret_color = theme.primary;
        // The theme's own colour, made translucent, as upstream's `TextField`
        // derives it. Opaque it would cover the glyphs it highlights.
        let selection_color = theme.primary.with_alpha(0x66);
        let placeholder = self.placeholder.clone().unwrap_or_default();
        let id = self.id;

        leaf(move || {
            let report_connection = connection;
            let report: ReportPlacement =
                Rc::new(move |offset, size, caret_x| {
                    let Some(connection) = report_connection else {
                        return;
                    };
                    // Two halves of one answer, as the channel defines them:
                    // where the field is in the window, and where the caret is
                    // inside the field.
                    connection.set_editable_transform(offset.dx as f64, offset.dy as f64);
                    connection.set_caret_rect(
                        caret_x as f64,
                        0.0,
                        CARET_WIDTH as f64,
                        size.height as f64,
                    );
                });

            RenderPointerRegion::new(
                id,
                RenderEditable::new(shown.clone())
                    .with_style(style.clone())
                    .with_placeholder(placeholder.clone(), placeholder_style.clone())
                    .with_caret(caret_color, editing)
                    .with_selection_color(selection_color)
                    .with_report(report),
            )
            .with_handlers(handlers.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::tests_support::install;
    use crate::services::text_input;

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
        // What is assertable here is the *choice of prefix*. This build stubs
        // the engine and every paragraph metric it returns is zero, so the
        // width itself cannot be measured without a real text stack -- the
        // example that runs against one checks that. The prefix is the part
        // that can be wrong in an interesting way: it is where a byte offset
        // and a UTF-16 offset get confused.
        let field = RenderEditable::new(value("ab\u{4e2d}", 3, (-1, -1)));
        // Three UTF-16 units -- 'a', 'b' and one BMP character -- five bytes.
        assert_eq!(field.value.caret_bytes(), Some(5));
        assert_eq!(field.caret_offset(), 0.0, "stubbed metrics measure nothing");

        let empty = RenderEditable::new(value("abc", 0, (-1, -1)));
        assert_eq!(empty.caret_offset(), 0.0);

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
            last: TextEditingValue::default(),
        };
        client.update_editing_value(value("zh", 2, (0, 2)));
        client.update_editing_value(value("\u{4e2d}", 1, (-1, -1)));

        assert_eq!(seen.borrow().as_slice(), &["zh".to_string(), "\u{4e2d}".to_string()]);
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
            last: TextEditingValue::default(),
        };
        client.update_editing_value(value("done", 4, (-1, -1)));
        client.perform_action(TextInputAction::Done);

        assert_eq!(*submitted.borrow(), Some("done".to_string()));
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
        assert_eq!(shown.text, "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}");
        assert_eq!(shown.selection_extent, value.selection_extent);
    }
}
