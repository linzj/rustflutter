// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Licences, timings and the bidirectional control characters (upstream
//! `foundation/licenses.dart`, `foundation/timeline.dart`,
//! `foundation/unicode.dart`).
//!
//! Three small things `foundation` keeps in three files, together here for
//! the reason the earlier merges had: each is one idea and none fills a file.
//!
//! # Recorded divergences
//!
//! * Upstream's `LicenseRegistry.licenses` is a `Stream`, because a collector
//!   may read a file. The collectors here return their entries outright:
//!   there is no executor, and the one collector that matters is the build's
//!   own generated table, which is already in memory.
//! * Upstream's `FlutterTimeline` writes to the Dart timeline as well as
//!   collecting; there is no timeline to write to here, so what is ported is
//!   the collecting half -- which is the half with the arithmetic in it, and
//!   the half a test can check.

// -- Licences -----------------------------------------------------------------

/// Upstream `LicenseParagraph`: one paragraph of a licence, and how far in it
/// sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LicenseParagraph {
    pub text: String,
    /// How far the paragraph is indented, or
    /// [`LicenseParagraph::CENTERED_INDENT`].
    pub indent: i32,
}

impl LicenseParagraph {
    /// Upstream `LicenseParagraph.centeredIndent`: not an indent at all but a
    /// guess that the line was centred, which is what a licence's title
    /// usually is.
    pub const CENTERED_INDENT: i32 = -1;

    pub fn new(text: impl Into<String>, indent: i32) -> LicenseParagraph {
        LicenseParagraph {
            text: text.into(),
            indent,
        }
    }
}

/// Upstream `LicenseEntry`: one licence, and what it covers.
pub trait LicenseEntry {
    fn packages(&self) -> Vec<String>;
    fn paragraphs(&self) -> Vec<LicenseParagraph>;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParserState {
    BeforeParagraph,
    InParagraph,
}

/// Upstream `LicenseEntryWithLineBreaks`: a licence as the plain text it came
/// as, parsed into paragraphs on the way out.
///
/// The parsing is the whole class. A licence file is hard-wrapped prose, so
/// the line breaks inside a paragraph are not paragraph breaks and the blank
/// lines are; and the indentation has to be read back out of the leading
/// spaces, because nothing else recorded it.
pub struct LicenseEntryWithLineBreaks {
    pub packages: Vec<String>,
    pub text: String,
}

impl LicenseEntryWithLineBreaks {
    pub fn new(packages: Vec<String>, text: impl Into<String>) -> LicenseEntryWithLineBreaks {
        LicenseEntryWithLineBreaks {
            packages,
            text: text.into(),
        }
    }
}

impl LicenseEntry for LicenseEntryWithLineBreaks {
    fn packages(&self) -> Vec<String> {
        self.packages.clone()
    }

