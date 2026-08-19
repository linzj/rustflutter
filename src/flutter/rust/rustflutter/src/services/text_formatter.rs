// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Refusing what a field will not hold (upstream
//! `services/text_formatter.dart`).
//!
//! A formatter sits between the keyboard and the field and gets to rewrite
//! every edit before it lands: drop the letters from a numeric field, stop
//! at a hundred characters, keep a line on one line. It is the only place a
//! field can say no to something the platform already did.
//!
//! # Working in code units
//!
//! The arithmetic below runs over UTF-16 code units, because that is what a
//! [`TextEditingValue`]'s offsets are and what upstream's Dart string indices
//! are. Everything is decoded to a `Vec<u16>` and re-encoded once, rather
//! than converted at every index -- the offsets and the text then cannot
//! disagree, which is the failure this arithmetic is prone to.
//!
//! # Recorded divergences
//!
//! * Upstream's filter takes a Dart `Pattern`, which is a `String` or a
//!   `RegExp`. There is no regular expression engine in this crate, so
//!   [`TextPattern`] is the closed set that covers what upstream itself uses
//!   and what fields ask for: a literal, or a test on one character.
//!   `FilteringTextInputFormatter.digitsOnly` is `RegExp(r'[0-9]')` and
//!   `singleLineFormatter` is the literal `'\n'`, and both are here.
//! * `maxLength` counts extended grapheme clusters upstream and Unicode
//!   scalar values here, the same boundary recorded on
//!   [`CharacterBoundary`](crate::services::text_boundary::CharacterBoundary).
//! * Upstream's default enforcement depends on `TargetPlatform`, which this
//!   crate does not have. The default here is
//!   [`MaxLengthEnforcement::Enforced`], which is upstream's answer for
//!   Android and Windows -- the two platforms this crate is verified on.

use crate::services::text_input::TextEditingValue;

/// Upstream `MaxLengthEnforcement`: what a field does when the limit is
/// reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MaxLengthEnforcement {
    /// The limit is a suggestion. The field goes over it and something else
    /// -- a counter turning red -- says so.
    None,
    /// The limit is a wall.
    #[default]
    Enforced,
    /// A wall, except while an input method is composing: half a Japanese
    /// word is not a word, and cutting one off mid-composition loses the
    /// whole of it rather than the last character.
    TruncateAfterCompositionEnds,
}

/// What a [`FilteringTextInputFormatter`] matches on.
///
/// Upstream's `Pattern` in the two forms this crate can evaluate; see the
/// module's divergences.
#[derive(Clone)]
pub enum TextPattern {
    /// A run of characters, matched wherever it occurs.
    Literal(String),
    /// A test on one character, which is what a character class is.
    Chars(std::rc::Rc<dyn Fn(char) -> bool>),
}

impl TextPattern {
    pub fn chars(test: impl Fn(char) -> bool + 'static) -> TextPattern {
        TextPattern::Chars(std::rc::Rc::new(test))
    }

    /// Every match, as half-open ranges of UTF-16 code units, in order and
    /// non-overlapping -- upstream's `Pattern.allMatches`.
    fn matches(&self, units: &[u16]) -> Vec<(usize, usize)> {
        match self {
            TextPattern::Literal(literal) => {
                let needle: Vec<u16> = literal.encode_utf16().collect();
                if needle.is_empty() || needle.len() > units.len() {
                    return Vec::new();
                }
                let mut found = Vec::new();
                let mut at = 0;
                while at + needle.len() <= units.len() {
                    if units[at..at + needle.len()] == needle[..] {
                        found.push((at, at + needle.len()));
                        at += needle.len();
                    } else {
                        at += 1;
                    }
                }
                found
            }
            TextPattern::Chars(test) => {
                let text = String::from_utf16_lossy(units);
                let mut found = Vec::new();
                let mut at = 0;
                for character in text.chars() {
                    let width = character.len_utf16();
                    if test(character) {
                        found.push((at, at + width));
                    }
                    at += width;
                }
                found
            }
        }
    }
}

