// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Telling a screen reader something happened, when nothing on screen said it.
//!
//! Upstream's `semantics/semantics_event.dart` and `semantics/semantics_service.dart`.
//!
//! Almost everything a reader hears comes from the semantics tree
//! ([`crate::semantics`]): the tree changes, the platform notices, and the
//! reader is told. These events are the other path -- the application saying
//! something out loud that the tree cannot express, because nothing on screen
//! changed. A camera app naming what came into the viewfinder is upstream's own
//! example, and it is a good one: the picture changed and no widget did.
//!
//! # Prefer the tree, and upstream keeps saying so
//!
//! The doc comment on nearly every one of these says to use `Semantics` instead
//! when it will do, and [`AnnounceSemanticsEvent`] carries a link to Android's
//! own deprecation of the mechanism. The reason is TalkBack's behaviour:
//! `announceForAccessibility` **clears the speech queue** and speaks over
//! whatever was being read. An announcement is an interruption, and an
//! interruption is only right when what it interrupts matters less.
//!
//! # These are messages, not calls
//!
//! Each event becomes a map on [`crate::services::system::ACCESSIBILITY`], a
//! `BasicMessageChannel` -- so there is no reply and no failure. The map's shape
//! is the protocol the engine's platform bridge reads, and it is what the tests
//! here pin: the engine turns `type` into `UIAccessibility*Notification` on iOS
//! and `AccessibilityEvent` on Android, and a key spelled differently is simply
//! not read.

use crate::direction::TextDirection;
use crate::services::codec::Value;

/// Upstream `Assertiveness`: whether an announcement waits its turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Assertiveness {
    /// Spoken when the reader is idle. The default, and almost always right.
    #[default]
    Polite,
    /// Interrupts whatever is being said.
    ///
    /// Upstream: "It should only be used for time-sensitive/critical
    /// notifications." Every announcement is already an interruption of the
    /// user's attention; this one is an interruption of the reader's sentence.
    Assertive,
}

impl Assertiveness {
    /// The wire value, which is the enum's index -- upstream sends
    /// `assertiveness.index`.
    pub fn index(self) -> i32 {
        match self {
            Assertiveness::Polite => 0,
            Assertiveness::Assertive => 1,
        }
    }
}

/// Upstream `SemanticsEvent`: something that happened, addressed to whatever
/// assistive technology is listening.
///
/// Upstream is an abstract class with a `type` string and a `getDataMap`; here
/// it is an enum, because the set is closed -- the engine's bridges switch on
/// exactly these five type strings and a sixth would not be understood by
/// anything.
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticsEvent {
    /// Upstream `AnnounceSemanticsEvent`.
    Announce {
        /// The view this is on. Upstream's older `SemanticsService.announce`
        /// read it off the implicit view and is deprecated for exactly that
        /// reason: with more than one window there is no implicit view to read.
        view_id: i32,
        message: String,
        /// The direction `message` is read in -- not the app's direction. An
        /// announcement in the other script than the interface is why this is
        /// per-event.
        text_direction: TextDirection,
        assertiveness: Assertiveness,
    },
    /// Upstream `TooltipSemanticsEvent`. Android only: it is how a tooltip's
    /// text reaches TalkBack, since the tooltip is not in the tree long enough
    /// to be walked.
    Tooltip { message: String },
    /// Upstream `LongPressSemanticsEvent`. Android only, and it carries no
    /// data: it asks TalkBack to play its long-press sound, nothing more.
    LongPress,
    /// Upstream `TapSemanticEvent`. Android only, the tap sound.
    ///
    /// Note the name: upstream's is `TapSemanticEvent`, not
    /// `TapSemanticsEvent`, while its long-press sibling is
    /// `LongPressSemanticsEvent`. The inconsistency is upstream's and is not
    /// worth silently correcting -- a reader looking for either name should
    /// find it.
    Tap,
    /// Upstream `FocusSemanticEvent`: move the reader's focus here.
    ///
    /// Upstream's own doc warns against it twice over -- "using this API is
    /// generally not recommended, as it may break a users' expectation of how
    /// a11y focus works" -- and gives one acceptable case (a focused object
    /// replaced by another) and one to avoid (a popup opening, where moving
    /// focus confuses rather than helps).
    Focus,
}

