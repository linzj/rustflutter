//! What the framework says when something goes wrong -- a port of the error
//! half of upstream's `foundation/assertions.dart`, on top of the diagnostics
//! tree.
//!
//! The four error diagnostics -- description, summary, hint, spacer -- exist
//! so that an error message is a **structured** thing rather than a string.
//! That is what lets one error be shown in full in a console, one line deep in
//! an IDE tooltip, and as a card in the inspector, all from the same object.
//!
//! The level is what does the separating: exactly one part of an error is a
//! [`ErrorSummary`], and the summary is what a one-line rendering shows.

use crate::diagnostics::{
    DiagnosticLevel, DiagnosticPropertiesBuilder, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsTreeStyle, PropertyValue, TextTreeNode,
};
use crate::stack_frame::StackFrame;

/// Upstream's `_ErrorDiagnostic`: a line of an error message.
///
/// The three named kinds below differ **only in their level**, and everything
/// else about them is identical: no name, no separator, flat style. Upstream
/// writes them as three one-line subclasses for that reason.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorDiagnostic {
    pub base: DiagnosticsProperty,
    /// Upstream stores the message as a `List<Object>` so that an
    /// interpolated string can keep the objects it interpolated -- the
    /// inspector can then make each one clickable. Joined here, with the
    /// parts kept so the structure is not lost.
    pub parts: Vec<String>,
}

impl ErrorDiagnostic {
    fn new(parts: Vec<String>, level: DiagnosticLevel) -> ErrorDiagnostic {
        let joined = parts.concat();
        let mut base = DiagnosticsProperty::new::<&str>(None, PropertyValue::Text(joined.clone()));
        base.show_name = false;
        base.show_separator = false;
        base.style = DiagnosticsTreeStyle::Flat;
        base.default_level = level;
        base.description = Some(joined);
        ErrorDiagnostic { base, parts }
    }

    /// Upstream's `valueToString`, which is the parts joined with nothing
    /// between them -- they are the pieces of one sentence, not a list.
    pub fn value_to_string(&self) -> String {
        self.parts.concat()
    }
}

impl DiagnosticsNode for ErrorDiagnostic {
    fn name(&self) -> Option<&str> {
        None
    }

    fn to_description(&self) -> String {
        self.value_to_string()
    }

    fn level(&self) -> DiagnosticLevel {
        self.base.default_level
    }

    fn show_name(&self) -> bool {
        false
    }

    fn show_separator(&self) -> bool {
        false
    }

    fn style(&self) -> DiagnosticsTreeStyle {
        DiagnosticsTreeStyle::Flat
    }
}

/// Upstream `ErrorDescription`: the body of an error, at `Info`.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorDescription(pub ErrorDiagnostic);

impl ErrorDescription {
    pub fn new(message: impl Into<String>) -> ErrorDescription {
        ErrorDescription(ErrorDiagnostic::new(
            vec![message.into()],
            DiagnosticLevel::Info,
        ))
    }

    /// Upstream's `_fromParts`, which keeps an interpolated message's pieces.
    pub fn from_parts(parts: Vec<String>) -> ErrorDescription {
        ErrorDescription(ErrorDiagnostic::new(parts, DiagnosticLevel::Info))
    }
}

/// Upstream `ErrorSummary`: the one line that says what went wrong.
///
/// **Exactly one part of an error should be a summary**, and it is what a
/// one-line rendering shows. An error with two would have two answers to
/// "what happened"; one with none falls back to the exception's own first
/// line, which is what [`FlutterErrorDetails::summary`] does.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorSummary(pub ErrorDiagnostic);

impl ErrorSummary {
    pub fn new(message: impl Into<String>) -> ErrorSummary {
        ErrorSummary(ErrorDiagnostic::new(
            vec![message.into()],
            DiagnosticLevel::Summary,
        ))
    }

    pub fn from_parts(parts: Vec<String>) -> ErrorSummary {
        ErrorSummary(ErrorDiagnostic::new(parts, DiagnosticLevel::Summary))
    }
}

