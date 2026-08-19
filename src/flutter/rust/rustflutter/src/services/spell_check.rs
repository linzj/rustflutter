// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Asking the platform which words are misspelled (upstream
//! `services/spell_check.dart`).
//!
//! The framework does not know how to spell. Every platform ships a
//! dictionary and a checker for the languages the reader has installed, and
//! this is the channel to it: hand over a locale and a string, get back the
//! ranges that look wrong and what to offer instead.
//!
//! # Recorded divergences
//!
//! * Upstream's `fetchSpellCheckSuggestions` is a `Future`. There is no
//!   executor here, so the result arrives through a callback -- the shape
//!   every other [`MethodChannel`] call in this crate has.
//! * `SuggestionSpan.range` is upstream's `TextRange`, whose offsets are
//!   UTF-16 code units because they came off the wire. They are kept in those
//!   units, for the reason recorded on
//!   [`text_editing_delta`](crate::services::text_editing_delta).

use std::cell::RefCell;
use std::rc::Rc;

use crate::platform::Locale;
use crate::services::channel::MethodChannel;
use crate::services::codec::{StandardMethodCodec, Value};
use crate::services::text_editing_delta::Utf16Range;

/// Upstream `SuggestionSpan`: a run that looks misspelled, and what to offer
/// in its place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestionSpan {
    pub range: Utf16Range,
    pub suggestions: Vec<String>,
}

impl SuggestionSpan {
    pub fn new(range: Utf16Range, suggestions: Vec<String>) -> SuggestionSpan {
        SuggestionSpan { range, suggestions }
    }
}

/// Upstream `SpellCheckResults`: the spans, and the text they were found in.
///
/// The text is carried because a result is only about the string it was asked
/// for -- one keystroke later the offsets mean something else, and the only
/// way to know is to have kept what was asked.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SpellCheckResults {
    pub spell_checked_text: String,
    pub suggestion_spans: Vec<SuggestionSpan>,
}

impl SpellCheckResults {
    pub fn new(
        spell_checked_text: impl Into<String>,
        suggestion_spans: Vec<SuggestionSpan>,
    ) -> SpellCheckResults {
        SpellCheckResults {
            spell_checked_text: spell_checked_text.into(),
            suggestion_spans,
        }
    }
}

/// Upstream `SpellCheckService`.
pub trait SpellCheckService {
    /// Upstream `fetchSpellCheckSuggestions`. `callback` is handed nothing
    /// when the request was refused -- upstream returns null for the same
    /// case, which is a request cancelled because another was already in
    /// flight.
    fn fetch_spell_check_suggestions(
        &self,
        locale: &Locale,
        text: &str,
        callback: Box<dyn FnOnce(Option<Vec<SuggestionSpan>>)>,
    );
}

/// Upstream `DefaultSpellCheckService`: the one that asks the platform.
pub struct DefaultSpellCheckService {
    channel: MethodChannel<StandardMethodCodec>,
    /// Upstream's `lastSavedResults`, which the merge below reads.
    last_saved_results: Rc<RefCell<Option<SpellCheckResults>>>,
}

impl Default for DefaultSpellCheckService {
    fn default() -> DefaultSpellCheckService {
        DefaultSpellCheckService::new()
    }
}

impl DefaultSpellCheckService {
    /// Upstream `SystemChannels.spellCheck`.
    pub const CHANNEL: &'static str = "flutter/spellcheck";
    /// The one method the channel has.
    pub const METHOD: &'static str = "SpellCheck.initiateSpellCheck";

    pub fn new() -> DefaultSpellCheckService {
        DefaultSpellCheckService {
            channel: MethodChannel::named(DefaultSpellCheckService::CHANNEL, StandardMethodCodec),
            last_saved_results: Rc::new(RefCell::new(None)),
        }
    }

    /// What the last completed request found, which is what upstream keeps in
    /// `lastSavedResults`.
    pub fn last_saved_results(&self) -> Option<SpellCheckResults> {
        self.last_saved_results.borrow().clone()
    }

    /// Upstream `mergeResults`: two lists of spans, both sorted by where they
    /// start, walked together into one.
    ///
    /// Where the two agree on where a span starts, the *old* one is kept.
    /// That is upstream's choice and not an accident: the reader may already
    /// have been offered the old suggestions, and swapping the list under a
    /// menu that is open would change what tapping it does.
    pub fn merge_results(
        old_results: &[SuggestionSpan],
        new_results: &[SuggestionSpan],
    ) -> Vec<SuggestionSpan> {
        let mut merged = Vec::new();
        let (mut old_index, mut new_index) = (0, 0);
        while old_index < old_results.len() && new_index < new_results.len() {
            let old_span = &old_results[old_index];
            let new_span = &new_results[new_index];
            if old_span.range.start == new_span.range.start {
                merged.push(old_span.clone());
                old_index += 1;
                new_index += 1;
            } else if old_span.range.start < new_span.range.start {
                merged.push(old_span.clone());
                old_index += 1;
            } else {
                merged.push(new_span.clone());
                new_index += 1;
            }
        }
        merged.extend_from_slice(&old_results[old_index..]);
        merged.extend_from_slice(&new_results[new_index..]);
        merged
    }