impl std::fmt::Debug for TextPattern {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextPattern::Literal(literal) => formatter
                .debug_tuple("TextPattern::Literal")
                .field(literal)
                .finish(),
            TextPattern::Chars(_) => formatter.write_str("TextPattern::Chars(..)"),
        }
    }
}

/// Upstream `TextInputFormatter`.
pub trait TextInputFormatter {
    /// Upstream `formatEditUpdate`: the value the field should take, given
    /// what it held and what the platform just made of it.
    fn format_edit_update(
        &self,
        old_value: &TextEditingValue,
        new_value: &TextEditingValue,
    ) -> TextEditingValue;
}

/// Upstream's `_TextEditingValueAccumulator`: the half-built answer while a
/// filter walks the text.
///
/// The offsets move as the text under them changes, which is the whole reason
/// this exists -- a filter that rewrote the text and left the selection where
/// it was would put the caret somewhere else on every keystroke.
struct Accumulator {
    input: TextEditingValue,
    units: Vec<u16>,
    output: Vec<u16>,
    selection: Option<(i32, i32)>,
    composing: Option<(i32, i32)>,
}

impl Accumulator {
    fn new(input: &TextEditingValue) -> Accumulator {
        let units: Vec<u16> = input.text.encode_utf16().collect();
        Accumulator {
            // Upstream keeps a selection whenever it is valid, and a
            // composing region only when it is valid *and not collapsed* --
            // a collapsed composing region is upstream's "nothing is being
            // composed" and must not be dragged along by the arithmetic.
            selection: (input.selection_base >= 0 && input.selection_extent >= 0)
                .then_some((input.selection_base, input.selection_extent)),
            composing: (input.composing_base >= 0
                && input.composing_extent >= 0
                && input.composing_base != input.composing_extent)
                .then_some((input.composing_base, input.composing_extent)),
            input: input.clone(),
            units,
            output: Vec::new(),
        }
    }

    fn finalize(self) -> TextEditingValue {
        let (composing_base, composing_extent) = match self.composing {
            Some((base, extent)) if base != extent => (base, extent),
            _ => (-1, -1),
        };
        let (selection_base, selection_extent) = self.selection.unwrap_or((-1, -1));
        TextEditingValue {
            text: String::from_utf16_lossy(&self.output),
            selection_base,
            selection_extent,
            composing_base,
            composing_extent,
        }
    }
}

/// Upstream `FilteringTextInputFormatter`.
pub struct FilteringTextInputFormatter {
    pub filter_pattern: TextPattern,
    /// Whether the pattern says what is allowed or what is not.
    pub allow: bool,
    /// What a banned run becomes. Empty -- upstream's default -- deletes it.
    pub replacement_string: String,
}

impl FilteringTextInputFormatter {
    pub fn new(
        filter_pattern: TextPattern,
        allow: bool,
        replacement_string: impl Into<String>,
    ) -> FilteringTextInputFormatter {
        FilteringTextInputFormatter {
            filter_pattern,
            allow,
            replacement_string: replacement_string.into(),
        }
    }

    /// Upstream `FilteringTextInputFormatter.allow`.
    pub fn allow(filter_pattern: TextPattern) -> FilteringTextInputFormatter {
        FilteringTextInputFormatter::new(filter_pattern, true, "")
    }

    /// Upstream `FilteringTextInputFormatter.deny`.
    pub fn deny(filter_pattern: TextPattern) -> FilteringTextInputFormatter {
        FilteringTextInputFormatter::new(filter_pattern, false, "")
    }

    /// Upstream `FilteringTextInputFormatter.digitsOnly`, which is
    /// `RegExp(r'[0-9]')` allowed.
    pub fn digits_only() -> FilteringTextInputFormatter {
        FilteringTextInputFormatter::allow(TextPattern::chars(|character| {
            character.is_ascii_digit()
        }))
    }

    /// Upstream `FilteringTextInputFormatter.singleLineFormatter`, which is
    /// the literal newline denied.
    pub fn single_line() -> FilteringTextInputFormatter {
        FilteringTextInputFormatter::deny(TextPattern::Literal("\n".to_string()))
    }

