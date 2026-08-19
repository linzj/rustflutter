// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What the keyboard changed, rather than what the text now is (upstream
//! `services/text_editing_delta.dart`).
//!
//! The ordinary text-input protocol sends the whole new value on every
//! keystroke, and a field that wants to know what actually happened -- a
//! formatter refusing a character, an undo stack, a spell checker -- has to
//! diff two strings and guess. A delta says it outright: this was inserted
//! here, that range was deleted, this range was replaced, or nothing about
//! the text changed and only the selection moved.
//!
//! The four are a closed set, so they are an enum with a struct per variant,
//! the way [`ShapeBorder`](crate::borders::ShapeBorder) and the slider shapes
//! are put together.
//!
//! # Offsets are UTF-16 code units
//!
//! Unlike [`text_boundary`](crate::services::text_boundary), which counts
//! bytes because it works on the crate's own strings, a delta is what the
//! platform said and is kept in the platform's units. The conversion happens
//! in [`apply`](TextEditingDelta::apply), which is where a delta stops being
//! a message and becomes a string.
//!
//! # Recorded divergences
//!
//! * Upstream's `TextSelection` carries an affinity and a directionality
//!   alongside the two offsets. [`TextEditingValue`] here carries the two
//!   offsets only, for the reason recorded on it, so a delta carries the same
//!   two and `selectionAffinity` and `selectionIsDirectional` are read off the
//!   wire and dropped.
//! * `debugFillProperties` on all four is the diagnostics tree, which is P10.

use crate::services::codec::Value;
use crate::services::text_input::{TextEditingValue, utf16_to_byte};

/// A range on the wire, in UTF-16 code units. `-1` at either end is
/// upstream's "no range".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Utf16Range {
    pub start: i32,
    pub end: i32,
}

impl Utf16Range {
    pub const NONE: Utf16Range = Utf16Range { start: -1, end: -1 };

    pub const fn new(start: i32, end: i32) -> Utf16Range {
        Utf16Range { start, end }
    }

    pub const fn collapsed(offset: i32) -> Utf16Range {
        Utf16Range {
            start: offset,
            end: offset,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.start >= 0 && self.end >= 0
    }

    /// Upstream's `_debugTextRangeIsValid`: an invalid range is fine -- it
    /// means "none" -- and a valid one has to fit the text.
    pub fn fits(&self, text: &str) -> bool {
        if !self.is_valid() {
            return true;
        }
        let length = text.encode_utf16().count() as i32;
        self.start <= length && self.end <= length
    }
}

/// Upstream's `_replace`: the range of `text` swapped for `replacement`.
///
/// Returns nothing when the range does not land on character boundaries,
/// where upstream would assert. A malformed message from the platform is the
/// one way this happens, and refusing the delta is better than a panic in the
/// middle of typing.
fn replace(text: &str, replacement: &str, range: Utf16Range) -> Option<String> {
    let start = utf16_to_byte(text, range.start.min(range.end))?;
    let end = utf16_to_byte(text, range.start.max(range.end))?;
    let mut result = String::with_capacity(text.len() + replacement.len());
    result.push_str(&text[..start]);
    result.push_str(replacement);
    result.push_str(&text[end..]);
    Some(result)
}

/// Upstream `TextEditingDeltaInsertion`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEditingDeltaInsertion {
    pub text_inserted: String,
    pub insertion_offset: i32,
}

/// Upstream `TextEditingDeltaDeletion`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEditingDeltaDeletion {
    pub deleted_range: Utf16Range,
}

/// Upstream `TextEditingDeltaReplacement`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEditingDeltaReplacement {
    pub replacement_text: String,
    pub replaced_range: Utf16Range,
}

/// Upstream `TextEditingDeltaNonTextUpdate`: the text is what it was and only
/// the selection or the composing region moved.
///
/// It has no fields of its own -- the selection and composing that moved are
/// on the delta itself, which every variant carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TextEditingDeltaNonTextUpdate;