    /// Upstream's decoding of one result map.
    fn span_from(entry: &Value) -> Option<SuggestionSpan> {
        let Value::Map(pairs) = entry else {
            return None;
        };
        let get = |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| matches!(key, Value::String(key) if key == name))
                .map(|(_, value)| value)
        };
        let integer = |name: &str| match get(name) {
            Some(Value::I32(number)) => Some(*number),
            Some(Value::I64(number)) => Some(*number as i32),
            _ => None,
        };
        let suggestions = match get("suggestions") {
            Some(Value::List(items)) => items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        Some(SuggestionSpan::new(
            Utf16Range::new(integer("startIndex")?, integer("endIndex")?),
            suggestions,
        ))
    }

    /// Upstream's merge step, split out so it can be checked without a
    /// platform to answer.
    ///
    /// Upstream merges when the text has not changed **and the two lists are
    /// equal**, which is written there as `spansHaveChanged =
    /// listEquals(...)`. Merging two equal lists gives the first back, so as
    /// written the merge cannot change anything -- the name says one thing
    /// and the value says the other. It is ported as upstream wrote it,
    /// because a port that quietly fixed it would answer differently from the
    /// framework it is a port of, and this is the note that says so.
    fn reconcile(
        last: Option<&SpellCheckResults>,
        text: &str,
        fresh: Vec<SuggestionSpan>,
    ) -> Vec<SuggestionSpan> {
        let Some(last) = last else {
            return fresh;
        };
        let text_has_not_changed = last.spell_checked_text == text;
        let spans_have_changed = last.suggestion_spans == fresh;
        if text_has_not_changed && spans_have_changed {
            DefaultSpellCheckService::merge_results(&last.suggestion_spans, &fresh)
        } else {
            fresh
        }
    }
}

