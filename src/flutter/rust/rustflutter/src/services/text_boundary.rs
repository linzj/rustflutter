// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Where one unit of text stops and the next begins (upstream
//! `services/text_boundary.dart`).
//!
//! Ctrl-left, shift-down, double-click, a screen reader reading a paragraph
//! at a time -- all of them are the same question asked with a different
//! unit, and this is the interface that asks it. A boundary answers two
//! things about a position: where the unit containing it starts, and where it
//! ends. Everything else -- the range, moving to the next unit -- follows
//! from those two.
//!
//! # Offsets are bytes
//!
//! Upstream counts in UTF-16 code units, because Dart strings are UTF-16.
//! These count in UTF-8 bytes, because Rust strings are, and because that is
//! what the rest of this crate uses inside -- the platform channel converts
//! at the edge (`utf16_to_byte` in [`text_input`](crate::services::text_input)).
//! The two agree for ASCII and disagree everywhere else, and the boundaries
//! are the wrong place to keep a second convention.
//!
//! # Recorded divergences
//!
//! * [`CharacterBoundary`] walks Unicode scalar values, not extended grapheme
//!   clusters. Upstream uses the `characters` package, and there is no
//!   grapheme segmenter in this crate or in `std`; what that costs is a
//!   combining mark or an emoji joined with a zero-width joiner, which this
//!   splits and upstream does not. The unit is right for everything that is
//!   one code point, which is what a caret usually meets.
//! * Upstream's `UntilPredicate` and the `TextBoundary.moveByOffset` helpers
//!   built on it belong with the text-editing actions, which are not ported.

use crate::services::text_input::TextEditingValue;

/// A run of text, as upstream's `TextRange`: `-1` for either end means there
/// is no boundary that way.
///
/// Upstream's is in `dart:ui` and carries the same convention; it is declared
/// here because the boundaries are the first thing in this crate that needs
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRange {
    pub start: isize,
    pub end: isize,
}

impl TextRange {
    pub const EMPTY: TextRange = TextRange { start: -1, end: -1 };

    pub const fn new(start: isize, end: isize) -> TextRange {
        TextRange { start, end }
    }

    /// Upstream `TextRange.collapsed`.
    pub const fn collapsed(offset: isize) -> TextRange {
        TextRange {
            start: offset,
            end: offset,
        }
    }

    /// Upstream `isValid`: neither end is the "no boundary" marker.
    pub fn is_valid(&self) -> bool {
        self.start >= 0 && self.end >= 0
    }

    pub fn is_collapsed(&self) -> bool {
        self.start == self.end
    }

    /// The text this range covers, or nothing if it is not a valid range of
    /// `text`.
    pub fn text_inside<'a>(&self, text: &'a str) -> Option<&'a str> {
        if !self.is_valid() || self.start > self.end {
            return None;
        }
        let (start, end) = (self.start as usize, self.end as usize);
        if end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return None;
        }
        Some(&text[start..end])
    }
}

/// Upstream `TextBoundary`.
///
/// The two halves are what an implementation overrides, and either half is
/// enough: an implementation that knows the whole unit at once overrides
/// [`text_boundary_at`](TextBoundary::text_boundary_at) and gets the two ends
/// from it, and one that walks outwards from a position overrides the two
/// ends and gets the range from them. Upstream's defaults are mutually
/// recursive for exactly that reason, and so are these -- which is also why
/// an implementation that overrides none of the three would loop for ever.
pub trait TextBoundary {
    /// Upstream `getLeadingTextBoundaryAt`.
    fn leading_boundary_at(&self, position: isize) -> Option<isize> {
        if position < 0 {
            return None;
        }
        let start = self.text_boundary_at(position).start;
        (start >= 0).then_some(start)
    }

    /// Upstream `getTrailingTextBoundaryAt`.
    fn trailing_boundary_at(&self, position: isize) -> Option<isize> {
        let end = self.text_boundary_at(position.max(0)).end;
        (end >= 0).then_some(end)
    }

    /// Upstream `getTextBoundaryAt`.
    fn text_boundary_at(&self, position: isize) -> TextRange {
        TextRange::new(
            self.leading_boundary_at(position).unwrap_or(-1),
            self.trailing_boundary_at(position).unwrap_or(-1),
        )
    }
}

/// Upstream `CharacterBoundary`: one character either side.
///
/// See the module's note on grapheme clusters: this walks Unicode scalar
/// values.
pub struct CharacterBoundary<'a> {
    text: &'a str,
}

