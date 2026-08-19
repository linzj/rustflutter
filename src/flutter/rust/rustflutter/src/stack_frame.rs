//! Stack traces, parsed and filtered -- a port of upstream's
//! `foundation/stack_frame.dart` and the stack-filtering half of
//! `foundation/assertions.dart`.
//!
//! A stack trace as the runtime prints it is a wall of text, most of it about
//! the framework rather than about the caller's mistake. What is here turns
//! that text into frames and then throws away the frames nobody wants to read:
//! the async plumbing, the timer internals, and the long repetitive runs a
//! build error leaves behind.

/// Upstream `StackFrame`: one line of a stack trace, taken apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackFrame {
    /// The line as it was printed, kept so that a frame can always be shown
    /// again exactly as it arrived.
    pub source: String,
    /// The frame's position in the trace. `-1` for the two synthetic frames
    /// below, which have no position.
    pub number: i32,
    pub column: i32,
    pub line: i32,
    pub package_scheme: String,
    pub package: String,
    pub package_path: String,
    pub class_name: String,
    pub method: String,
    pub is_constructor: bool,
}

impl StackFrame {
    /// Upstream's `asynchronousSuspension`.
    ///
    /// Not a real frame: it is the marker the runtime prints where a stack
    /// crosses an `await`, and the frames above and below it are from
    /// different moments in time. Keeping it as a frame is what lets a filter
    /// see the gap rather than silently joining the two halves.
    pub fn asynchronous_suspension() -> StackFrame {
        StackFrame {
            source: "<asynchronous suspension>".to_string(),
            number: -1,
            column: -1,
            line: -1,
            method: "asynchronous suspension".to_string(),
            package_scheme: String::new(),
            package: String::new(),
            package_path: String::new(),
            class_name: String::new(),
            is_constructor: false,
        }
    }

    /// Upstream's `stackOverFlowElision`: the `...` the runtime prints in
    /// place of the thousands of frames a stack overflow produced.
    pub fn stack_overflow_elision() -> StackFrame {
        StackFrame {
            source: "...".to_string(),
            number: -1,
            column: -1,
            line: -1,
            method: "...".to_string(),
            package_scheme: String::new(),
            package: String::new(),
            package_path: String::new(),
            class_name: String::new(),
            is_constructor: false,
        }
    }

    /// Upstream's `fromStackString`.
    ///
    /// Blank lines are dropped, and a line that does not parse is dropped
    /// rather than failing the whole trace -- upstream's comment notes that on
    /// the web a non-debug build prints the exception message above the trace,
    /// and one unparseable line should not cost the reader every other frame.
    pub fn from_stack_string(stack: &str) -> Vec<StackFrame> {
        stack
            .trim()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(Self::from_stack_trace_line)
            .collect()
    }

    /// Upstream's `fromStackTraceLine`.
    ///
    /// The shape it parses is `#3      Foo.bar (package:baz/qux.dart:12:34)`,
    /// and the interesting work is in the middle group:
    ///
    /// * `.<anonymous closure>` is stripped, because a closure inside a method
    ///   is that method as far as a reader is concerned;
    /// * a method beginning `new` is a **constructor**, and what follows is
    ///   the class -- with a dot in it meaning a named constructor, so
    ///   `new Foo.bar` is the class `Foo` and the method `bar`;
    /// * otherwise a dot separates the class from the method.
    ///
    /// Line and column are optional and become `-1` when absent, which is the
    /// same "no position" the two synthetic frames use.
    pub fn from_stack_trace_line(line: &str) -> Option<StackFrame> {
        if line == "<asynchronous suspension>" {
            return Some(Self::asynchronous_suspension());
        }
        if line == "..." {
            return Some(Self::stack_overflow_elision());
        }
        if !line.starts_with('#') {
            return None;
        }

        // `#<number><spaces><member> (<uri>[:line[:column]])`
        let rest = line.strip_prefix('#')?;
        let digits_end = rest.find(|character: char| !character.is_ascii_digit())?;
        let number: i32 = rest[..digits_end].parse().ok()?;
        let rest = rest[digits_end..].trim_start();
        let open = rest.rfind(" (")?;
        let member = &rest[..open];
        let location = rest[open + 2..].strip_suffix(')')?;

        let (class_name, method, is_constructor) = Self::split_member(member);
        let (scheme, package, package_path, line_number, column) = Self::split_location(location);

        Some(StackFrame {
            source: line.to_string(),
            number,
            column,
            line: line_number,
            package_scheme: scheme,
            package,
            package_path,
            class_name,
            method,
            is_constructor,
        })
    }