/// Upstream `ErrorHint`: advice, at `Hint`.
///
/// Above `Info` and below `Summary`, which is exactly its standing: a hint
/// matters more than the narration and less than what actually went wrong.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorHint(pub ErrorDiagnostic);

impl ErrorHint {
    pub fn new(message: impl Into<String>) -> ErrorHint {
        ErrorHint(ErrorDiagnostic::new(
            vec![message.into()],
            DiagnosticLevel::Hint,
        ))
    }

    pub fn from_parts(parts: Vec<String>) -> ErrorHint {
        ErrorHint(ErrorDiagnostic::new(parts, DiagnosticLevel::Hint))
    }
}

/// Upstream `ErrorSpacer`: a blank line.
///
/// It is a real property rather than a `\n` in the text because the parts of
/// an error are rendered separately -- a newline inside a description would
/// be indented and prefixed along with the text around it, and would not read
/// as a gap at all.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorSpacer(pub DiagnosticsProperty);

impl Default for ErrorSpacer {
    fn default() -> ErrorSpacer {
        ErrorSpacer::new()
    }
}

impl ErrorSpacer {
    pub fn new() -> ErrorSpacer {
        let mut property = DiagnosticsProperty::new(Some(""), PropertyValue::Null);
        property.description = Some(String::new());
        property.show_name = false;
        ErrorSpacer(property)
    }
}

/// What kind of thing was thrown, for the sentence upstream builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionKind {
    /// Upstream's `AssertionError()` arm.
    Assertion,
    /// A thrown `String`, which upstream calls a *message* rather than an
    /// error -- because somebody threw prose, and calling it an error would
    /// dress it up as more than it is.
    Message,
    /// An `Error` or `Exception`, named by its type.
    Error,
    /// Anything else, which upstream suffixes with `object` -- a thrown
    /// `Duration` is a "Duration object", because "a Duration was thrown"
    /// would read as though `Duration` were an error type.
    Object,
    /// A number, which gets a sentence of its own.
    Number,
}

/// Upstream `FlutterErrorDetails`: an error and everything known about it.
#[derive(Clone, Debug, PartialEq)]
pub struct FlutterErrorDetails {
    pub exception: String,
    pub exception_kind: ExceptionKind,
    pub stack: Vec<StackFrame>,
    /// Upstream's `library`, defaulting to `"Flutter framework"` -- so an
    /// error reported by a package says which package.
    pub library: Option<String>,
    /// Upstream's `context`: what the framework was doing. It reads as a verb
    /// phrase -- "while laying out" -- because it is dropped into a sentence.
    pub context: Option<String>,
    /// Upstream's `silent`, which suppresses the report **in debug builds
    /// only**.
    pub silent: bool,
    /// Upstream's `informationCollector`, as the lines it would have
    /// collected.
    pub information: Vec<String>,
}