impl<'a> CharacterBoundary<'a> {
    pub fn new(text: &'a str) -> CharacterBoundary<'a> {
        CharacterBoundary { text }
    }

    /// The start of the character `position` falls in -- itself when it is
    /// already on a boundary.
    fn floor(&self, position: usize) -> usize {
        let mut index = position.min(self.text.len());
        while index > 0 && !self.text.is_char_boundary(index) {
            index -= 1;
        }
        index
    }
}

impl TextBoundary for CharacterBoundary<'_> {
    fn leading_boundary_at(&self, position: isize) -> Option<isize> {
        if position < 0 {
            return None;
        }
        Some(self.floor(position as usize) as isize)
    }

    fn trailing_boundary_at(&self, position: isize) -> Option<isize> {
        if position >= self.text.len() as isize {
            return None;
        }
        // Upstream asks for the range at `position + 1` and takes its end,
        // which is the first boundary at or after that -- so a position
        // already on a boundary moves on by one character, and one inside a
        // character moves to the end of that character. A negative position
        // clamps to zero and therefore answers zero, not one: there is no
        // character before the start to step over.
        let mut next = ((position + 1).max(0) as usize).min(self.text.len());
        while next < self.text.len() && !self.text.is_char_boundary(next) {
            next += 1;
        }
        Some(next as isize)
    }

    fn text_boundary_at(&self, position: isize) -> TextRange {
        if position < 0 {
            return TextRange::new(-1, self.trailing_boundary_at(position).unwrap_or(-1));
        }
        if position >= self.text.len() as isize {
            return TextRange::new(self.leading_boundary_at(position).unwrap_or(-1), -1);
        }
        let start = self.floor(position as usize);
        let end = self.text[start..]
            .chars()
            .next()
            .map(|character| start + character.len_utf8())
            .unwrap_or(self.text.len());
        TextRange::new(start as isize, end as isize)
    }
}

/// Upstream `LineBoundary`: the visual line, wrapping included.
///
/// Upstream asks a `TextLayoutMetrics` for the line at an offset, because
/// only the layout knows where a soft wrap fell. Here the lines are handed in
/// as the ranges the layout produced, which is the same information and the
/// only part of `TextLayoutMetrics` a boundary uses.
pub struct LineBoundary<'a> {
    lines: &'a [(usize, usize)],
}

impl<'a> LineBoundary<'a> {
    pub fn new(lines: &'a [(usize, usize)]) -> LineBoundary<'a> {
        LineBoundary { lines }
    }
}

impl TextBoundary for LineBoundary<'_> {
    fn text_boundary_at(&self, position: isize) -> TextRange {
        let position = position.max(0) as usize;
        // The line that contains the position, or -- for a position at the
        // very end of the text -- the last one, which is where upstream's
        // layout puts a caret past the final character.
        let line = self
            .lines
            .iter()
            .find(|(start, end)| position >= *start && position <= *end)
            .or_else(|| self.lines.last());
        match line {
            Some((start, end)) => TextRange::new(*start as isize, *end as isize),
            None => TextRange::EMPTY,
        }
    }
}

/// Upstream `ParagraphBoundary`: up to the line terminators either side.
///
/// A paragraph here is a hard-wrapped one -- what the reader typed a return
/// for -- and not a visual line, which is why it reads the text rather than
/// the layout.
pub struct ParagraphBoundary<'a> {
    text: &'a str,
}

impl<'a> ParagraphBoundary<'a> {
    pub fn new(text: &'a str) -> ParagraphBoundary<'a> {
        ParagraphBoundary { text }
    }
}

/// Upstream `TextLayoutMetrics.isLineTerminator`, which is every character
/// Unicode says ends a line and not only `\n`.
pub fn is_line_terminator(character: char) -> bool {
    matches!(
        character,
        '\u{000A}'   // line feed
            | '\u{000B}' // vertical tab
            | '\u{000C}' // form feed
            | '\u{000D}' // carriage return
            | '\u{0085}' // next line
            | '\u{2028}' // line separator
            | '\u{2029}' // paragraph separator
    )
}

impl ParagraphBoundary<'_> {
    /// The character starting at `index`, if there is one.
    fn char_at(&self, index: usize) -> Option<char> {
        self.text.get(index..).and_then(|rest| rest.chars().next())
    }

    /// The character ending at `index`, and where it starts.
    fn char_before(&self, index: usize) -> Option<(usize, char)> {
        self.text[..index]
            .char_indices()
            .next_back()
            .map(|(start, character)| (start, character))
    }
}