    fn split_member(member: &str) -> (String, String, bool) {
        let member = member.replace(".<anonymous closure>", "");
        if let Some(after_new) = member.strip_prefix("new ") {
            let mut class_name = after_new.to_string();
            let mut method = String::new();
            if class_name.is_empty() {
                class_name = "<unknown>".to_string();
            } else if let Some((before, after)) = class_name.clone().split_once('.') {
                class_name = before.to_string();
                method = after.to_string();
            }
            return (class_name, method, true);
        }
        if member == "new" {
            return ("<unknown>".to_string(), String::new(), true);
        }
        match member.split_once('.') {
            Some((class_name, method)) => (class_name.to_string(), method.to_string(), false),
            None => (String::new(), member, false),
        }
    }

    /// Splits `package:foo/bar/baz.dart:12:34`.
    ///
    /// The **package is only pulled out for the `dart:` and `package:`
    /// schemes**; a `file:` frame keeps its whole path and a package of
    /// `<unknown>`, because a file path has no package to name.
    fn split_location(location: &str) -> (String, String, String, i32, i32) {
        let (uri, line_number, column) = Self::split_position(location);
        let (scheme, path) = match uri.split_once(':') {
            Some((scheme, path)) => (scheme.to_string(), path.to_string()),
            None => (String::new(), uri.to_string()),
        };
        let mut package = "<unknown>".to_string();
        let mut package_path = path.clone();
        if scheme == "dart" || scheme == "package" {
            let path = path.trim_start_matches('/');
            if let Some((first, rest)) = path.split_once('/') {
                package = first.to_string();
                package_path = rest.to_string();
            } else {
                package = path.to_string();
                package_path = String::new();
            }
        }
        (scheme, package, package_path, line_number, column)
    }

    /// Peels an optional `:line:column` off the end.
    fn split_position(location: &str) -> (String, i32, i32) {
        let parts: Vec<&str> = location.rsplitn(3, ':').collect();
        // `rsplitn` yields the pieces from the right.
        match parts.as_slice() {
            [column, line, head]
                if column.parse::<i32>().is_ok() && line.parse::<i32>().is_ok() =>
            {
                (
                    head.to_string(),
                    line.parse().unwrap_or(-1),
                    column.parse().unwrap_or(-1),
                )
            }
            [line, head] if line.parse::<i32>().is_ok() => {
                (head.to_string(), line.parse().unwrap_or(-1), -1)
            }
            [line, head, more] if line.parse::<i32>().is_ok() => {
                (format!("{more}:{head}"), line.parse().unwrap_or(-1), -1)
            }
            _ => (location.to_string(), -1, -1),
        }
    }
}

/// Upstream `PartialStackFrame`: a pattern that matches a frame.
///
/// Only three of a frame's fields are compared -- package, class and method --
/// because a filter is about *which code* a frame is in, and the line and
/// column change with every edit to that code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartialStackFrame {
    pub package: String,
    pub class_name: String,
    pub method: String,
}

impl PartialStackFrame {
    pub fn new(
        package: impl Into<String>,
        class_name: impl Into<String>,
        method: impl Into<String>,
    ) -> PartialStackFrame {
        PartialStackFrame {
            package: package.into(),
            class_name: class_name.into(),
            method: method.into(),
        }
    }

    /// Upstream's `asynchronousSuspension`.
    pub fn asynchronous_suspension() -> PartialStackFrame {
        PartialStackFrame::new("", "", "asynchronous suspension")
    }

    /// Upstream's `matches`.
    ///
    /// The package is a **pattern** matched anywhere in
    /// `scheme:package/path`, while the class and method must be equal. That
    /// asymmetry is what lets one filter name a whole library -- a package
    /// pattern of `flutter` covers every file in it -- while still pointing at
    /// one method inside it.
    pub fn matches(&self, frame: &StackFrame) -> bool {
        let whole = format!(
            "{}:{}/{}",
            frame.package_scheme, frame.package, frame.package_path
        );
        whole.contains(&self.package)
            && frame.method == self.method
            && frame.class_name == self.class_name
    }
}