    /// Upstream's `paragraphs` getter, character by character.
    ///
    /// Two states: between paragraphs, counting the indentation, and inside
    /// one, looking for the end of the line. Upstream's own comments call the
    /// indentation rule a wild heuristic and say what it was fitted to -- the
    /// common BSD and LGPL texts -- and it is ported as it is, because a
    /// licence rendered differently from upstream's is a licence displayed
    /// wrongly.
    fn paragraphs(&self) -> Vec<LicenseParagraph> {
        let characters: Vec<char> = self.text.chars().collect();
        let mut line_start = 0usize;
        let mut position = 0usize;
        let mut last_line_indent = 0i32;
        let mut current_line_indent = 0i32;
        let mut paragraph_indent: Option<i32> = None;
        let mut state = ParserState::BeforeParagraph;
        let mut lines: Vec<String> = Vec::new();
        let mut result: Vec<LicenseParagraph> = Vec::new();

        while position < characters.len() {
            match state {
                ParserState::BeforeParagraph => match characters[position] {
                    ' ' => {
                        line_start = position + 1;
                        current_line_indent += 1;
                    }
                    '\t' => {
                        line_start = position + 1;
                        // Upstream's tab is eight columns, not one.
                        current_line_indent += 8;
                    }
                    '\r' | '\n' | '\u{000C}' => {
                        if !lines.is_empty() {
                            result.push(take_paragraph(&mut lines, paragraph_indent));
                        }
                        // A CRLF is one break, not two -- otherwise every
                        // paragraph in a file written on Windows is followed
                        // by an empty one.
                        if characters[position] == '\r'
                            && position + 1 < characters.len()
                            && characters[position + 1] == '\n'
                        {
                            position += 1;
                        }
                        last_line_indent = 0;
                        current_line_indent = 0;
                        paragraph_indent = None;
                        line_start = position + 1;
                    }
                    character => {
                        // Upstream's hack for the LGPL 2.1, which opens with a
                        // bracketed paragraph whose continuation lines are
                        // indented one further than its first. Counting the
                        // bracket as indentation is what keeps the two lines
                        // in one paragraph.
                        if character == '[' {
                            current_line_indent += 1;
                        }
                        if !lines.is_empty() && current_line_indent > last_line_indent {
                            result.push(take_paragraph(&mut lines, paragraph_indent));
                            paragraph_indent = None;
                        }
                        if paragraph_indent.is_none() {
                            // Upstream calls this a wild heuristic and says
                            // what it was fitted to: past ten columns the line
                            // is taken for a centred title, and otherwise one
                            // level of indent is three columns.
                            paragraph_indent = Some(if current_line_indent > 10 {
                                LicenseParagraph::CENTERED_INDENT
                            } else {
                                current_line_indent / 3
                            });
                        }
                        state = ParserState::InParagraph;
                    }
                },
                ParserState::InParagraph => match characters[position] {
                    '\n' => {
                        lines.push(slice(&characters, line_start, position));
                        last_line_indent = current_line_indent;
                        current_line_indent = 0;
                        line_start = position + 1;
                        state = ParserState::BeforeParagraph;
                    }
                    '\u{000C}' => {
                        lines.push(slice(&characters, line_start, position));
                        result.push(take_paragraph(&mut lines, paragraph_indent));
                        last_line_indent = 0;
                        current_line_indent = 0;
                        paragraph_indent = None;
                        line_start = position + 1;
                        state = ParserState::BeforeParagraph;
                    }
                    _ => {}
                },
            }
            position += 1;
        }
        // Whatever is left over. Upstream's two cases: between paragraphs
        // there may be finished lines waiting, and inside one the last line
        // has not been taken yet.
        match state {
            ParserState::BeforeParagraph => {
                if !lines.is_empty() {
                    result.push(take_paragraph(&mut lines, paragraph_indent));
                }
            }
            ParserState::InParagraph => {
                lines.push(slice(&characters, line_start, position));
                result.push(take_paragraph(&mut lines, paragraph_indent));
            }
        }
        result
    }
}

fn slice(characters: &[char], start: usize, end: usize) -> String {
    characters[start..end].iter().collect()
}

/// Upstream's `getParagraph`: the lines so far, joined with single spaces.
///
/// The join is what unwraps the hard-wrapped prose, and it is why a licence
/// reflows to whatever width it is shown at.
fn take_paragraph(lines: &mut Vec<String>, indent: Option<i32>) -> LicenseParagraph {
    let text = lines.join(" ");
    lines.clear();
    LicenseParagraph::new(text, indent.unwrap_or(0))
}

/// Upstream `LicenseRegistry`: where everything that has a licence puts it.
pub struct LicenseRegistry;

type Collector = Box<dyn Fn() -> Vec<Box<dyn LicenseEntry>>>;

thread_local! {
    static COLLECTORS: std::cell::RefCell<Vec<Collector>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl LicenseRegistry {
    /// Upstream `addLicense`.
    pub fn add_license(collector: impl Fn() -> Vec<Box<dyn LicenseEntry>> + 'static) {
        COLLECTORS.with(|collectors| collectors.borrow_mut().push(Box::new(collector)));
    }

    /// Upstream `licenses`, as a list rather than a stream; see the module's
    /// divergences.
    ///
    /// The collectors run in the order they were added, which is upstream's
    /// -- a licence page is read top to bottom and the order it was built in
    /// is the only order there is.
    pub fn licenses() -> Vec<Box<dyn LicenseEntry>> {
        COLLECTORS.with(|collectors| {
            collectors
                .borrow()
                .iter()
                .flat_map(|collector| collector())
                .collect()
        })
    }

    /// Upstream `reset`, which is `@visibleForTesting` there and here.
    pub fn reset() {
        COLLECTORS.with(|collectors| collectors.borrow_mut().clear());
    }
}

// -- Timings ------------------------------------------------------------------

/// Upstream `TimedBlock`: one span of work that was measured.
#[derive(Clone, Debug, PartialEq)]
pub struct TimedBlock {
    pub name: String,
    pub start: f64,
    pub end: f64,
}

impl TimedBlock {
    pub fn new(name: impl Into<String>, start: f64, end: f64) -> TimedBlock {
        TimedBlock {
            name: name.into(),
            start,
            end,
        }
    }

    pub fn duration(&self) -> f64 {
        self.end - self.start
    }
}

/// Upstream `AggregatedTimedBlock`: everything that happened under one name.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregatedTimedBlock {
    pub name: String,
    pub duration: f64,
    pub count: usize,
}

impl AggregatedTimedBlock {
    pub fn new(name: impl Into<String>, duration: f64, count: usize) -> AggregatedTimedBlock {
        AggregatedTimedBlock {
            name: name.into(),
            duration,
            count,
        }
    }
}

/// Upstream `AggregatedTimings`: the blocks, and the sums.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregatedTimings {
    pub timed_blocks: Vec<TimedBlock>,
}