    /// Upstream's `_processRegion`: one stretch of the input, either kept as
    /// it is or swapped for the replacement.
    fn process_region(&self, banned: bool, start: usize, end: usize, state: &mut Accumulator) {
        let replacement: Vec<u16> = if banned {
            // Upstream: an empty banned region contributes nothing, not the
            // replacement string -- otherwise a pattern that matches between
            // characters would insert one at every gap.
            if start == end {
                Vec::new()
            } else {
                self.replacement_string.encode_utf16().collect()
            }
        } else {
            state.units[start..end].to_vec()
        };
        state.output.extend_from_slice(&replacement);

        if replacement.len() == end - start {
            // Nothing moved, so no index has to.
            return;
        }

        let adjust = |original: i32| -> i32 {
            let original = original as isize;
            let (start, end) = (start as isize, end as isize);
            // What the replacement added, for an index that is past where it
            // went in.
            let added = if original <= start && original < end {
                0
            } else {
                replacement.len() as isize
            };
            // What the region removed, for the part of it before this index.
            let removed = original.clamp(start, end) - start;
            (added - removed) as i32
        };

        if let Some(selection) = &mut state.selection {
            selection.0 += adjust(state.input.selection_base);
            selection.1 += adjust(state.input.selection_extent);
        }
        if let Some(composing) = &mut state.composing {
            composing.0 += adjust(state.input.composing_base);
            composing.1 += adjust(state.input.composing_extent);
        }
    }
}

impl TextInputFormatter for FilteringTextInputFormatter {
    /// Upstream `formatEditUpdate`, which does not look at the old value at
    /// all: a filter is a statement about what the field may hold, not about
    /// what changed.
    fn format_edit_update(
        &self,
        _old_value: &TextEditingValue,
        new_value: &TextEditingValue,
    ) -> TextEditingValue {
        let mut state = Accumulator::new(new_value);
        let matches = self.filter_pattern.matches(&state.units);
        let mut previous_end = 0;
        for (start, end) in matches {
            // The gap before this match, then the match itself. Which of the
            // two is the banned one is the whole of what `allow` means.
            self.process_region(self.allow, previous_end, start, &mut state);
            self.process_region(!self.allow, start, end, &mut state);
            previous_end = end;
        }
        let length = state.units.len();
        self.process_region(self.allow, previous_end, length, &mut state);
        state.finalize()
    }
}

/// Upstream `LengthLimitingTextInputFormatter`.
pub struct LengthLimitingTextInputFormatter {
    /// Upstream's nullable `maxLength`. `None` and `Some(-1)` both mean no
    /// limit; upstream keeps the two apart in its API and treats them the
    /// same here, so both are accepted.
    pub max_length: Option<i32>,
    pub max_length_enforcement: Option<MaxLengthEnforcement>,
}

impl LengthLimitingTextInputFormatter {
    pub fn new(max_length: Option<i32>) -> LengthLimitingTextInputFormatter {
        debug_assert!(
            max_length.is_none_or(|length| length == -1 || length > 0),
            "a maximum length is a positive number, or -1 for none"
        );
        LengthLimitingTextInputFormatter {
            max_length,
            max_length_enforcement: None,
        }
    }

    pub fn with_enforcement(
        mut self,
        enforcement: MaxLengthEnforcement,
    ) -> LengthLimitingTextInputFormatter {
        self.max_length_enforcement = Some(enforcement);
        self
    }

    /// Upstream `getDefaultMaxLengthEnforcement`; see the module's note on
    /// why this is one answer rather than a table.
    pub fn default_max_length_enforcement() -> MaxLengthEnforcement {
        MaxLengthEnforcement::Enforced
    }

    /// How many characters a string is, which is what the limit counts.
    fn character_count(text: &str) -> usize {
        text.chars().count()
    }