/// What one of the four kinds of change is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextEditingDeltaKind {
    Insertion(TextEditingDeltaInsertion),
    Deletion(TextEditingDeltaDeletion),
    Replacement(TextEditingDeltaReplacement),
    NonTextUpdate(TextEditingDeltaNonTextUpdate),
}

/// Upstream `TextEditingDelta`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEditingDelta {
    pub old_text: String,
    /// Where the selection is *after* the change, which is the platform's
    /// word on it and not something derived from the edit.
    pub selection: Utf16Range,
    pub composing: Utf16Range,
    pub kind: TextEditingDeltaKind,
}

impl TextEditingDelta {
    /// The text this delta deleted, for the deletion variant.
    pub fn text_deleted(&self) -> Option<&str> {
        let TextEditingDeltaKind::Deletion(deletion) = &self.kind else {
            return None;
        };
        let start = utf16_to_byte(&self.old_text, deletion.deleted_range.start)?;
        let end = utf16_to_byte(&self.old_text, deletion.deleted_range.end)?;
        self.old_text.get(start..end)
    }

    /// Upstream `apply`: `value` with this change made.
    ///
    /// Returns nothing where upstream would assert -- a range the text cannot
    /// hold, which only a malformed platform message produces.
    pub fn apply(&self, value: &TextEditingValue) -> Option<TextEditingValue> {
        let new_text = match &self.kind {
            TextEditingDeltaKind::Insertion(insertion) => replace(
                &self.old_text,
                &insertion.text_inserted,
                Utf16Range::collapsed(insertion.insertion_offset),
            )?,
            TextEditingDeltaKind::Deletion(deletion) => {
                replace(&self.old_text, "", deletion.deleted_range)?
            }
            TextEditingDeltaKind::Replacement(replacement) => replace(
                &self.old_text,
                &replacement.replacement_text,
                replacement.replaced_range,
            )?,
            // Upstream applies nothing and copies the selection and composing
            // across, which is the whole of what this delta is.
            TextEditingDeltaKind::NonTextUpdate(_) => self.old_text.clone(),
        };
        if !self.selection.fits(&new_text) || !self.composing.fits(&new_text) {
            return None;
        }
        Some(TextEditingValue {
            text: new_text,
            selection_base: self.selection.start,
            selection_extent: self.selection.end,
            composing_base: self.composing.start,
            composing_extent: self.composing.end,
            ..value.clone()
        })
    }