impl SpellCheckService for DefaultSpellCheckService {
    fn fetch_spell_check_suggestions(
        &self,
        locale: &Locale,
        text: &str,
        callback: Box<dyn FnOnce(Option<Vec<SuggestionSpan>>)>,
    ) {
        let arguments = Value::List(vec![
            Value::String(locale.to_language_tag()),
            Value::String(text.to_string()),
        ]);
        let saved = Rc::clone(&self.last_saved_results);
        let text = text.to_string();
        self.channel
            .invoke_with_reply(DefaultSpellCheckService::METHOD, arguments, move |reply| {
                // Upstream catches whatever the call throws and answers null,
                // with the note that the request was cancelled because
                // another was pending. An error reply is that same case.
                let Ok(Some(Value::List(items))) = reply else {
                    callback(None);
                    return;
                };
                let fresh: Vec<SuggestionSpan> = items
                    .iter()
                    .filter_map(DefaultSpellCheckService::span_from)
                    .collect();
                let spans =
                    DefaultSpellCheckService::reconcile(saved.borrow().as_ref(), &text, fresh);
                *saved.borrow_mut() = Some(SpellCheckResults::new(text, spans.clone()));
                callback(Some(spans));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: i32, end: i32, suggestions: &[&str]) -> SuggestionSpan {
        SuggestionSpan::new(
            Utf16Range::new(start, end),
            suggestions.iter().map(|word| word.to_string()).collect(),
        )
    }

    #[test]
    fn merging_walks_both_lists_in_order_of_where_the_spans_start() {
        let old = [span(0, 3, &["the"]), span(10, 14, &["word"])];
        let new = [span(5, 8, &["and"]), span(10, 14, &["ward"])];
        let merged = DefaultSpellCheckService::merge_results(&old, &new);
        assert_eq!(
            merged.iter().map(|s| s.range.start).collect::<Vec<_>>(),
            vec![0, 5, 10]
        );
    }

    #[test]
    fn where_two_spans_start_together_the_old_suggestions_are_kept() {
        // Upstream's choice, and not an accident: the reader may already have
        // been offered the old list, and swapping it under an open menu
        // changes what tapping it does.
        let old = [span(10, 14, &["word"])];
        let new = [span(10, 14, &["ward"])];
        let merged = DefaultSpellCheckService::merge_results(&old, &new);
        assert_eq!(merged.len(), 1, "the two are one span, not two");
        assert_eq!(merged[0].suggestions, vec!["word".to_string()]);
    }

    #[test]
    fn whatever_is_left_over_on_either_side_comes_along() {
        let old = [
            span(0, 3, &["a"]),
            span(20, 24, &["b"]),
            span(30, 33, &["c"]),
        ];
        let new = [span(0, 3, &["a"])];
        let merged = DefaultSpellCheckService::merge_results(&old, &new);
        assert_eq!(
            merged.iter().map(|s| s.range.start).collect::<Vec<_>>(),
            vec![0, 20, 30]
        );
        // And the same the other way round.
        let merged = DefaultSpellCheckService::merge_results(&new, &old);
        assert_eq!(
            merged.iter().map(|s| s.range.start).collect::<Vec<_>>(),
            vec![0, 20, 30]
        );
    }

    #[test]
    fn merging_two_empty_lists_is_empty() {
        assert!(DefaultSpellCheckService::merge_results(&[], &[]).is_empty());
    }

    #[test]
    fn the_merge_only_runs_when_the_two_lists_are_already_equal() {
        // Upstream's condition reads `spansHaveChanged = listEquals(...)`:
        // the name says changed and the value says equal, and the merge runs
        // on the equal branch. Merging two equal lists gives the first back,
        // so as written the merge cannot change anything. Ported as upstream
        // wrote it, and asserted so that the day upstream fixes it, this
        // fails rather than drifting quietly.
        let last = SpellCheckResults::new("hello wrld", vec![span(6, 10, &["world"])]);
        let same = vec![span(6, 10, &["world"])];
        assert_eq!(
            DefaultSpellCheckService::reconcile(Some(&last), "hello wrld", same.clone()),
            same,
            "equal lists: merged, and the merge of equals is the same list"
        );

        // Different spans for the same text take the *fresh* list outright --
        // no merge -- which is the branch upstream's variable name suggests
        // should have been the merging one.
        let different = vec![span(0, 5, &["hello"]), span(6, 10, &["world"])];
        assert_eq!(
            DefaultSpellCheckService::reconcile(Some(&last), "hello wrld", different.clone()),
            different
        );
    }

    #[test]
    fn a_result_for_different_text_replaces_rather_than_merges() {
        // The offsets in a stale result mean something else once a keystroke
        // has landed, which is why the text is carried alongside them.
        let last = SpellCheckResults::new("hello wrld", vec![span(6, 10, &["world"])]);
        let fresh = vec![span(0, 4, &["help"])];
        assert_eq!(
            DefaultSpellCheckService::reconcile(Some(&last), "helo wrld", fresh.clone()),
            fresh
        );
        // And with nothing saved at all, the fresh list is the answer.
        assert_eq!(
            DefaultSpellCheckService::reconcile(None, "anything", fresh.clone()),
            fresh
        );
    }

    #[test]
    fn a_result_map_decodes_the_two_offsets_and_the_suggestions() {
        let entry = Value::Map(vec![
            (Value::String("startIndex".to_string()), Value::I32(6)),
            (Value::String("endIndex".to_string()), Value::I32(10)),
            (
                Value::String("suggestions".to_string()),
                Value::List(vec![
                    Value::String("world".to_string()),
                    Value::String("weld".to_string()),
                ]),
            ),
        ]);
        assert_eq!(
            DefaultSpellCheckService::span_from(&entry),
            Some(span(6, 10, &["world", "weld"]))
        );

        // A span with no offsets is not a span; one with no suggestions is --
        // the platform reports a word it dislikes and has nothing to offer.
        let no_offsets = Value::Map(vec![(
            Value::String("suggestions".to_string()),
            Value::List(vec![]),
        )]);
        assert_eq!(DefaultSpellCheckService::span_from(&no_offsets), None);
        let no_suggestions = Value::Map(vec![
            (Value::String("startIndex".to_string()), Value::I64(1)),
            (Value::String("endIndex".to_string()), Value::I64(2)),
        ]);
        assert_eq!(
            DefaultSpellCheckService::span_from(&no_suggestions),
            Some(span(1, 2, &[]))
        );
    }

    #[test]
    fn the_channel_and_method_are_the_ones_the_platform_listens_on() {
        // A typo here is a spell checker that silently never answers, which
        // is indistinguishable from a platform that has no dictionary.
        assert_eq!(DefaultSpellCheckService::CHANNEL, "flutter/spellcheck");
        assert_eq!(
            DefaultSpellCheckService::METHOD,
            "SpellCheck.initiateSpellCheck"
        );
    }
}