/// Upstream `StackFilter`: something that marks frames as not worth reading.
///
/// A filter does **not** remove frames. It writes a *reason* beside each one it
/// recognises, and the printer collapses a run of equal reasons into a single
/// line saying what was left out. A reader who needs the detail can still be
/// told how much was hidden and why.
pub trait StackFilter {
    /// Upstream's `filter`, which writes into `reasons` in place.
    fn filter(&self, frames: &[StackFrame], reasons: &mut [Option<String>]);
}

/// Upstream `RepetitiveStackFrameFilter`: collapses a run of frames that keep
/// coming round.
///
/// The case it exists for is a build error inside a widget that rebuilds its
/// own subtree: the same handful of framework frames appears dozens of times,
/// and none of the repeats tells the reader anything the first did not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepetitiveStackFrameFilter {
    pub frames: Vec<PartialStackFrame>,
    pub replacement: String,
}

impl RepetitiveStackFrameFilter {
    pub fn new(
        frames: Vec<PartialStackFrame>,
        replacement: impl Into<String>,
    ) -> RepetitiveStackFrameFilter {
        RepetitiveStackFrameFilter {
            frames,
            replacement: replacement.into(),
        }
    }

    /// Upstream's `numFrames`.
    pub fn num_frames(&self) -> usize {
        self.frames.len()
    }

    fn matches_at(&self, frames: &[StackFrame], at: usize) -> bool {
        if frames.len() < at + self.num_frames() {
            return false;
        }
        self.frames
            .iter()
            .zip(&frames[at..at + self.num_frames()])
            .all(|(pattern, frame)| pattern.matches(frame))
    }
}

impl StackFilter for RepetitiveStackFrameFilter {
    /// Upstream's `filter`, loop bound and all.
    ///
    /// **Upstream's bound is exclusive of the last possible window.** The loop
    /// runs `index < stackFrames.length - numFrames`, so a repetition that
    /// ends exactly at the final frame is never tested and never collapsed;
    /// `<=` would be needed to reach it. Ported as written -- changing it
    /// would collapse a run upstream leaves expanded, and a stack trace that
    /// differs between the two ports is worse than one that matches upstream's
    /// oddity. See the regression line.
    fn filter(&self, frames: &[StackFrame], reasons: &mut [Option<String>]) {
        let count = self.num_frames();
        if count == 0 || frames.len() < count {
            return;
        }
        let mut index = 0;
        while index + count < frames.len() {
            if self.matches_at(frames, index) {
                for reason in reasons.iter_mut().skip(index).take(count) {
                    *reason = Some(self.replacement.clone());
                }
                index += count;
            } else {
                index += 1;
            }
        }
    }
}

/// The packages and classes upstream's `FlutterError.defaultStackFilter`
/// removes outright.
///
/// These are not collapsed with a reason -- they are **taken out entirely**,
/// because they are the machinery that got the error to the handler rather
/// than anything about the error. A reader looking for their own mistake
/// should not have to scroll past the timer that ran the callback.
pub const REMOVED_PACKAGES_AND_CLASSES: [&str; 8] = [
    "dart:async-patch",
    "dart:async",
    "package:stack_trace",
    "class _AssertionError",
    "class _FakeAsync",
    "class _FrameCallbackEntry",
    "class _Timer",
    "class _RawReceivePortImpl",
];