impl SemanticsEvent {
    /// Upstream's `type`: the string the engine's bridge switches on.
    pub fn event_type(&self) -> &'static str {
        match self {
            SemanticsEvent::Announce { .. } => "announce",
            SemanticsEvent::Tooltip { .. } => "tooltip",
            SemanticsEvent::LongPress => "longPress",
            SemanticsEvent::Tap => "tap",
            SemanticsEvent::Focus => "focus",
        }
    }

    /// Upstream's `getDataMap`.
    ///
    /// **`assertiveness` is left out when it is polite**, which is upstream's
    /// `if (assertiveness != Assertiveness.polite)` collection-if. Absent and
    /// zero mean the same thing to every current bridge, so this is only a
    /// smaller message -- but it is what upstream sends, and a test that
    /// compared against a map with the key present would be testing this port
    /// rather than the protocol.
    pub fn data_map(&self) -> Vec<(Value, Value)> {
        match self {
            SemanticsEvent::Announce {
                view_id,
                message,
                text_direction,
                assertiveness,
            } => {
                let mut data = vec![
                    (Value::String("viewId".into()), Value::I32(*view_id)),
                    (
                        Value::String("message".into()),
                        Value::String(message.clone()),
                    ),
                    (
                        Value::String("textDirection".into()),
                        Value::I32(direction_index(*text_direction)),
                    ),
                ];
                if *assertiveness != Assertiveness::Polite {
                    data.push((
                        Value::String("assertiveness".into()),
                        Value::I32(assertiveness.index()),
                    ));
                }
                data
            }
            SemanticsEvent::Tooltip { message } => vec![(
                Value::String("message".into()),
                Value::String(message.clone()),
            )],
            SemanticsEvent::LongPress | SemanticsEvent::Tap | SemanticsEvent::Focus => Vec::new(),
        }
    }

    /// Upstream's `toMap({int? nodeId})`: the whole message.
    ///
    /// `node_id` is added only when there is one -- an announcement belongs to
    /// the application rather than to any node, while a focus event has to name
    /// what to focus.
    pub fn to_map(&self, node_id: Option<i32>) -> Value {
        let mut event = vec![
            (
                Value::String("type".into()),
                Value::String(self.event_type().into()),
            ),
            (Value::String("data".into()), Value::Map(self.data_map())),
        ];
        if let Some(node_id) = node_id {
            event.push((Value::String("nodeId".into()), Value::I32(node_id)));
        }
        Value::Map(event)
    }
}

/// Upstream sends `textDirection.index`, and the two enums are **not in the
/// same order**.
///
/// `dart:ui` declares `rtl` first (`sky_engine/lib/ui/text.dart`), so on the
/// wire `rtl` is 0 and `ltr` is 1. This crate's [`TextDirection`] declares
/// `Ltr` first, because left-to-right is the default everywhere else in it.
/// Casting one to the other would send every announcement in the wrong
/// direction, and nothing on either side would report a fault -- a screen
/// reader would simply read Arabic left to right.
///
/// Which is why this is a written-out mapping and not a cast.
fn direction_index(direction: TextDirection) -> i32 {
    match direction {
        TextDirection::Rtl => 0,
        TextDirection::Ltr => 1,
    }
}

/// Upstream `SemanticsService`: the way an application sends one of these.
///
/// A namespace rather than an object, as upstream's `abstract final class` is.
pub struct SemanticsService;

impl SemanticsService {
    /// Upstream `SemanticsService.sendAnnouncement`.
    ///
    /// The view is named rather than looked up. Upstream's older `announce`
    /// read the implicit view and is deprecated with its reason attached --
    /// "this API is incompatible with multiple windows" -- so the replacement
    /// is what is ported and the deprecated shape is not.
    pub fn send_announcement(
        view_id: i32,
        message: impl Into<String>,
        text_direction: TextDirection,
        assertiveness: Assertiveness,
    ) {
        send(SemanticsEvent::Announce {
            view_id,
            message: message.into(),
            text_direction,
            assertiveness,
        });
    }

    /// Upstream `SemanticsService.tooltip`.
    pub fn tooltip(message: impl Into<String>) {
        send(SemanticsEvent::Tooltip {
            message: message.into(),
        });
    }