impl TextBoundary for ParagraphBoundary<'_> {
    fn leading_boundary_at(&self, position: isize) -> Option<isize> {
        if position < 0 || self.text.is_empty() {
            return None;
        }
        let length = self.text.len() as isize;
        if position >= length {
            return Some(length);
        }
        if position == 0 {
            return Some(0);
        }
        let mut index = position as usize;
        // A position sitting on a terminator belongs to the paragraph that
        // ends there, not to the one starting after it -- so the walk begins
        // at the character *before* the terminator. Upstream's two steps back
        // for a CRLF are that same rule: the character before the pair,
        // because the pair is one terminator and not two.
        if let Some(character) = self.char_at(index) {
            if is_line_terminator(character) {
                let terminator_start = match (character, self.char_before(index)) {
                    ('\u{000A}', Some((carriage_return, '\u{000D}'))) => carriage_return,
                    _ => index,
                };
                index = self
                    .char_before(terminator_start)
                    .map_or(0, |(start, _)| start);
            }
        }
        while index > 0 {
            if self.char_at(index).is_some_and(is_line_terminator) {
                return Some((index + self.char_at(index).unwrap().len_utf8()) as isize);
            }
            index = match self.char_before(index) {
                Some((start, _)) => start,
                None => break,
            };
        }
        Some(0)
    }

    fn trailing_boundary_at(&self, position: isize) -> Option<isize> {
        if position >= self.text.len() as isize || self.text.is_empty() {
            return None;
        }
        if position < 0 {
            return Some(0);
        }
        let mut index = position as usize;
        while !self.char_at(index).is_some_and(is_line_terminator) {
            index += match self.char_at(index) {
                Some(character) => character.len_utf8(),
                None => return Some(self.text.len() as isize),
            };
            if index >= self.text.len() {
                return Some(self.text.len() as isize);
            }
        }
        // The terminator belongs to the paragraph it ends, and a CRLF is one
        // terminator rather than two.
        let terminator = self.char_at(index).unwrap();
        let after = index + terminator.len_utf8();
        if terminator == '\u{000D}' && self.char_at(after) == Some('\u{000A}') {
            return Some((after + 1) as isize);
        }
        Some(after as isize)
    }
}

/// Upstream `DocumentBoundary`: the whole of it.
pub struct DocumentBoundary<'a> {
    text: &'a str,
}

impl<'a> DocumentBoundary<'a> {
    pub fn new(text: &'a str) -> DocumentBoundary<'a> {
        DocumentBoundary { text }
    }

    /// The document a field holds, which is the case this is actually used
    /// for: a select-all, or a ctrl-home.
    pub fn of(value: &'a TextEditingValue) -> DocumentBoundary<'a> {
        DocumentBoundary::new(&value.text)
    }
}

impl TextBoundary for DocumentBoundary<'_> {
    fn leading_boundary_at(&self, position: isize) -> Option<isize> {
        (position >= 0).then_some(0)
    }

    fn trailing_boundary_at(&self, position: isize) -> Option<isize> {
        (position < self.text.len() as isize).then_some(self.text.len() as isize)
    }
}

/// Upstream `TextLayoutMetrics`: what a laid-out paragraph can be asked about
/// its own shape.
///
/// [`LineBoundary`] is the one thing here that needs it, and it takes the
/// line ranges directly -- so what is left of upstream's interface is the two
/// static predicates every caller of it actually uses, which are about
/// characters and not about layout at all.
pub struct TextLayoutMetrics;

impl TextLayoutMetrics {
    /// Upstream `TextLayoutMetrics.isWhitespace`.
    ///
    /// Upstream's own comment says this is standing in for ICU information it
    /// does not expose yet, and lists the sixteen code points by hand. The
    /// list is upstream's, not Rust's `char::is_whitespace`: the two differ
    /// -- upstream counts the four ASCII separators `0x1C`-`0x1F`, which Rust
    /// does not, and Rust counts `0x0085` and `0x2028`-`0x2029`, which
    /// upstream leaves out because they are line terminators and it handles
    /// those separately.
    pub fn is_whitespace(character: char) -> bool {
        matches!(
            character,
            '\u{0009}' // horizontal tab
                | '\u{000A}' // line feed
                | '\u{000B}' // vertical tab
                | '\u{000C}' // form feed
                | '\u{000D}' // carriage return
                | '\u{001C}' // file separator
                | '\u{001D}' // group separator
                | '\u{001E}' // record separator
                | '\u{001F}' // unit separator
                | '\u{0020}' // space
                | '\u{00A0}' // no-break space
                | '\u{1680}' // ogham space mark
                | '\u{2000}'
                ..='\u{200A}' // en quad through hair space
                | '\u{202F}' // narrow no-break space
                | '\u{205F}' // medium mathematical space
                | '\u{3000}' // ideographic space
        )
    }