/// Upstream's removal pass inside `defaultStackFilter`.
///
/// Returns the frames worth keeping and how many were dropped, which is what
/// upstream reports as "(elided N frames from ...)".
pub fn remove_uninteresting_frames(frames: Vec<StackFrame>) -> (Vec<StackFrame>, usize) {
    let mut kept = Vec::with_capacity(frames.len());
    let mut skipped = 0;
    for frame in frames {
        let class_name = format!("class {}", frame.class_name);
        let package = format!("{}:{}", frame.package_scheme, frame.package);
        if REMOVED_PACKAGES_AND_CLASSES.contains(&class_name.as_str())
            || REMOVED_PACKAGES_AND_CLASSES.contains(&package.as_str())
        {
            skipped += 1;
            continue;
        }
        kept.push(frame);
    }
    (kept, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> StackFrame {
        StackFrame::from_stack_trace_line(line).expect("that line parses")
    }

    #[test]
    fn an_ordinary_frame_comes_apart_into_its_pieces() {
        let frame = parse("#3      Foo.bar (package:baz/qux.dart:12:34)");
        assert_eq!(frame.number, 3);
        assert_eq!(frame.class_name, "Foo");
        assert_eq!(frame.method, "bar");
        assert_eq!(frame.package_scheme, "package");
        assert_eq!(frame.package, "baz");
        assert_eq!(frame.package_path, "qux.dart");
        assert_eq!(frame.line, 12);
        assert_eq!(frame.column, 34);
        assert!(!frame.is_constructor);
        assert_eq!(
            frame.source, "#3      Foo.bar (package:baz/qux.dart:12:34)",
            "and the line is kept exactly as it arrived"
        );
    }

    #[test]
    fn a_closure_inside_a_method_is_that_method() {
        // Which is what a reader means by it -- the closure has no name of its
        // own to look up.
        let frame = parse("#0      Foo.bar.<anonymous closure> (package:baz/qux.dart:1:2)");
        assert_eq!(frame.class_name, "Foo");
        assert_eq!(frame.method, "bar");
    }

    #[test]
    fn a_frame_beginning_new_is_a_constructor() {
        let plain = parse("#0      new Foo (package:baz/qux.dart:1:2)");
        assert!(plain.is_constructor);
        assert_eq!(plain.class_name, "Foo");
        assert_eq!(plain.method, "", "an unnamed constructor has no method");

        // A dot in the class means a *named* constructor, and the part after
        // it is the name.
        let named = parse("#0      new Foo.bar (package:baz/qux.dart:1:2)");
        assert!(named.is_constructor);
        assert_eq!(named.class_name, "Foo");
        assert_eq!(named.method, "bar");
    }

    #[test]
    fn a_bare_function_has_no_class() {
        let frame = parse("#0      main (package:baz/qux.dart:1:2)");
        assert_eq!(frame.class_name, "");
        assert_eq!(frame.method, "main");
    }

    #[test]
    fn a_missing_position_is_minus_one_rather_than_zero() {
        // The same "no position" the two synthetic frames use, and distinct
        // from line one column zero.
        let no_column = parse("#0      main (package:baz/qux.dart:12)");
        assert_eq!(no_column.line, 12);
        assert_eq!(no_column.column, -1);

        let neither = parse("#0      main (package:baz/qux.dart)");
        assert_eq!(neither.line, -1);
        assert_eq!(neither.column, -1);
        assert_eq!(neither.package_path, "qux.dart");
    }

    #[test]
    fn only_the_dart_and_package_schemes_have_a_package_to_name() {
        // A file path has none, so it keeps its whole path and says so.
        let from_package = parse("#0      main (package:baz/lib/qux.dart:1:2)");
        assert_eq!(from_package.package, "baz");
        assert_eq!(from_package.package_path, "lib/qux.dart");

        let from_dart = parse("#0      main (dart:async/future.dart:1:2)");
        assert_eq!(from_dart.package_scheme, "dart");
        assert_eq!(from_dart.package, "async");
        assert_eq!(from_dart.package_path, "future.dart");

        let from_file = parse("#0      main (file:///home/me/qux.dart:1:2)");
        assert_eq!(from_file.package_scheme, "file");
        assert_eq!(from_file.package, "<unknown>");
        assert_eq!(from_file.package_path, "///home/me/qux.dart");
    }

    #[test]
    fn the_two_synthetic_frames_are_recognised_by_their_whole_line() {
        // Neither is a real frame: one marks where a stack crosses an await,
        // the other stands in for the thousands a stack overflow produced.
        let gap = parse("<asynchronous suspension>");
        assert_eq!(gap, StackFrame::asynchronous_suspension());
        assert_eq!(gap.number, -1);

        let elision = parse("...");
        assert_eq!(elision, StackFrame::stack_overflow_elision());
    }

    #[test]
    fn a_line_that_does_not_parse_costs_only_itself() {
        // Upstream's comment: on the web a non-debug build prints the
        // exception message above the trace, and one unparseable line should
        // not cost the reader every other frame.
        let frames = StackFrame::from_stack_string(
            "Error: something went wrong\n\
             #0      main (package:baz/qux.dart:1:2)\n\
             \n\
             #1      Foo.bar (package:baz/qux.dart:3:4)\n",
        );
        assert_eq!(frames.len(), 2, "the message and the blank line are gone");
        assert_eq!(frames[0].number, 0);
        assert_eq!(frames[1].number, 1);
    }

    // -- Filtering -------------------------------------------------------

    fn frames(lines: &[&str]) -> Vec<StackFrame> {
        lines.iter().map(|line| parse(line)).collect()
    }

    fn framework(method: &str) -> String {
        format!("#0      Widget.{method} (package:flutter/src/widgets/framework.dart:1:2)")
    }

    #[test]
    fn a_partial_frame_matches_the_code_and_not_the_line_number() {
        // A filter is about which code a frame is in, and the line and column
        // change with every edit to that code.
        let pattern = PartialStackFrame::new("flutter", "Widget", "build");
        assert!(pattern.matches(&parse(&framework("build"))));
        assert!(pattern.matches(&parse(
            "#9      Widget.build (package:flutter/src/widgets/framework.dart:999:1)"
        )));
        assert!(!pattern.matches(&parse(&framework("layout"))));
    }

    #[test]
    fn the_package_is_a_pattern_while_the_class_and_method_must_be_equal() {
        // Which lets one filter name a whole library while still pointing at
        // one method inside it.
        let pattern = PartialStackFrame::new("flutter", "Widget", "build");
        assert!(
            pattern.matches(&parse(
                "#0      Widget.build (package:flutter/src/widgets/framework.dart:1:2)"
            )),
            "the package name appears in the path"
        );
        assert!(
            pattern.matches(&parse(
                "#0      Widget.build (package:flutter_test/src/widget_tester.dart:1:2)"
            )),
            "a package merely containing the pattern matches too -- upstream's              `package` is a Pattern matched with allMatches, not an equality"
        );

        let almost = PartialStackFrame::new("flutter", "Widget", "buil");
        assert!(
            !almost.matches(&parse(&framework("build"))),
            "but the method is compared whole"
        );
    }

    #[test]
    fn a_repetitive_run_is_marked_with_one_reason_for_every_frame_in_it() {
        // A filter does not remove frames: it writes a reason beside each one,
        // and the printer collapses a run of equal reasons into one line. A
        // reader can still be told how much was hidden and why.
        let filter = RepetitiveStackFrameFilter::new(
            vec![
                PartialStackFrame::new("flutter", "Widget", "build"),
                PartialStackFrame::new("flutter", "Element", "rebuild"),
            ],
            "...     Normal element mounting",
        );
        let stack = frames(&[
            "#0      main (package:app/main.dart:1:2)",
            "#1      Widget.build (package:flutter/src/widgets/framework.dart:1:2)",
            "#2      Element.rebuild (package:flutter/src/widgets/framework.dart:3:4)",
            "#3      other (package:app/main.dart:5:6)",
            "#4      last (package:app/main.dart:7:8)",
        ]);
        let mut reasons = vec![None; stack.len()];
        filter.filter(&stack, &mut reasons);

        assert_eq!(reasons[0], None);
        assert_eq!(
            reasons[1].as_deref(),
            Some("...     Normal element mounting")
        );
        assert_eq!(
            reasons[2].as_deref(),
            Some("...     Normal element mounting")
        );
        assert_eq!(reasons[3], None);
    }

    #[test]
    fn a_repetition_ending_at_the_very_last_frame_is_left_expanded() {
        // Upstream's loop bound is `index < length - numFrames`, exclusive of
        // the last possible window, so the final run is never tested. `<=`
        // would reach it. Ported as written: a stack trace that differs
        // between the two ports is worse than one that matches upstream's
        // oddity, and this line is what keeps the choice deliberate.
        let filter = RepetitiveStackFrameFilter::new(
            vec![
                PartialStackFrame::new("flutter", "Widget", "build"),
                PartialStackFrame::new("flutter", "Element", "rebuild"),
            ],
            "collapsed",
        );
        let stack = frames(&[
            "#0      main (package:app/main.dart:1:2)",
            "#1      Widget.build (package:flutter/src/widgets/framework.dart:1:2)",
            "#2      Element.rebuild (package:flutter/src/widgets/framework.dart:3:4)",
        ]);
        let mut reasons = vec![None; stack.len()];
        filter.filter(&stack, &mut reasons);
        assert_eq!(
            reasons,
            vec![None, None, None],
            "the run reaches the end, so upstream never looks at it"
        );

        // One more frame after it and the same run is found.
        let stack = frames(&[
            "#0      main (package:app/main.dart:1:2)",
            "#1      Widget.build (package:flutter/src/widgets/framework.dart:1:2)",
            "#2      Element.rebuild (package:flutter/src/widgets/framework.dart:3:4)",
            "#3      after (package:app/main.dart:5:6)",
        ]);
        let mut reasons = vec![None; stack.len()];
        filter.filter(&stack, &mut reasons);
        assert_eq!(reasons[1].as_deref(), Some("collapsed"));
    }

    #[test]
    fn a_matched_run_is_stepped_over_rather_than_rescanned() {
        let filter = RepetitiveStackFrameFilter::new(
            vec![PartialStackFrame::new("flutter", "Widget", "build")],
            "collapsed",
        );
        let stack = frames(&[
            "#0      Widget.build (package:flutter/a.dart:1:2)",
            "#1      Widget.build (package:flutter/a.dart:1:2)",
            "#2      Widget.build (package:flutter/a.dart:1:2)",
            "#3      main (package:app/main.dart:1:2)",
        ]);
        let mut reasons = vec![None; stack.len()];
        filter.filter(&stack, &mut reasons);
        assert_eq!(reasons[0].as_deref(), Some("collapsed"));
        assert_eq!(reasons[1].as_deref(), Some("collapsed"));
        assert_eq!(reasons[2].as_deref(), Some("collapsed"));
    }

    #[test]
    fn a_filter_with_more_frames_than_the_stack_marks_nothing() {
        let filter = RepetitiveStackFrameFilter::new(
            vec![
                PartialStackFrame::new("flutter", "Widget", "build"),
                PartialStackFrame::new("flutter", "Element", "rebuild"),
                PartialStackFrame::new("flutter", "Element", "mount"),
            ],
            "collapsed",
        );
        let stack = frames(&["#0      Widget.build (package:flutter/a.dart:1:2)"]);
        let mut reasons = vec![None; stack.len()];
        filter.filter(&stack, &mut reasons);
        assert_eq!(reasons, vec![None]);
        assert_eq!(filter.num_frames(), 3);
    }

    #[test]
    fn the_machinery_that_delivered_the_error_is_taken_out_entirely() {
        // Not collapsed with a reason: a reader looking for their own mistake
        // should not have to scroll past the timer that ran the callback.
        let stack = frames(&[
            "#0      main (package:app/main.dart:1:2)",
            "#1      _Timer._runTimers (dart:isolate-patch/timer_impl.dart:1:2)",
            "#2      Future.then (dart:async/future.dart:3:4)",
            "#3      Trace.terse (package:stack_trace/src/trace.dart:5:6)",
            "#4      Widget.build (package:flutter/a.dart:7:8)",
        ]);
        let (kept, skipped) = remove_uninteresting_frames(stack);
        assert_eq!(skipped, 3);
        let methods: Vec<&str> = kept.iter().map(|frame| frame.method.as_str()).collect();
        assert_eq!(methods, vec!["main", "build"]);
    }

    #[test]
    fn a_frame_from_an_ordinary_package_is_never_removed() {
        let stack = frames(&[
            "#0      main (package:app/main.dart:1:2)",
            "#1      Foo.bar (package:my_async/thing.dart:3:4)",
        ]);
        let (kept, skipped) = remove_uninteresting_frames(stack);
        assert_eq!(skipped, 0, "my_async is not dart:async");
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn the_removal_list_is_upstreams() {
        assert_eq!(REMOVED_PACKAGES_AND_CLASSES.len(), 8);
        assert!(REMOVED_PACKAGES_AND_CLASSES.contains(&"dart:async"));
        assert!(REMOVED_PACKAGES_AND_CLASSES.contains(&"dart:async-patch"));
        assert!(REMOVED_PACKAGES_AND_CLASSES.contains(&"class _AssertionError"));
    }
}