impl FlutterErrorDetails {
    pub fn new(exception: impl Into<String>, kind: ExceptionKind) -> FlutterErrorDetails {
        FlutterErrorDetails {
            exception: exception.into(),
            exception_kind: kind,
            stack: Vec::new(),
            library: Some("Flutter framework".to_string()),
            context: None,
            silent: false,
            information: Vec::new(),
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn with_library(mut self, library: impl Into<String>) -> Self {
        self.library = Some(library.into());
        self
    }

    pub fn with_stack(mut self, stack: Vec<StackFrame>) -> Self {
        self.stack = stack;
        self
    }

    pub fn with_silent(mut self, silent: bool) -> Self {
        self.silent = silent;
        self
    }

    pub fn with_information(mut self, information: Vec<String>) -> Self {
        self.information = information;
        self
    }

    /// Upstream's `copyWith`, whose every argument is optional and keeps what
    /// was there.
    pub fn copy_with(
        &self,
        exception: Option<String>,
        context: Option<String>,
        library: Option<String>,
        silent: Option<bool>,
    ) -> FlutterErrorDetails {
        FlutterErrorDetails {
            exception: exception.unwrap_or_else(|| self.exception.clone()),
            context: context.or_else(|| self.context.clone()),
            library: library.or_else(|| self.library.clone()),
            silent: silent.unwrap_or(self.silent),
            ..self.clone()
        }
    }

    /// Upstream's `exceptionAsString`, reduced to the part that is not about
    /// Dart's own `AssertionError` formatting.
    pub fn exception_as_string(&self) -> String {
        self.exception.clone()
    }

    /// Upstream's `summary`.
    ///
    /// The first *summary-level* part if the error has one, and otherwise the
    /// **first line** of the exception, left-trimmed. Falling back to the
    /// first line rather than the whole thing is the decision: a summary that
    /// ran to a paragraph would not be a summary.
    pub fn summary(&self, properties: &DiagnosticPropertiesBuilder) -> ErrorSummary {
        let found = properties
            .properties()
            .iter()
            .find(|property| property.level() == DiagnosticLevel::Summary);
        match found {
            Some(property) => ErrorSummary::new(property.to_description()),
            None => ErrorSummary::new(self.format_exception()),
        }
    }

    fn format_exception(&self) -> String {
        self.exception_as_string()
            .lines()
            .next()
            .unwrap_or("")
            .trim_start()
            .to_string()
    }

    /// Upstream's `debugFillProperties`, and its sentence-building.
    ///
    /// The wording distinguishes four cases, and the distinctions are real:
    /// a thrown string is a *message* rather than an error, because somebody
    /// threw prose; anything that is not an `Error` or `Exception` is named
    /// "a `Foo` object", because "a Duration was thrown" would read as though
    /// `Duration` were an error type; and a number gets a sentence of its own.
    pub fn describe(&self) -> Vec<String> {
        let verb = match &self.context {
            Some(context) => format!("thrown {context}"),
            None => "thrown".to_string(),
        };
        let mut lines = Vec::new();
        match self.exception_kind {
            ExceptionKind::Number => {
                lines.push(format!("The number {} was {verb}.", self.exception));
            }
            kind => {
                let name = match kind {
                    ExceptionKind::Assertion => "assertion".to_string(),
                    ExceptionKind::Message => "message".to_string(),
                    ExceptionKind::Error => self.exception_type_name(),
                    ExceptionKind::Object => format!("{} object", self.exception_type_name()),
                    ExceptionKind::Number => unreachable!(),
                };
                lines.push(format!("The following {name} was {verb}:"));
                lines.push(self.exception_as_string());
            }
        }
        lines.extend(self.information.iter().cloned());
        lines
    }

    fn exception_type_name(&self) -> String {
        self.exception
            .split(':')
            .next()
            .unwrap_or(&self.exception)
            .trim()
            .to_string()
    }
}

/// Upstream `DiagnosticsStackTrace`: a stack trace as a block of an error.
///
/// It is a [`DiagnosticsBlock`](crate::diagnostics::DiagnosticsBlock)
/// upstream, and the reason matters: a stack trace in an error is a *section*
/// with lines under it, not a property with a long value. A property would be
/// wrapped and indented as one paragraph and stop looking like a stack.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagnosticsStackTrace {
    pub name: String,
    pub frames: Vec<StackFrame>,
    /// Upstream's `showSeparator`, false when the trace stands alone.
    pub show_separator: bool,
}

impl DiagnosticsStackTrace {
    pub fn new(name: impl Into<String>, frames: Vec<StackFrame>) -> DiagnosticsStackTrace {
        DiagnosticsStackTrace {
            name: name.into(),
            frames,
            show_separator: true,
        }
    }

    pub fn with_show_separator(mut self, show: bool) -> Self {
        self.show_separator = show;
        self
    }