    /// Upstream `TextEditingDelta.fromJSON`: works out which of the four this
    /// message describes.
    ///
    /// The platform does not say. It sends "this destination range became
    /// this source text", and everything below is upstream's reading of what
    /// that means -- which matters most while composing, because a native IME
    /// replaces the whole composing region on every keystroke rather than
    /// reporting one character. Typing the `d` of `world` arrives as "(0,4)
    /// became `world`", and the only way to know that is an insertion is to
    /// notice that the text inside the old region did not change.
    pub fn from_json(encoded: &[(Value, Value)]) -> Option<TextEditingDelta> {
        let get = |name: &str| {
            encoded
                .iter()
                .find(|(key, _)| matches!(key, Value::String(key) if key == name))
                .map(|(_, value)| value)
        };
        let string = |name: &str| match get(name) {
            Some(Value::String(text)) => Some(text.clone()),
            _ => None,
        };
        let integer = |name: &str, fallback: i32| match get(name) {
            Some(Value::I32(number)) => *number,
            Some(Value::I64(number)) => *number as i32,
            Some(Value::F64(number)) => *number as i32,
            _ => fallback,
        };

        let old_text = string("oldText")?;
        let destination_start = integer("deltaStart", -1);
        let destination_end = integer("deltaEnd", -1);
        let source = string("deltaText").unwrap_or_default();
        let source_end = source.encode_utf16().count() as i32;

        let selection =
            Utf16Range::new(integer("selectionBase", -1), integer("selectionExtent", -1));
        let composing =
            Utf16Range::new(integer("composingBase", -1), integer("composingExtent", -1));

        // Upstream's `isNonTextUpdate`: both ends at -1 means the platform is
        // reporting a selection move and nothing else.
        if destination_start == -1 && destination_start == destination_end {
            return Some(TextEditingDelta {
                old_text,
                selection,
                composing,
                kind: TextEditingDeltaKind::NonTextUpdate(TextEditingDeltaNonTextUpdate),
            });
        }

        let destination = Utf16Range::new(destination_start, destination_end);
        let new_text = replace(&old_text, &source, destination)?;

        let destination_length = destination_end - destination_start;
        let is_deletion_greater_than_one = destination_length - source_end > 1;
        let is_deleting_by_replacing_with_empty = source.is_empty() && source_end == 0;
        let is_replaced_by_shorter =
            is_deletion_greater_than_one && source_end < destination_length;
        let is_replaced_by_longer = source_end > destination_length;
        let is_replaced_by_same = source_end == destination_length;
        let is_inserting_inside_composing = destination_start + source_end > destination_end;
        let is_deleting_inside_composing = !is_replaced_by_shorter
            && !is_deleting_by_replacing_with_empty
            && destination_start + source_end < destination_end;

        // The two runs to compare: what the old composing region held, and
        // what the same span of the replacement holds. Equal means the edit
        // only added to or took from the region rather than rewriting it.
        let (new_composing_text, original_composing_text) = if is_deleting_by_replacing_with_empty
            || is_deleting_inside_composing
            || is_replaced_by_shorter
        {
            (
                slice_utf16(&source, 0, source_end)?,
                slice_utf16(&old_text, destination_start, destination_start + source_end)?,
            )
        } else {
            (
                slice_utf16(&source, 0, destination_length)?,
                slice_utf16(&old_text, destination_start, destination_end)?,
            )
        };
        let region_text_changed = original_composing_text != new_composing_text;

        let kind = if old_text == new_text {
            // Nothing about the text changed after all.
            TextEditingDeltaKind::NonTextUpdate(TextEditingDeltaNonTextUpdate)
        } else if (is_deleting_by_replacing_with_empty || is_deleting_inside_composing)
            && !region_text_changed
        {
            // Upstream: a single-character deletion is reported as the whole
            // destination range, so the range is narrowed to the one
            // character that actually went.
            let start = if is_deletion_greater_than_one {
                destination_start
            } else {
                destination_end - 1
            };
            TextEditingDeltaKind::Deletion(TextEditingDeltaDeletion {
                deleted_range: Utf16Range::new(start, destination_end),
            })
        } else if (destination_start == destination_end || is_inserting_inside_composing)
            && !region_text_changed
        {
            TextEditingDeltaKind::Insertion(TextEditingDeltaInsertion {
                text_inserted: slice_utf16(&source, destination_length, source_end)?,
                insertion_offset: destination_end,
            })
        } else if region_text_changed
            || is_replaced_by_longer
            || is_replaced_by_shorter
            || is_replaced_by_same
        {
            TextEditingDeltaKind::Replacement(TextEditingDeltaReplacement {
                replacement_text: source,
                replaced_range: destination,
            })
        } else {
            // Upstream asserts false here and falls back to a non-text
            // update. There is no message that reaches it, and falling back
            // rather than panicking is the same choice upstream made for
            // release builds.
            TextEditingDeltaKind::NonTextUpdate(TextEditingDeltaNonTextUpdate)
        };

        Some(TextEditingDelta {
            old_text,
            selection,
            composing,
            kind,
        })
    }
}