impl AggregatedTimings {
    pub fn new(timed_blocks: Vec<TimedBlock>) -> AggregatedTimings {
        AggregatedTimings { timed_blocks }
    }

    /// Upstream's `_computeAggregatedBlocks`, which keeps the order the names
    /// were first seen in. A map that reordered them would make two runs of
    /// the same frame print differently.
    pub fn aggregated_blocks(&self) -> Vec<AggregatedTimedBlock> {
        let mut aggregate: Vec<AggregatedTimedBlock> = Vec::new();
        for block in &self.timed_blocks {
            match aggregate.iter_mut().find(|entry| entry.name == block.name) {
                Some(entry) => {
                    entry.duration += block.duration();
                    entry.count += 1;
                }
                None => aggregate.push(AggregatedTimedBlock::new(
                    block.name.clone(),
                    block.duration(),
                    1,
                )),
            }
        }
        aggregate
    }

    /// Upstream `getAggregated`: a name that was never timed answers a zero
    /// block rather than nothing, so that a caller printing a table does not
    /// have to special-case the row that never ran.
    pub fn aggregated(&self, name: &str) -> AggregatedTimedBlock {
        self.aggregated_blocks()
            .into_iter()
            .find(|block| block.name == name)
            .unwrap_or_else(|| AggregatedTimedBlock::new(name, 0.0, 0))
    }
}

/// Upstream `FlutterTimeline`: measuring what the framework spends its time
/// on.
///
/// Collection is off until it is switched on, which is upstream's rule and
/// the reason the timing calls can be left in: an unmeasured `time_sync` is a
/// call and a return.
pub struct FlutterTimeline;

