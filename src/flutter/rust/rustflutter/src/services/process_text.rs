// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What the platform can do with a selection (upstream
//! `services/process_text.dart`).
//!
//! Android lets any application register something it can do to selected
//! text -- translate it, look it up, add it to a note -- and offers those in
//! the selection toolbar. The framework does not know what they are, so it
//! asks: which actions exist, and then, run this one on this string.
//!
//! # Recorded divergences
//!
//! * Both calls are `Future`s upstream and callbacks here, for the reason
//!   recorded on [`spell_check`](crate::services::spell_check).
//! * Upstream's `setChannel` exists so a test can swap the channel; this
//!   takes one at construction instead, which does the same and does not need
//!   an assert to hide it from release builds.

use crate::services::channel::MethodChannel;
use crate::services::codec::{StandardMethodCodec, Value};

/// Upstream `ProcessTextAction`: one thing the platform offers to do.
///
/// The id is what to send back to run it; the label is what to show. They are
/// separate because the label is translated and the id is not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessTextAction {
    pub id: String,
    pub label: String,
}

impl ProcessTextAction {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> ProcessTextAction {
        ProcessTextAction {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Upstream `ProcessTextService`.
pub trait ProcessTextService {
    /// Upstream `queryTextActions`.
    fn query_text_actions(&self, callback: Box<dyn FnOnce(Vec<ProcessTextAction>)>);

    /// Upstream `processTextAction`. `read_only` tells the platform whether
    /// the field can be written back to, which is what decides between
    /// offering to replace the text and offering only to read it.
    fn process_text_action(
        &self,
        id: &str,
        text: &str,
        read_only: bool,
        callback: Box<dyn FnOnce(Option<String>)>,
    );
}

/// Upstream `DefaultProcessTextService`: the one that asks the platform.
pub struct DefaultProcessTextService {
    channel: MethodChannel<StandardMethodCodec>,
}

impl Default for DefaultProcessTextService {
    fn default() -> DefaultProcessTextService {
        DefaultProcessTextService::new()
    }
}

impl DefaultProcessTextService {
    /// Upstream `SystemChannels.processText`.
    pub const CHANNEL: &'static str = "flutter/processtext";
    pub const QUERY_METHOD: &'static str = "ProcessText.queryTextActions";
    pub const ACTION_METHOD: &'static str = "ProcessText.processTextAction";

    pub fn new() -> DefaultProcessTextService {
        DefaultProcessTextService::with_channel(MethodChannel::named(
            DefaultProcessTextService::CHANNEL,
            StandardMethodCodec,
        ))
    }

    /// The service over a channel of the caller's choosing, which is what
    /// upstream's `setChannel` is for.
    pub fn with_channel(channel: MethodChannel<StandardMethodCodec>) -> DefaultProcessTextService {
        DefaultProcessTextService { channel }
    }

    /// Upstream's decoding of the query's reply: a map of id to label.
    ///
    /// Anything that is not a pair of strings is dropped rather than
    /// refusing the whole list -- one malformed entry should not cost the
    /// reader every other action the platform offers.
    pub fn actions_from(reply: &Value) -> Vec<ProcessTextAction> {
        let Value::Map(pairs) = reply else {
            return Vec::new();
        };
        pairs
            .iter()
            .filter_map(|(id, label)| match (id, label) {
                (Value::String(id), Value::String(label)) => {
                    Some(ProcessTextAction::new(id.clone(), label.clone()))
                }
                _ => None,
            })
            .collect()
    }
}

impl ProcessTextService for DefaultProcessTextService {
    fn query_text_actions(&self, callback: Box<dyn FnOnce(Vec<ProcessTextAction>)>) {
        self.channel.invoke_with_reply(
            DefaultProcessTextService::QUERY_METHOD,
            Value::Null,
            move |reply| {
                // Upstream catches and answers with the empty list, and a
                // null reply is the empty list too: a platform with nothing
                // to offer and a platform that failed to say are the same
                // thing to the toolbar that has to be drawn either way.
                callback(match reply {
                    Ok(Some(value)) => DefaultProcessTextService::actions_from(&value),
                    _ => Vec::new(),
                });
            },
        );
    }

    fn process_text_action(
        &self,
        id: &str,
        text: &str,
        read_only: bool,
        callback: Box<dyn FnOnce(Option<String>)>,
    ) {
        let arguments = Value::List(vec![
            Value::String(id.to_string()),
            Value::String(text.to_string()),
            Value::Bool(read_only),
        ]);
        self.channel.invoke_with_reply(
            DefaultProcessTextService::ACTION_METHOD,
            arguments,
            move |reply| {
                // Null is a real answer: the action ran and changed nothing,
                // which is what a "look this up" action does.
                callback(match reply {
                    Ok(Some(Value::String(text))) => Some(text),
                    _ => None,
                });
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(id: &str, label: &str) -> (Value, Value) {
        (
            Value::String(id.to_string()),
            Value::String(label.to_string()),
        )
    }

    #[test]
    fn the_reply_is_a_map_of_id_to_label() {
        // Two fields and not one, because the label is translated and the id
        // is not: sending back the label would break the moment the reader
        // changes language.
        let reply = Value::Map(vec![
            pair("android.intent.TRANSLATE", "Translate"),
            pair("com.example.NOTE", "Add to note"),
        ]);
        assert_eq!(
            DefaultProcessTextService::actions_from(&reply),
            vec![
                ProcessTextAction::new("android.intent.TRANSLATE", "Translate"),
                ProcessTextAction::new("com.example.NOTE", "Add to note"),
            ]
        );
    }

    #[test]
    fn one_malformed_entry_does_not_cost_the_others() {
        // A platform that sends one bad pair should not empty the toolbar.
        let reply = Value::Map(vec![
            pair("good", "Good"),
            (Value::I32(3), Value::String("no id".to_string())),
            (Value::String("no label".to_string()), Value::Null),
        ]);
        assert_eq!(
            DefaultProcessTextService::actions_from(&reply),
            vec![ProcessTextAction::new("good", "Good")]
        );
    }

    #[test]
    fn a_reply_that_is_not_a_map_is_no_actions_rather_than_a_refusal() {
        // Upstream catches and answers with the empty list. A platform with
        // nothing to offer and one that failed to say are the same thing to
        // the toolbar that has to be drawn either way.
        assert!(DefaultProcessTextService::actions_from(&Value::Null).is_empty());
        assert!(DefaultProcessTextService::actions_from(&Value::List(vec![])).is_empty());
        assert!(DefaultProcessTextService::actions_from(&Value::Map(vec![])).is_empty());
    }

    #[test]
    fn the_channel_and_the_two_methods_are_the_ones_the_platform_listens_on() {
        assert_eq!(DefaultProcessTextService::CHANNEL, "flutter/processtext");
        assert_eq!(
            DefaultProcessTextService::QUERY_METHOD,
            "ProcessText.queryTextActions"
        );
        assert_eq!(
            DefaultProcessTextService::ACTION_METHOD,
            "ProcessText.processTextAction"
        );
    }
}