    /// The lines a renderer would show, one per frame, each as its original
    /// source text.
    pub fn children(&self) -> Vec<TextTreeNode> {
        self.frames
            .iter()
            .map(|frame| TextTreeNode::new(frame.source.clone()))
            .collect()
    }
}

impl DiagnosticsNode for DiagnosticsStackTrace {
    fn name(&self) -> Option<&str> {
        Some(&self.name)
    }

    fn to_description(&self) -> String {
        String::new()
    }

    fn level(&self) -> DiagnosticLevel {
        DiagnosticLevel::Info
    }

    fn show_separator(&self) -> bool {
        self.show_separator
    }

    fn style(&self) -> DiagnosticsTreeStyle {
        DiagnosticsTreeStyle::Whitespace
    }
}

/// Upstream `FlutterError`: the framework's own error type, and the desk the
/// reports land on.
#[derive(Debug, Default)]
pub struct FlutterError {
    /// Upstream's `_errorCount`, which is why the second and later errors of a
    /// run are printed shorter than the first.
    error_count: usize,
    /// Whether `onError` is set at all. Upstream's default is `presentError`;
    /// clearing it makes errors silent, which a test does deliberately.
    pub has_on_error: bool,
    reported: Vec<FlutterErrorDetails>,
}

impl FlutterError {
    /// Upstream's `wrapWidth`.
    pub const WRAP_WIDTH: usize = 100;

    pub fn new() -> FlutterError {
        FlutterError {
            error_count: 0,
            has_on_error: true,
            reported: Vec::new(),
        }
    }

    /// Upstream's `reportError`, which is `onError?.call(details)` and nothing
    /// else.
    ///
    /// **The whole point is that it is replaceable.** A test swaps `onError`
    /// to collect errors instead of printing them, and an application swaps it
    /// to send them somewhere; the framework calls this one function either
    /// way.
    pub fn report_error(&mut self, details: FlutterErrorDetails) -> bool {
        if !self.has_on_error {
            return false;
        }
        self.error_count += 1;
        self.reported.push(details);
        true
    }

    pub fn error_count(&self) -> usize {
        self.error_count
    }

    pub fn reported(&self) -> &[FlutterErrorDetails] {
        &self.reported
    }

    /// Upstream's `resetErrorCount`.
    ///
    /// Upstream's documentation is worth keeping: this exists so that a test
    /// framework can make each test's first error print in full, rather than
    /// having tests after the first silently produce the shorter form.
    pub fn reset_error_count(&mut self) {
        self.error_count = 0;
    }