thread_local! {
    static COLLECTION_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static OPEN_BLOCKS: std::cell::RefCell<Vec<(String, f64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static FINISHED_BLOCKS: std::cell::RefCell<Vec<TimedBlock>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl FlutterTimeline {
    /// Upstream `debugCollectionEnabled`. Turning it off clears what was
    /// collected, which is upstream's behaviour: a half-collected run is not
    /// a measurement of anything.
    pub fn set_collection_enabled(enabled: bool) {
        if COLLECTION_ENABLED.with(|flag| flag.get()) == enabled {
            return;
        }
        COLLECTION_ENABLED.with(|flag| flag.set(enabled));
        FlutterTimeline::debug_reset();
    }

    pub fn collection_enabled() -> bool {
        COLLECTION_ENABLED.with(|flag| flag.get())
    }

    /// Upstream `startSync`. `now` is the caller's clock, because this crate
    /// has no ambient one -- and because a test that supplies its own is the
    /// only way to check the arithmetic.
    pub fn start_sync(name: &str, now: f64) {
        if !FlutterTimeline::collection_enabled() {
            return;
        }
        OPEN_BLOCKS.with(|blocks| blocks.borrow_mut().push((name.to_string(), now)));
    }

    /// Upstream `finishSync`, which closes the innermost open block.
    pub fn finish_sync(now: f64) {
        if !FlutterTimeline::collection_enabled() {
            return;
        }
        let finished = OPEN_BLOCKS.with(|blocks| blocks.borrow_mut().pop());
        if let Some((name, start)) = finished {
            FINISHED_BLOCKS
                .with(|blocks| blocks.borrow_mut().push(TimedBlock::new(name, start, now)));
        }
    }

    /// Upstream `debugCollect`, which upstream throws from if collection was
    /// never switched on. Here that is an empty set of timings: nothing was
    /// measured, and that is what nothing measured looks like.
    pub fn debug_collect() -> AggregatedTimings {
        let blocks = FINISHED_BLOCKS.with(|blocks| blocks.borrow().clone());
        FlutterTimeline::debug_reset();
        AggregatedTimings::new(blocks)
    }

    /// Upstream `debugReset`.
    pub fn debug_reset() {
        OPEN_BLOCKS.with(|blocks| blocks.borrow_mut().clear());
        FINISHED_BLOCKS.with(|blocks| blocks.borrow_mut().clear());
    }
}

// -- Unicode ------------------------------------------------------------------

/// Upstream `Unicode`: the bidirectional control characters.
///
/// They are here rather than written inline anywhere because they are
/// invisible: a stray one in a source file is impossible to see and changes
/// how everything after it is laid out.
pub struct Unicode;

impl Unicode {
    /// Left-to-right embedding.
    pub const LRE: char = '\u{202A}';
    /// Right-to-left embedding.
    pub const RLE: char = '\u{202B}';
    /// Pop directional formatting: ends the nearest embedding or override.
    pub const PDF: char = '\u{202C}';
    /// Left-to-right override.
    pub const LRO: char = '\u{202D}';
    /// Right-to-left override.
    pub const RLO: char = '\u{202E}';
    /// Left-to-right isolate.
    pub const LRI: char = '\u{2066}';
    /// Right-to-left isolate.
    pub const RLI: char = '\u{2067}';
    /// First strong isolate: the direction is whatever the first strongly
    /// directional character inside says it is.
    pub const FSI: char = '\u{2068}';
    /// Pop directional isolate: ends the nearest isolate.
    pub const PDI: char = '\u{2069}';
    /// Left-to-right mark: a zero-width character with a direction and
    /// nothing else.
    pub const LRM: char = '\u{200E}';
    /// Right-to-left mark.
    pub const RLM: char = '\u{200F}';
    /// Arabic letter mark.
    pub const ALM: char = '\u{061C}';
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraphs(text: &str) -> Vec<LicenseParagraph> {
        LicenseEntryWithLineBreaks::new(vec!["a".to_string()], text).paragraphs()
    }

    #[test]
    fn hard_wrapped_lines_join_into_one_paragraph() {
        // A licence file is wrapped prose: the breaks inside a paragraph are
        // not paragraph breaks. Joining with single spaces is what lets it
        // reflow to whatever width it is shown at.
        let parsed = paragraphs("one two\nthree four\n\nnext");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].text, "one two three four");
        assert_eq!(parsed[1].text, "next");
    }

    #[test]
    fn a_blank_line_is_what_ends_a_paragraph() {
        let parsed = paragraphs("first\n\nsecond\n\n\nthird");
        assert_eq!(
            parsed.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
            vec!["first", "second", "third"],
            "runs of blank lines are one break, not several empty paragraphs"
        );
    }

    #[test]
    fn a_crlf_between_paragraphs_is_one_break_and_not_two() {
        // Otherwise every paragraph in a licence written on Windows is
        // followed by an empty one. The collapse lives in the
        // between-paragraphs state, which is where upstream puts it.
        let parsed = paragraphs("first\r\n\r\nsecond");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].text, "second");
    }

    #[test]
    fn a_carriage_return_inside_a_paragraph_stays_in_the_text() {
        // Upstream's quirk, asserted rather than tidied away. The
        // in-paragraph state watches for a line feed and a form feed only, so
        // the carriage return of a CRLF that ends a *wrapped line* is part of
        // that line and comes out inside the joined paragraph. Only a CRLF
        // that ends the paragraph is collapsed.
        let parsed = paragraphs("first\r\nsecond");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].text, "first\r second");
    }

    #[test]
    fn one_level_of_indent_is_three_columns() {
        // Upstream calls this a wild heuristic and says what it was fitted
        // to. It is ported as it is, because a licence laid out differently
        // from upstream's is a licence displayed wrongly.
        assert_eq!(paragraphs("none")[0].indent, 0);
        assert_eq!(paragraphs("  two spaces")[0].indent, 0);
        assert_eq!(paragraphs("   three spaces")[0].indent, 1);
        assert_eq!(paragraphs("      six spaces")[0].indent, 2);
    }

    #[test]
    fn a_line_indented_past_ten_columns_is_taken_for_a_centred_title() {
        // Which is what a licence's heading usually is, and why the answer is
        // a marker rather than a bigger number: it is not an indent at all.
        assert_eq!(
            paragraphs("           eleven spaces")[0].indent,
            LicenseParagraph::CENTERED_INDENT
        );
        assert_eq!(LicenseParagraph::CENTERED_INDENT, -1);
    }

    #[test]
    fn a_tab_is_eight_columns() {
        // One column would put a tab-indented paragraph at indent zero, which
        // is the same as no indent at all.
        assert_eq!(paragraphs("\tone tab")[0].indent, 2);
        assert_eq!(
            paragraphs("\t\ttwo tabs")[0].indent,
            LicenseParagraph::CENTERED_INDENT,
            "sixteen columns is past the centring threshold"
        );
    }

    #[test]
    fn a_line_indented_further_than_the_last_starts_a_new_paragraph() {
        // Without this, a licence's indented sub-clauses run into the
        // sentence above them.
        let parsed = paragraphs("first line\n    indented further");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].text, "first line");
        assert_eq!(parsed[1].text, "indented further");
        assert_eq!(parsed[1].indent, 1);
    }

    #[test]
    fn an_opening_bracket_counts_as_indentation() {
        // Upstream's hack, and it names the licence it is for: the LGPL 2.1
        // opens with a bracketed paragraph whose continuation is indented one
        // further than its first line. Without counting the bracket, the two
        // lines come out as two paragraphs.
        let parsed = paragraphs("[this is a\n single paragraph]");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].text, "[this is a single paragraph]");
    }

    #[test]
    fn a_form_feed_ends_a_paragraph_wherever_it_falls() {
        // Licences are sometimes page-broken, and a page break in the middle
        // of a line still ends the paragraph.
        let parsed = paragraphs("before\u{000C}after");
        assert_eq!(
            parsed.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
            vec!["before", "after"]
        );
    }

    #[test]
    fn the_last_paragraph_comes_out_with_no_trailing_newline() {
        // The two end-of-input cases: inside a paragraph the last line has
        // not been taken yet, and between paragraphs there may be finished
        // lines waiting. Dropping either loses the end of the licence.
        assert_eq!(paragraphs("only")[0].text, "only");
        assert_eq!(paragraphs("only\n")[0].text, "only");
    }

    #[test]
    fn the_registry_hands_back_what_was_added_in_the_order_it_was_added() {
        // A licence page is read top to bottom, and the order it was built in
        // is the only order there is.
        LicenseRegistry::reset();
        LicenseRegistry::add_license(|| {
            vec![Box::new(LicenseEntryWithLineBreaks::new(
                vec!["first".to_string()],
                "one",
            ))]
        });
        LicenseRegistry::add_license(|| {
            vec![Box::new(LicenseEntryWithLineBreaks::new(
                vec!["second".to_string()],
                "two",
            ))]
        });
        let licenses = LicenseRegistry::licenses();
        assert_eq!(
            licenses
                .iter()
                .flat_map(|entry| entry.packages())
                .collect::<Vec<_>>(),
            vec!["first".to_string(), "second".to_string()]
        );
        LicenseRegistry::reset();
        assert!(LicenseRegistry::licenses().is_empty());
    }

    #[test]
    fn timings_add_up_per_name_and_keep_the_order_they_were_first_seen() {
        // A map that reordered the names would make two runs of the same
        // frame print differently, which is the one thing a timing table has
        // to not do.
        let timings = AggregatedTimings::new(vec![
            TimedBlock::new("build", 0.0, 2.0),
            TimedBlock::new("layout", 2.0, 5.0),
            TimedBlock::new("build", 5.0, 6.0),
        ]);
        let blocks = timings.aggregated_blocks();
        assert_eq!(
            blocks.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
            vec!["build", "layout"]
        );
        assert_eq!(blocks[0].duration, 3.0);
        assert_eq!(blocks[0].count, 2);
    }

    #[test]
    fn a_name_that_was_never_timed_answers_a_zero_block() {
        // So that a caller printing a table does not have to special-case the
        // row that never ran.
        let timings = AggregatedTimings::new(vec![TimedBlock::new("build", 0.0, 1.0)]);
        let missing = timings.aggregated("paint");
        assert_eq!(missing.duration, 0.0);
        assert_eq!(missing.count, 0);
        assert_eq!(missing.name, "paint");
    }

    #[test]
    fn nothing_is_collected_until_collection_is_switched_on() {
        // The reason the timing calls can be left in the framework: an
        // unmeasured block is a call and a return.
        FlutterTimeline::set_collection_enabled(false);
        FlutterTimeline::start_sync("build", 0.0);
        FlutterTimeline::finish_sync(1.0);
        assert!(FlutterTimeline::debug_collect().timed_blocks.is_empty());

        FlutterTimeline::set_collection_enabled(true);
        FlutterTimeline::start_sync("build", 0.0);
        FlutterTimeline::finish_sync(1.0);
        let collected = FlutterTimeline::debug_collect();
        assert_eq!(collected.timed_blocks.len(), 1);
        assert_eq!(collected.timed_blocks[0].duration(), 1.0);
        // Collecting clears, so the next collection is the next run.
        assert!(FlutterTimeline::debug_collect().timed_blocks.is_empty());
        FlutterTimeline::set_collection_enabled(false);
    }

    #[test]
    fn nested_blocks_close_innermost_first() {
        // A stack and not a queue: layout inside build finishes before build
        // does, and pairing them the other way round gives every inner block
        // the outer one's duration.
        FlutterTimeline::set_collection_enabled(true);
        FlutterTimeline::start_sync("build", 0.0);
        FlutterTimeline::start_sync("layout", 1.0);
        FlutterTimeline::finish_sync(3.0);
        FlutterTimeline::finish_sync(10.0);
        let collected = FlutterTimeline::debug_collect();
        assert_eq!(
            collected
                .timed_blocks
                .iter()
                .map(|b| (b.name.as_str(), b.duration()))
                .collect::<Vec<_>>(),
            vec![("layout", 2.0), ("build", 10.0)]
        );
        FlutterTimeline::set_collection_enabled(false);
    }

    #[test]
    fn the_bidi_controls_are_the_code_points_they_name() {
        // Invisible characters: a wrong one here is impossible to see and
        // changes how everything after it is laid out.
        assert_eq!(Unicode::LRE, '\u{202A}');
        assert_eq!(Unicode::RLE, '\u{202B}');
        assert_eq!(Unicode::PDF, '\u{202C}');
        assert_eq!(Unicode::LRO, '\u{202D}');
        assert_eq!(Unicode::RLO, '\u{202E}');
        assert_eq!(Unicode::LRI, '\u{2066}');
        assert_eq!(Unicode::RLI, '\u{2067}');
        assert_eq!(Unicode::FSI, '\u{2068}');
        assert_eq!(Unicode::PDI, '\u{2069}');
        assert_eq!(Unicode::LRM, '\u{200E}');
        assert_eq!(Unicode::RLM, '\u{200F}');
        assert_eq!(Unicode::ALM, '\u{061C}');
    }
}