    /// Upstream `truncate`: the value cut to `max_length` characters, with
    /// the selection and the composing region pulled back to fit.
    pub fn truncate(value: &TextEditingValue, max_length: i32) -> TextEditingValue {
        let max_length = max_length.max(0) as usize;
        let truncated: String = value.text.chars().take(max_length).collect();
        let length = truncated.encode_utf16().count() as i32;
        let composing_valid = value.composing_base >= 0
            && value.composing_extent >= 0
            && value.composing_base != value.composing_extent;
        // Upstream drops the composing region entirely when the truncation
        // reached into it: half a composition is not a composition.
        let (composing_base, composing_extent) = if composing_valid && length > value.composing_base
        {
            (value.composing_base, value.composing_extent.min(length))
        } else {
            (-1, -1)
        };
        TextEditingValue {
            text: truncated,
            selection_base: value.selection_base.min(length),
            selection_extent: value.selection_extent.min(length),
            composing_base,
            composing_extent,
        }
    }
}

impl TextInputFormatter for LengthLimitingTextInputFormatter {
    fn format_edit_update(
        &self,
        old_value: &TextEditingValue,
        new_value: &TextEditingValue,
    ) -> TextEditingValue {
        let Some(max_length) = self.max_length else {
            return new_value.clone();
        };
        if max_length == -1
            || LengthLimitingTextInputFormatter::character_count(&new_value.text)
                <= max_length as usize
        {
            return new_value.clone();
        }
        let at_limit = LengthLimitingTextInputFormatter::character_count(&old_value.text)
            == max_length as usize;
        let enforcement = self
            .max_length_enforcement
            .unwrap_or_else(LengthLimitingTextInputFormatter::default_max_length_enforcement);
        match enforcement {
            MaxLengthEnforcement::None => new_value.clone(),
            MaxLengthEnforcement::Enforced => {
                // Already full and nothing selected: the keystroke is refused
                // outright rather than truncated, so that the caret does not
                // jump to the end of the field.
                if at_limit && !new_value_has_selection(old_value) {
                    return old_value.clone();
                }
                LengthLimitingTextInputFormatter::truncate(new_value, max_length)
            }
            MaxLengthEnforcement::TruncateAfterCompositionEnds => {
                if at_limit && !is_composing(old_value) {
                    return old_value.clone();
                }
                // A composition in flight is let over the limit until it
                // ends, which is what this enforcement is for.
                if is_composing(new_value) {
                    return new_value.clone();
                }
                LengthLimitingTextInputFormatter::truncate(new_value, max_length)
            }
        }
    }
}

/// Upstream's `selection.isCollapsed`, negated -- whether anything is
/// selected as opposed to the caret merely sitting somewhere.
fn new_value_has_selection(value: &TextEditingValue) -> bool {
    value.selection_base != value.selection_extent
}