    /// Whether this error would be printed in full: upstream prints the whole
    /// thing for the first error and a shorter form afterwards.
    pub fn prints_in_full(&self) -> bool {
        self.error_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_error_lines_differ_only_in_their_level() {
        // Everything else about them is identical, which is why upstream
        // writes them as three one-line subclasses.
        let description = ErrorDescription::new("the thing broke");
        let summary = ErrorSummary::new("the thing broke");
        let hint = ErrorHint::new("the thing broke");

        assert_eq!(description.0.level(), DiagnosticLevel::Info);
        assert_eq!(summary.0.level(), DiagnosticLevel::Summary);
        assert_eq!(hint.0.level(), DiagnosticLevel::Hint);

        for line in [&description.0, &summary.0, &hint.0] {
            assert_eq!(line.to_description(), "the thing broke");
            assert!(!line.show_name(), "no name");
            assert!(!line.show_separator(), "and no separator");
            assert_eq!(line.style(), DiagnosticsTreeStyle::Flat);
            assert_eq!(line.name(), None);
        }
    }

    #[test]
    fn a_hint_matters_more_than_the_narration_and_less_than_what_went_wrong() {
        // Which is exactly where upstream puts it in the order.
        assert!(DiagnosticLevel::Info < DiagnosticLevel::Hint);
        assert!(DiagnosticLevel::Hint < DiagnosticLevel::Summary);
    }

    #[test]
    fn an_interpolated_message_keeps_its_pieces() {
        // Upstream stores them as a list so the inspector can make each
        // interpolated object clickable; joined for display with nothing
        // between them, because they are one sentence rather than a list.
        let parts = vec![
            "Element ".to_string(),
            "Text".to_string(),
            " must be ".to_string(),
            "red".to_string(),
        ];
        let described = ErrorDescription::from_parts(parts.clone());
        assert_eq!(described.0.parts, parts);
        assert_eq!(described.0.value_to_string(), "Element Text must be red");
        assert_eq!(
            ErrorSummary::from_parts(parts.clone()).0.level(),
            DiagnosticLevel::Summary
        );
        assert_eq!(
            ErrorHint::from_parts(parts).0.level(),
            DiagnosticLevel::Hint
        );
    }

    #[test]
    fn a_spacer_is_a_property_rather_than_a_newline_in_the_text() {
        // A newline inside a description would be indented and prefixed along
        // with the text around it, and would not read as a gap at all.
        let spacer = ErrorSpacer::new();
        assert_eq!(spacer.0.to_description(), "");
        assert!(!spacer.0.show_name);
        assert_eq!(spacer.0.name.as_deref(), Some(""));
    }

    #[test]
    fn the_summary_is_the_summary_level_part_when_there_is_one() {
        let details = FlutterErrorDetails::new("Boom: it broke", ExceptionKind::Error);
        let mut builder = DiagnosticPropertiesBuilder::new();
        builder.add(ErrorDescription::new("some narration").0.base.clone());
        builder.add(ErrorSummary::new("the button was null").0.base.clone());

        assert_eq!(
            details.summary(&builder).0.to_description(),
            "the button was null"
        );
    }

    #[test]
    fn otherwise_it_is_the_first_line_of_the_exception_and_only_the_first() {
        // A summary that ran to a paragraph would not be a summary.
        let details = FlutterErrorDetails::new(
            "  Boom: it broke\nand here is why\nat great length",
            ExceptionKind::Error,
        );
        let builder = DiagnosticPropertiesBuilder::new();
        assert_eq!(
            details.summary(&builder).0.to_description(),
            "Boom: it broke",
            "left-trimmed, and the rest dropped"
        );
    }

    #[test]
    fn a_thrown_string_is_called_a_message_rather_than_an_error() {
        // Somebody threw prose, and calling it an error would dress it up as
        // more than it is.
        let details = FlutterErrorDetails::new("something is off", ExceptionKind::Message);
        assert_eq!(details.describe()[0], "The following message was thrown:");
    }

    #[test]
    fn something_that_is_not_an_error_type_is_called_an_object() {
        // "A Duration was thrown" would read as though Duration were an error
        // type.
        let thrown = FlutterErrorDetails::new("Duration", ExceptionKind::Object);
        assert_eq!(
            thrown.describe()[0],
            "The following Duration object was thrown:"
        );

        let real_error = FlutterErrorDetails::new("StateError: bad state", ExceptionKind::Error);
        assert_eq!(
            real_error.describe()[0],
            "The following StateError was thrown:",
            "no `object` suffix for a real error type"
        );
    }

    #[test]
    fn an_assertion_and_a_number_each_get_their_own_wording() {
        let assertion = FlutterErrorDetails::new("assert failed", ExceptionKind::Assertion);
        assert_eq!(
            assertion.describe()[0],
            "The following assertion was thrown:"
        );

        let number = FlutterErrorDetails::new("42", ExceptionKind::Number);
        assert_eq!(
            number.describe(),
            vec!["The number 42 was thrown."],
            "a sentence of its own, and no second line"
        );
    }

    #[test]
    fn the_context_reads_as_a_verb_phrase_dropped_into_the_sentence() {
        let details = FlutterErrorDetails::new("StateError: bad", ExceptionKind::Error)
            .with_context("while laying out the widget tree");
        assert_eq!(
            details.describe()[0],
            "The following StateError was thrown while laying out the widget tree:"
        );
    }

    #[test]
    fn the_collected_information_comes_after_the_exception() {
        let details =
            FlutterErrorDetails::new("StateError: bad", ExceptionKind::Error).with_information(
                vec!["The widget was:".to_string(), "  Text(\"hi\")".to_string()],
            );
        let lines = details.describe();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[1], "StateError: bad");
        assert_eq!(lines[2], "The widget was:");
    }