    /// Not upstream's, and named for what it is: the two above are the whole of
    /// `SemanticsService`, and the other three events reach the platform
    /// through `RenderObject.sendSemanticsEvent` with a node id. There is no
    /// such method on [`crate::render::RenderBox`] yet, so this is the seam
    /// until there is.
    pub fn send_for_node(node_id: i32, event: SemanticsEvent) {
        send_with_node(event, Some(node_id));
    }
}

/// Puts an event on the accessibility channel.
fn send(event: SemanticsEvent) {
    send_with_node(event, None);
}

fn send_with_node(event: SemanticsEvent, node_id: Option<i32>) {
    crate::services::system::ACCESSIBILITY.send(&event.to_map(node_id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(map: &Value, name: &str) -> Option<Value> {
        let Value::Map(pairs) = map else {
            return None;
        };
        pairs
            .iter()
            .find(|(k, _)| *k == Value::String(name.into()))
            .map(|(_, v)| v.clone())
    }

    fn data(event: &SemanticsEvent, name: &str) -> Option<Value> {
        key(&key(&event.to_map(None), "data")?, name)
    }

    // -- The five type strings ----------------------------------------------------

    #[test]
    fn every_event_carries_the_type_string_the_engine_switches_on() {
        // The engine turns these into `UIAccessibility*Notification` on iOS and
        // `AccessibilityEvent` on Android. A word spelled differently is not an
        // error -- it is simply not read.
        assert_eq!(
            SemanticsEvent::Announce {
                view_id: 0,
                message: String::new(),
                text_direction: TextDirection::Ltr,
                assertiveness: Assertiveness::Polite,
            }
            .event_type(),
            "announce"
        );
        assert_eq!(
            SemanticsEvent::Tooltip {
                message: String::new()
            }
            .event_type(),
            "tooltip"
        );
        assert_eq!(SemanticsEvent::LongPress.event_type(), "longPress");
        assert_eq!(SemanticsEvent::Tap.event_type(), "tap");
        assert_eq!(SemanticsEvent::Focus.event_type(), "focus");
    }

    #[test]
    fn long_press_is_camel_cased_and_the_others_are_one_word() {
        // Worth its own line: `longPress` is the only one with a capital in it,
        // and `long_press` or `longpress` would both be silently ignored.
        assert_eq!(SemanticsEvent::LongPress.event_type(), "longPress");
        assert!(
            !SemanticsEvent::Tap
                .event_type()
                .contains(char::is_uppercase)
        );
    }

    // -- The announcement's payload -----------------------------------------------

    #[test]
    fn an_announcement_carries_its_view_message_and_direction() {
        let event = SemanticsEvent::Announce {
            view_id: 7,
            message: "Two faces in frame".into(),
            text_direction: TextDirection::Ltr,
            assertiveness: Assertiveness::Polite,
        };
        assert_eq!(data(&event, "viewId"), Some(Value::I32(7)));
        assert_eq!(
            data(&event, "message"),
            Some(Value::String("Two faces in frame".into()))
        );
        assert!(data(&event, "textDirection").is_some());
    }

    #[test]
    fn the_direction_index_is_dart_uis_and_not_this_crates() {
        // The two enums are in opposite orders: `dart:ui` declares `rtl` first,
        // this crate declares `Ltr` first. A cast would send every announcement
        // in the wrong direction, and nothing on either side would report a
        // fault -- a screen reader would just read Arabic left to right.
        let ltr = SemanticsEvent::Announce {
            view_id: 0,
            message: "hello".into(),
            text_direction: TextDirection::Ltr,
            assertiveness: Assertiveness::Polite,
        };
        let rtl = SemanticsEvent::Announce {
            view_id: 0,
            message: "مرحبا".into(),
            text_direction: TextDirection::Rtl,
            assertiveness: Assertiveness::Polite,
        };
        assert_eq!(
            data(&rtl, "textDirection"),
            Some(Value::I32(0)),
            "dart:ui's rtl is 0"
        );
        assert_eq!(
            data(&ltr, "textDirection"),
            Some(Value::I32(1)),
            "and its ltr is 1"
        );
        assert_ne!(
            data(&ltr, "textDirection"),
            Some(Value::I32(TextDirection::Ltr as i32)),
            "which is not this crate's own index -- the point of the mapping"
        );
    }

    #[test]
    fn a_polite_announcement_leaves_the_assertiveness_out_altogether() {
        // Upstream's collection-if. Absent and zero read the same to every
        // current bridge, so this is only a smaller message -- but it is the
        // message upstream sends, and a test written against a map with the key
        // present would be pinning this port rather than the protocol.
        let polite = SemanticsEvent::Announce {
            view_id: 0,
            message: "ready".into(),
            text_direction: TextDirection::Ltr,
            assertiveness: Assertiveness::Polite,
        };
        assert_eq!(data(&polite, "assertiveness"), None);

        let urgent = SemanticsEvent::Announce {
            view_id: 0,
            message: "battery critical".into(),
            text_direction: TextDirection::Ltr,
            assertiveness: Assertiveness::Assertive,
        };
        assert_eq!(data(&urgent, "assertiveness"), Some(Value::I32(1)));
    }

    #[test]
    fn polite_is_the_default() {
        // Every announcement is already an interruption of the reader's
        // attention; assertive is an interruption of the reader's sentence, and
        // is opt-in for that reason.
        assert_eq!(Assertiveness::default(), Assertiveness::Polite);
        assert_eq!(Assertiveness::Polite.index(), 0);
        assert_eq!(Assertiveness::Assertive.index(), 1);
    }

    // -- The events with nothing to say -------------------------------------------

    #[test]
    fn the_three_feedback_events_carry_no_data() {
        // They ask TalkBack to make a sound or move focus. There is nothing to
        // describe, and an empty map is what upstream sends -- not an absent
        // `data` key.
        for event in [
            SemanticsEvent::LongPress,
            SemanticsEvent::Tap,
            SemanticsEvent::Focus,
        ] {
            assert!(event.data_map().is_empty());
            assert_eq!(
                key(&event.to_map(None), "data"),
                Some(Value::Map(Vec::new())),
                "an empty map, not a missing key"
            );
        }
    }

    #[test]
    fn a_tooltip_event_carries_only_its_text() {
        let event = SemanticsEvent::Tooltip {
            message: "Create".into(),
        };
        assert_eq!(event.data_map().len(), 1);
        assert_eq!(
            data(&event, "message"),
            Some(Value::String("Create".into()))
        );
    }

    // -- The envelope --------------------------------------------------------------

    #[test]
    fn a_node_id_is_included_only_when_there_is_one() {
        // An announcement belongs to the application and names no node; a focus
        // event has to say what to focus.
        let event = SemanticsEvent::Focus;
        assert_eq!(key(&event.to_map(None), "nodeId"), None);
        assert_eq!(key(&event.to_map(Some(42)), "nodeId"), Some(Value::I32(42)));
    }

    #[test]
    fn the_envelope_is_type_then_data() {
        // The map is insertion-ordered here and in Dart, and the order is what
        // goes over the wire. Nothing reads it positionally, but a diff of two
        // encodings is a great deal easier to read when it is stable.
        let Value::Map(pairs) = SemanticsEvent::Tap.to_map(Some(3)) else {
            panic!("an event encodes as a map");
        };
        let keys: Vec<&Value> = pairs.iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            vec![
                &Value::String("type".into()),
                &Value::String("data".into()),
                &Value::String("nodeId".into()),
            ]
        );
    }

    #[test]
    fn an_announcements_data_keys_are_in_upstreams_order() {
        let event = SemanticsEvent::Announce {
            view_id: 1,
            message: "x".into(),
            text_direction: TextDirection::Ltr,
            assertiveness: Assertiveness::Assertive,
        };
        let keys: Vec<Value> = event.data_map().into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            vec![
                Value::String("viewId".into()),
                Value::String("message".into()),
                Value::String("textDirection".into()),
                Value::String("assertiveness".into()),
            ]
        );
    }

    #[test]
    fn the_whole_message_encodes_on_the_accessibility_codec() {
        // The channel is a BasicMessageChannel with the standard codec, so the
        // one thing that could still go wrong is a Value the codec cannot
        // write. This checks the shape survives a round trip rather than
        // trusting that it would.
        use crate::services::codec::{MessageCodec, StandardMessageCodec};
        let event = SemanticsEvent::Announce {
            view_id: 2,
            message: "encoded".into(),
            text_direction: TextDirection::Rtl,
            assertiveness: Assertiveness::Assertive,
        };
        let codec = StandardMessageCodec::new();
        let bytes = codec.encode(&event.to_map(Some(9))).expect("encodable");
        let back = codec.decode(&bytes).expect("decodable");
        assert_eq!(back, event.to_map(Some(9)));
    }
}