    /// Upstream `TextLayoutMetrics.isLineTerminator`, which is
    /// [`is_line_terminator`] -- the same function, reachable from the name
    /// upstream puts it under.
    pub fn is_line_terminator(character: char) -> bool {
        is_line_terminator(character)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstreams_whitespace_list_is_not_rusts() {
        // Upstream's own comment says the list stands in for ICU information
        // it does not expose, and it is hand-written -- so it is not the same
        // set `char::is_whitespace` gives, and taking Rust's would change
        // where a ctrl-left stops.
        //
        // Upstream counts the four ASCII separators; Rust does not.
        for separator in ['\u{001C}', '\u{001D}', '\u{001E}', '\u{001F}'] {
            assert!(TextLayoutMetrics::is_whitespace(separator));
            assert!(!separator.is_whitespace(), "{separator:?}");
        }
        // Rust counts the next line and the two separators; upstream leaves
        // them out, because it treats them as line terminators instead.
        for terminator in ['\u{0085}', '\u{2028}', '\u{2029}'] {
            assert!(!TextLayoutMetrics::is_whitespace(terminator));
            assert!(terminator.is_whitespace(), "{terminator:?}");
            assert!(TextLayoutMetrics::is_line_terminator(terminator));
        }
    }

    #[test]
    fn the_ordinary_spaces_are_all_whitespace() {
        for space in [
            ' ', '\t', '\n', '\u{000B}', '\u{000C}', '\r', '\u{00A0}', '\u{1680}', '\u{2000}',
            '\u{2005}', '\u{200A}', '\u{202F}', '\u{205F}', '\u{3000}',
        ] {
            assert!(TextLayoutMetrics::is_whitespace(space), "{space:?}");
        }
        assert!(!TextLayoutMetrics::is_whitespace('a'));
        // Zero-width space is not whitespace to upstream: it is a line-break
        // opportunity, not a gap, and treating it as one would let a word
        // selection stop in the middle of a word.
        assert!(!TextLayoutMetrics::is_whitespace('\u{200B}'));
    }

    #[test]
    fn a_character_boundary_walks_whole_code_points_not_bytes() {
        // The offsets are bytes, so a two-byte character is two apart -- but
        // the boundary never stops inside one. A caret that landed mid-byte
        // would slice a character in half on the next backspace.
        let text = "aé中";
        let boundary = CharacterBoundary::new(text);
        assert_eq!(boundary.text_boundary_at(0), TextRange::new(0, 1));
        // Byte 1 starts "é", which is two bytes.
        assert_eq!(boundary.text_boundary_at(1), TextRange::new(1, 3));
        // Byte 2 is *inside* "é": the leading boundary walks back to 1.
        assert_eq!(boundary.text_boundary_at(2), TextRange::new(1, 3));
        // "中" is three.
        assert_eq!(boundary.text_boundary_at(3), TextRange::new(3, 6));
    }

    #[test]
    fn a_character_boundary_says_nothing_past_either_end() {
        // `-1` is upstream's "there is no boundary that way", and it is what
        // stops a caret walking off the end of the text.
        let boundary = CharacterBoundary::new("ab");
        assert_eq!(boundary.leading_boundary_at(-1), None);
        assert_eq!(boundary.trailing_boundary_at(2), None);
        assert_eq!(boundary.text_boundary_at(2), TextRange::new(2, -1));
        assert_eq!(boundary.text_boundary_at(-1), TextRange::new(-1, 0));
        assert!(!boundary.text_boundary_at(2).is_valid());
    }

    #[test]
    fn the_trailing_character_boundary_always_moves_on() {
        // Upstream asks for the range at `position + 1`, so a position
        // already sitting on a boundary still advances. Returning the
        // position itself would make a ctrl-right that never moves.
        let boundary = CharacterBoundary::new("abc");
        assert_eq!(boundary.trailing_boundary_at(0), Some(1));
        assert_eq!(boundary.trailing_boundary_at(1), Some(2));
    }

    #[test]
    fn a_paragraph_runs_to_the_terminator_and_the_terminator_belongs_to_it() {
        //            0123 45678
        let text = "one\ntwo\n";
        let boundary = ParagraphBoundary::new(text);
        // From inside the first paragraph: 0 to just past its newline.
        assert_eq!(boundary.text_boundary_at(1), TextRange::new(0, 4));
        assert_eq!(
            TextRange::new(0, 4).text_inside(text),
            Some("one\n"),
            "the terminator is part of the paragraph it ends"
        );
        // From inside the second.
        assert_eq!(boundary.text_boundary_at(5), TextRange::new(4, 8));
    }

    #[test]
    fn a_crlf_is_one_terminator_and_not_two() {
        // Two paragraphs, not three with an empty one between. A boundary
        // that treats CR and LF separately puts a phantom empty paragraph in
        // every file that came from Windows.
        //            0123  4  56789
        let text = "one\r\ntwo";
        let boundary = ParagraphBoundary::new(text);
        assert_eq!(boundary.trailing_boundary_at(0), Some(5));
        assert_eq!(
            TextRange::new(0, 5).text_inside(text),
            Some("one\r\n"),
            "both halves of the CRLF belong to the paragraph they end"
        );
        assert_eq!(boundary.leading_boundary_at(6), Some(5));
        // A position on the LF of a CRLF belongs to the paragraph before it,
        // and stepping back has to clear both halves.
        assert_eq!(boundary.leading_boundary_at(4), Some(0));
    }

    #[test]
    fn every_unicode_line_terminator_ends_a_paragraph() {
        // Not only `\n`: upstream's `isLineTerminator` lists seven, and a
        // paragraph boundary that only knows about the newline runs straight
        // through a form feed or a paragraph separator.
        for terminator in [
            '\u{000A}', '\u{000B}', '\u{000C}', '\u{000D}', '\u{0085}', '\u{2028}', '\u{2029}',
        ] {
            assert!(is_line_terminator(terminator), "{terminator:?}");
            let text = format!("a{terminator}b");
            let boundary = ParagraphBoundary::new(&text);
            assert_eq!(
                boundary.trailing_boundary_at(0),
                Some(1 + terminator.len_utf8() as isize),
                "{terminator:?} should end the first paragraph"
            );
        }
        assert!(!is_line_terminator('\t'), "a tab is not a line terminator");
        assert!(!is_line_terminator(' '));
    }

    #[test]
    fn a_paragraph_boundary_says_nothing_about_an_empty_document() {
        let boundary = ParagraphBoundary::new("");
        assert_eq!(boundary.leading_boundary_at(0), None);
        assert_eq!(boundary.trailing_boundary_at(0), None);
    }

    #[test]
    fn a_line_boundary_is_the_visual_line_wrapping_included() {
        // The distinction the two boundaries exist to keep apart: this one
        // stops at a soft wrap and the paragraph one does not.
        let text = "one two three";
        let lines = [(0usize, 7usize), (7, 13)];
        let by_line = LineBoundary::new(&lines);
        let by_paragraph = ParagraphBoundary::new(text);
        assert_eq!(by_line.text_boundary_at(2), TextRange::new(0, 7));
        assert_eq!(
            by_paragraph.text_boundary_at(2),
            TextRange::new(0, 13),
            "a soft wrap is not a paragraph break"
        );
        // A caret past the last character is on the last line, which is where
        // the layout puts it.
        assert_eq!(by_line.text_boundary_at(13), TextRange::new(7, 13));
    }

    #[test]
    fn a_document_boundary_is_the_whole_of_it_from_anywhere_inside() {
        let text = "one\ntwo";
        let boundary = DocumentBoundary::new(text);
        assert_eq!(boundary.text_boundary_at(0), TextRange::new(0, 7));
        assert_eq!(boundary.text_boundary_at(4), TextRange::new(0, 7));
        // And nothing from outside, in the same direction as everything else.
        assert_eq!(boundary.leading_boundary_at(-1), None);
        assert_eq!(boundary.trailing_boundary_at(7), None);
    }

    #[test]
    fn a_range_refuses_to_slice_a_character_in_half() {
        // `text_inside` is the one place a bad offset would panic rather than
        // answer, so it checks instead.
        let text = "é";
        assert_eq!(TextRange::new(0, 2).text_inside(text), Some("é"));
        assert_eq!(TextRange::new(0, 1).text_inside(text), None);
        assert_eq!(TextRange::new(0, 9).text_inside(text), None);
        assert_eq!(TextRange::EMPTY.text_inside(text), None);
        assert_eq!(TextRange::new(2, 0).text_inside(text), None);
        assert!(TextRange::collapsed(1).is_collapsed());
    }
}