    #[test]
    fn an_error_says_which_library_reported_it() {
        // So an error from a package is not mistaken for one from the
        // framework.
        let framework = FlutterErrorDetails::new("boom", ExceptionKind::Error);
        assert_eq!(framework.library.as_deref(), Some("Flutter framework"));
        assert_eq!(
            FlutterErrorDetails::new("boom", ExceptionKind::Error)
                .with_library("my_package")
                .library
                .as_deref(),
            Some("my_package")
        );
    }

    #[test]
    fn copying_keeps_everything_it_was_not_given() {
        let original = FlutterErrorDetails::new("boom", ExceptionKind::Error)
            .with_context("while building")
            .with_library("my_package")
            .with_silent(true)
            .with_information(vec!["extra".to_string()]);

        let same = original.copy_with(None, None, None, None);
        assert_eq!(same, original);

        let louder = original.copy_with(None, None, None, Some(false));
        assert!(!louder.silent);
        assert_eq!(louder.context, original.context);
        assert_eq!(louder.information, original.information);
    }

    #[test]
    fn a_stack_trace_is_a_section_with_lines_under_it_and_not_one_long_value() {
        // A property would be wrapped and indented as one paragraph and stop
        // looking like a stack.
        let frames = StackFrame::from_stack_string(
            "#0      main (package:app/main.dart:1:2)\n\
             #1      Foo.bar (package:app/foo.dart:3:4)",
        );
        let trace = DiagnosticsStackTrace::new("When the exception was thrown", frames);
        assert_eq!(trace.name(), Some("When the exception was thrown"));
        assert_eq!(trace.style(), DiagnosticsTreeStyle::Whitespace);

        let children = trace.children();
        assert_eq!(children.len(), 2, "one line per frame");
        assert_eq!(
            children[0].description,
            "#0      main (package:app/main.dart:1:2)"
        );
        assert!(trace.show_separator());
        assert!(
            !DiagnosticsStackTrace::new("x", Vec::new())
                .with_show_separator(false)
                .show_separator()
        );
    }

    #[test]
    fn reporting_an_error_goes_through_one_replaceable_function() {
        // A test swaps onError to collect errors instead of printing them, and
        // an application swaps it to send them somewhere; the framework calls
        // the same function either way.
        let mut errors = FlutterError::new();
        assert!(errors.report_error(FlutterErrorDetails::new("one", ExceptionKind::Error)));
        assert!(errors.report_error(FlutterErrorDetails::new("two", ExceptionKind::Error)));
        assert_eq!(errors.reported().len(), 2);
        assert_eq!(errors.error_count(), 2);

        errors.has_on_error = false;
        assert!(
            !errors.report_error(FlutterErrorDetails::new("three", ExceptionKind::Error)),
            "with no handler the error goes nowhere"
        );
        assert_eq!(errors.reported().len(), 2);
    }

    #[test]
    fn the_first_error_of_a_run_is_printed_in_full_and_later_ones_are_not() {
        // And resetting the count is what lets a test framework make each
        // test's first error print in full.
        let mut errors = FlutterError::new();
        assert!(errors.prints_in_full());
        errors.report_error(FlutterErrorDetails::new("one", ExceptionKind::Error));
        assert!(!errors.prints_in_full());

        errors.reset_error_count();
        assert!(errors.prints_in_full());
        assert_eq!(errors.error_count(), 0);
        assert_eq!(
            errors.reported().len(),
            1,
            "the count is reset, not the record"
        );
    }

    #[test]
    fn the_wrap_width_is_upstreams() {
        assert_eq!(FlutterError::WRAP_WIDTH, 100);
    }
}