/// Upstream's `composing.isValid`.
fn is_composing(value: &TextEditingValue) -> bool {
    value.composing_base >= 0 && value.composing_extent >= 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str, caret: i32) -> TextEditingValue {
        TextEditingValue {
            text: text.to_string(),
            selection_base: caret,
            selection_extent: caret,
            composing_base: -1,
            composing_extent: -1,
        }
    }

    fn composing(text: &str, caret: i32, base: i32, extent: i32) -> TextEditingValue {
        TextEditingValue {
            text: text.to_string(),
            selection_base: caret,
            selection_extent: caret,
            composing_base: base,
            composing_extent: extent,
        }
    }

    #[test]
    fn a_digits_only_field_drops_everything_else() {
        let formatter = FilteringTextInputFormatter::digits_only();
        let formatted = formatter.format_edit_update(&value("", 0), &value("a1b2c3", 6));
        assert_eq!(formatted.text, "123");
    }

    #[test]
    fn the_caret_moves_with_the_text_under_it() {
        // The whole reason the accumulator exists. A filter that rewrote the
        // text and left the caret where it was would put it somewhere else on
        // every keystroke -- typing "1a2" with the caret at the end should
        // leave the caret after "12", not off the end of it.
        let formatter = FilteringTextInputFormatter::digits_only();
        let formatted = formatter.format_edit_update(&value("1a", 2), &value("1a2", 3));
        assert_eq!(formatted.text, "12");
        assert_eq!(formatted.selection_base, 2);
        assert_eq!(formatted.selection_extent, 2);
    }

    #[test]
    fn a_caret_before_the_dropped_run_does_not_move() {
        // The other half of `adjustIndex`: an index at or before the start of
        // a region that shrank is not affected by it. Moving it would drag
        // the caret backwards every time a character was refused further
        // along the line.
        let formatter = FilteringTextInputFormatter::digits_only();
        let formatted = formatter.format_edit_update(&value("", 0), &value("12ab", 1));
        assert_eq!(formatted.text, "12");
        assert_eq!(formatted.selection_base, 1);
    }

    #[test]
    fn denying_is_allowing_turned_inside_out() {
        // The same pattern, the opposite answer: `allow` decides which of the
        // match and the gap is the banned region, and nothing else changes.
        let letters = || TextPattern::chars(|character| character.is_ascii_alphabetic());
        let allowed = FilteringTextInputFormatter::allow(letters())
            .format_edit_update(&value("", 0), &value("a1b2", 4));
        let denied = FilteringTextInputFormatter::deny(letters())
            .format_edit_update(&value("", 0), &value("a1b2", 4));
        assert_eq!(allowed.text, "ab");
        assert_eq!(denied.text, "12");
    }

    #[test]
    fn a_replacement_string_stands_in_for_each_banned_run() {
        // A run, not a character: three refused characters in a row become
        // one replacement, which is what makes `***` out of a password and
        // not `*********`.
        let formatter = FilteringTextInputFormatter::new(
            TextPattern::chars(|character| character.is_ascii_digit()),
            false,
            "#",
        );
        let formatted = formatter.format_edit_update(&value("", 0), &value("a123b", 5));
        assert_eq!(formatted.text, "a###b", "each digit is its own match");

        // A literal pattern matches the whole run at once, so one replacement
        // stands for all of it.
        let literal =
            FilteringTextInputFormatter::new(TextPattern::Literal("123".to_string()), false, "#");
        assert_eq!(
            literal
                .format_edit_update(&value("", 0), &value("a123b", 5))
                .text,
            "a#b"
        );
    }

    #[test]
    fn a_single_line_field_loses_its_newlines() {
        let formatter = FilteringTextInputFormatter::single_line();
        let formatted = formatter.format_edit_update(&value("", 0), &value("one\ntwo", 7));
        assert_eq!(formatted.text, "onetwo");
        assert_eq!(formatted.selection_base, 6);
    }

    #[test]
    fn a_collapsed_composing_region_is_dropped_rather_than_carried() {
        // Upstream keeps the composing region only when it is valid *and* not
        // collapsed. A collapsed one means nothing is being composed, and
        // dragging it through the arithmetic would end with a composing
        // region over text nobody is composing.
        let formatter = FilteringTextInputFormatter::digits_only();
        let formatted = formatter.format_edit_update(&value("", 0), &composing("1a2", 3, 2, 2));
        assert_eq!(formatted.text, "12");
        assert_eq!(formatted.composing_base, -1);
        assert_eq!(formatted.composing_extent, -1);
    }

    #[test]
    fn a_field_under_the_limit_is_left_alone() {
        let formatter = LengthLimitingTextInputFormatter::new(Some(5));
        let formatted = formatter.format_edit_update(&value("abc", 3), &value("abcd", 4));
        assert_eq!(formatted.text, "abcd");
    }

    #[test]
    fn no_limit_means_none_of_the_machinery_runs() {
        // Both `None` and `-1` are upstream's "no limit", and they have to
        // behave alike: an API that accepts -1 and then enforces it truncates
        // every field to nothing.
        for max in [None, Some(-1)] {
            let formatter = LengthLimitingTextInputFormatter::new(max);
            assert_eq!(
                formatter
                    .format_edit_update(&value("", 0), &value("a hundred characters", 20))
                    .text,
                "a hundred characters"
            );
        }
    }

    #[test]
    fn typing_into_a_full_field_is_refused_rather_than_truncated() {
        // The distinction that matters to the reader: truncating the new
        // value would move the caret to the end of the field on every
        // refused keystroke. Keeping the old value leaves everything where it
        // was.
        let formatter = LengthLimitingTextInputFormatter::new(Some(3));
        let old = value("abc", 1);
        let formatted = formatter.format_edit_update(&old, &value("abXc", 2));
        assert_eq!(formatted.text, "abc");
        assert_eq!(formatted.selection_base, 1, "the caret did not jump");
    }

    #[test]
    fn a_paste_into_a_field_with_a_selection_truncates_instead() {
        // With something selected the edit is a replacement rather than an
        // insertion, so upstream truncates rather than refusing -- the reader
        // asked to replace, and refusing would leave the selection intact and
        // look like nothing happened.
        let formatter = LengthLimitingTextInputFormatter::new(Some(3));
        let old = TextEditingValue {
            text: "abc".to_string(),
            selection_base: 0,
            selection_extent: 3,
            composing_base: -1,
            composing_extent: -1,
        };
        let formatted = formatter.format_edit_update(&old, &value("wxyz", 4));
        assert_eq!(formatted.text, "wxy");
        assert_eq!(formatted.selection_base, 3, "pulled back to fit");
    }

    #[test]
    fn a_composition_is_let_over_the_limit_until_it_ends() {
        // Half a Japanese word is not a word. Cutting a composition off
        // mid-way loses the whole of it rather than the last character, which
        // is why this enforcement exists at all.
        let formatter = LengthLimitingTextInputFormatter::new(Some(3))
            .with_enforcement(MaxLengthEnforcement::TruncateAfterCompositionEnds);
        let over = composing("abcde", 5, 0, 5);
        assert_eq!(
            formatter.format_edit_update(&value("ab", 2), &over).text,
            "abcde"
        );
        // Once the composition ends, the limit applies.
        assert_eq!(
            formatter
                .format_edit_update(&value("ab", 2), &value("abcde", 5))
                .text,
            "abc"
        );
    }

    #[test]
    fn none_lets_the_field_go_over_and_says_nothing() {
        let formatter = LengthLimitingTextInputFormatter::new(Some(3))
            .with_enforcement(MaxLengthEnforcement::None);
        assert_eq!(
            formatter
                .format_edit_update(&value("abc", 3), &value("abcdef", 6))
                .text,
            "abcdef"
        );
    }

    #[test]
    fn the_limit_counts_characters_and_not_bytes() {
        // Three emoji are twelve bytes and six code units; a limit of three
        // has to mean three of them. Counting either of the other two makes
        // the field refuse its first character.
        let formatter = LengthLimitingTextInputFormatter::new(Some(3));
        let three = "😀😀😀";
        assert_eq!(three.len(), 12);
        assert_eq!(three.encode_utf16().count(), 6);
        assert_eq!(
            formatter
                .format_edit_update(&value("", 0), &value(three, 6))
                .text,
            three
        );
        // And a fourth is over.
        let truncated = LengthLimitingTextInputFormatter::truncate(&value("😀😀😀😀", 8), 3);
        assert_eq!(truncated.text, three);
        assert_eq!(
            truncated.selection_base, 6,
            "the caret comes back to the end of what is left, in code units"
        );
    }

    #[test]
    fn truncating_into_a_composition_drops_it_entirely() {
        // Upstream's rule: the composing region survives only when the cut
        // fell past its start. A region whose start was cut away points at
        // text that is no longer there.
        let cut_inside =
            LengthLimitingTextInputFormatter::truncate(&composing("abcde", 5, 1, 5), 3);
        assert_eq!(cut_inside.text, "abc");
        assert_eq!(cut_inside.composing_extent, 3, "pulled back to the cut");
        let cut_before =
            LengthLimitingTextInputFormatter::truncate(&composing("abcde", 5, 4, 5), 3);
        assert_eq!(cut_before.composing_base, -1);
        assert_eq!(cut_before.composing_extent, -1);
    }
}