/// `text[start..end]` with both ends in UTF-16 code units.
fn slice_utf16(text: &str, start: i32, end: i32) -> Option<String> {
    let start = utf16_to_byte(text, start)?;
    let end = utf16_to_byte(text, end)?;
    text.get(start..end).map(|slice| slice.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(pairs: &[(&str, Value)]) -> Vec<(Value, Value)> {
        pairs
            .iter()
            .map(|(key, value)| (Value::String((*key).to_string()), value.clone()))
            .collect()
    }

    /// The shape the platform sends: a destination range in the old text, and
    /// the source text that replaced it.
    fn edit(old: &str, start: i32, end: i32, source: &str, caret: i32) -> Vec<(Value, Value)> {
        message(&[
            ("oldText", Value::String(old.to_string())),
            ("deltaStart", Value::I32(start)),
            ("deltaEnd", Value::I32(end)),
            ("deltaText", Value::String(source.to_string())),
            ("selectionBase", Value::I32(caret)),
            ("selectionExtent", Value::I32(caret)),
            ("composingBase", Value::I32(-1)),
            ("composingExtent", Value::I32(-1)),
        ])
    }

    #[test]
    fn typing_at_the_caret_is_an_insertion() {
        let delta = TextEditingDelta::from_json(&edit("ab", 2, 2, "c", 3)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::Insertion(TextEditingDeltaInsertion {
                text_inserted: "c".to_string(),
                insertion_offset: 2,
            })
        );
        let applied = delta
            .apply(&TextEditingValue::new("ab"))
            .expect("it applies");
        assert_eq!(applied.text, "abc");
        assert_eq!(applied.selection_extent, 3);
    }

    #[test]
    fn a_backspace_is_a_deletion_of_exactly_one_character() {
        // The platform reports the whole destination range; upstream narrows
        // a single-character deletion to the one character that went. A port
        // that keeps the reported range deletes from the start of the word.
        let delta = TextEditingDelta::from_json(&edit("abc", 0, 3, "ab", 2)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::Deletion(TextEditingDeltaDeletion {
                deleted_range: Utf16Range::new(2, 3),
            })
        );
        assert_eq!(delta.text_deleted(), Some("c"));
        assert_eq!(
            delta.apply(&TextEditingValue::new("abc")).unwrap().text,
            "ab"
        );
    }

    #[test]
    fn losing_more_than_one_character_to_a_shorter_string_is_a_replacement() {
        // The surprising line in upstream's classification, and the one a
        // reader guesses wrong: only a *single*-character shortening reads as
        // a deletion. `abcde` becoming `ab` over the whole range drops three
        // characters, and upstream calls that a replacement -- there is no
        // one character to point a deleted range at.
        let delta = TextEditingDelta::from_json(&edit("abcde", 0, 5, "ab", 2)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::Replacement(TextEditingDeltaReplacement {
                replacement_text: "ab".to_string(),
                replaced_range: Utf16Range::new(0, 5),
            })
        );
        assert_eq!(
            delta.apply(&TextEditingValue::new("abcde")).unwrap().text,
            "ab"
        );
    }

    #[test]
    fn selecting_a_run_and_deleting_it_is_a_deletion_of_the_whole_run() {
        // Replacing with nothing is the other way to lose more than one
        // character, and that one *is* a deletion -- of the reported range,
        // not narrowed, because every character in it went.
        let delta = TextEditingDelta::from_json(&edit("abcde", 0, 5, "", 0)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::Deletion(TextEditingDeltaDeletion {
                deleted_range: Utf16Range::new(0, 5),
            })
        );
        assert_eq!(delta.text_deleted(), Some("abcde"));
        assert_eq!(
            delta.apply(&TextEditingValue::new("abcde")).unwrap().text,
            ""
        );
    }

    #[test]
    fn composing_a_character_reads_as_an_insertion_not_a_replacement() {
        // Upstream's own example. A native IME replaces the whole composing
        // region on every keystroke: typing the `d` of `world` arrives as
        // "(0,4) became world". The only way to know that is an insertion is
        // that the text inside the old region did not change -- `worl` is
        // still `worl`.
        let delta = TextEditingDelta::from_json(&edit("worl", 0, 4, "world", 5)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::Insertion(TextEditingDeltaInsertion {
                text_inserted: "d".to_string(),
                insertion_offset: 4,
            })
        );
        assert_eq!(
            delta.apply(&TextEditingValue::new("worl")).unwrap().text,
            "world"
        );
    }

    #[test]
    fn deleting_from_a_composing_region_reads_as_a_deletion() {
        // The mirror of the case above: `world` becomes `worl`, the region's
        // own text is unchanged, so it is a deletion of the one character.
        let delta = TextEditingDelta::from_json(&edit("world", 0, 5, "worl", 4)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::Deletion(TextEditingDeltaDeletion {
                deleted_range: Utf16Range::new(4, 5),
            })
        );
        assert_eq!(delta.text_deleted(), Some("d"));
    }

    #[test]
    fn a_rewritten_composing_region_is_a_replacement() {
        // Here the region's own text *did* change -- `worl` became `hell` --
        // so neither the insertion nor the deletion reading applies, and it
        // is what it looks like: a replacement.
        let delta = TextEditingDelta::from_json(&edit("worl", 0, 4, "hello", 5)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::Replacement(TextEditingDeltaReplacement {
                replacement_text: "hello".to_string(),
                replaced_range: Utf16Range::new(0, 4),
            })
        );
        assert_eq!(
            delta.apply(&TextEditingValue::new("worl")).unwrap().text,
            "hello"
        );
    }

    #[test]
    fn a_selection_move_is_a_non_text_update() {
        // Both ends at -1 is the platform saying "the text is what it was".
        let delta = TextEditingDelta::from_json(&edit("abc", -1, -1, "", 1)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::NonTextUpdate(TextEditingDeltaNonTextUpdate)
        );
        let applied = delta.apply(&TextEditingValue::new("abc")).unwrap();
        assert_eq!(applied.text, "abc");
        assert_eq!(applied.selection_base, 1);
    }

    #[test]
    fn an_edit_that_changes_nothing_is_also_a_non_text_update() {
        // A replacement whose result equals the original: upstream checks the
        // texts rather than the ranges, because the ranges say an edit
        // happened and the texts say it did not.
        let delta = TextEditingDelta::from_json(&edit("abc", 0, 3, "abc", 3)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::NonTextUpdate(TextEditingDeltaNonTextUpdate)
        );
    }

    #[test]
    fn the_offsets_are_utf16_code_units_and_not_bytes() {
        // The delta is what the platform said, in the platform's units. A
        // character outside the basic plane is two code units and four bytes,
        // and reading the offset as a byte index puts the caret inside it.
        let old = "😀";
        assert_eq!(old.len(), 4, "four bytes");
        assert_eq!(old.encode_utf16().count(), 2, "two code units");
        let delta = TextEditingDelta::from_json(&edit(old, 2, 2, "!", 3)).expect("a delta");
        assert_eq!(
            delta.kind,
            TextEditingDeltaKind::Insertion(TextEditingDeltaInsertion {
                text_inserted: "!".to_string(),
                insertion_offset: 2,
            })
        );
        assert_eq!(
            delta.apply(&TextEditingValue::new(old)).unwrap().text,
            "😀!"
        );
    }

    #[test]
    fn an_offset_inside_a_character_is_refused_rather_than_panicking() {
        // Only a malformed platform message reaches this, and upstream
        // asserts. Refusing the delta is what a release build should do
        // instead: dropping one keystroke beats a panic mid-typing.
        assert_eq!(TextEditingDelta::from_json(&edit("😀", 1, 1, "!", 2)), None);
    }

    #[test]
    fn a_message_with_no_old_text_is_not_a_delta() {
        assert_eq!(
            TextEditingDelta::from_json(&message(&[("deltaStart", Value::I32(0))])),
            None
        );
    }

    #[test]
    fn a_range_that_does_not_fit_the_new_text_refuses_to_apply() {
        let delta = TextEditingDelta {
            old_text: "ab".to_string(),
            selection: Utf16Range::new(9, 9),
            composing: Utf16Range::NONE,
            kind: TextEditingDeltaKind::NonTextUpdate(TextEditingDeltaNonTextUpdate),
        };
        assert_eq!(delta.apply(&TextEditingValue::new("ab")), None);
        // An invalid range means "none" and always fits.
        assert!(Utf16Range::NONE.fits("ab"));
        assert!(!Utf16Range::new(0, 9).fits("ab"));
    }
}
