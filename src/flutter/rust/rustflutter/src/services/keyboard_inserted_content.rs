// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Content a soft keyboard put into a field that is not text (upstream
//! `services/keyboard_inserted_content.dart` and
//! `services/predictive_back_event.dart`).
//!
//! Two small things a platform can say about a field, each too small for a
//! file of its own here: an Android keyboard inserting a GIF or a sticker,
//! and an Android back gesture that has started but not yet been let go of.

use crate::render::Offset;
use crate::services::codec::Value;

/// Reads a named entry out of a decoded map.
fn entry<'a>(pairs: &'a [(Value, Value)], name: &str) -> Option<&'a Value> {
    pairs
        .iter()
        .find(|(key, _)| matches!(key, Value::String(key) if key == name))
        .map(|(_, value)| value)
}

fn string_entry(pairs: &[(Value, Value)], name: &str) -> Option<String> {
    match entry(pairs, name) {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn number_entry(pairs: &[(Value, Value)], name: &str) -> Option<f64> {
    match entry(pairs, name) {
        Some(Value::I32(number)) => Some(*number as f64),
        Some(Value::I64(number)) => Some(*number as f64),
        Some(Value::F64(number)) => Some(*number),
        _ => None,
    }
}

/// Upstream `KeyboardInsertedContent`: what an Android keyboard put into a
/// text field that is not text.
///
/// A GIF, a sticker, an image pasted from the keyboard's own picker. The
/// field is told the mime type and a URI, and -- when the platform chose to
/// send it inline -- the bytes as well.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct KeyboardInsertedContent {
    pub mime_type: String,
    pub uri: String,
    /// Upstream's nullable `Uint8List`. Absent means the content is at the
    /// URI and has not been sent inline, which is the ordinary case for
    /// anything large.
    pub data: Option<Vec<u8>>,
}

impl KeyboardInsertedContent {
    pub fn new(mime_type: impl Into<String>, uri: impl Into<String>) -> KeyboardInsertedContent {
        KeyboardInsertedContent {
            mime_type: mime_type.into(),
            uri: uri.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Vec<u8>) -> Self {
        self.data = Some(data);
        self
    }

    /// Upstream `hasData`: absent and empty are the same answer, because a
    /// zero-length attachment is nothing to insert either way.
    pub fn has_data(&self) -> bool {
        self.data.as_ref().is_some_and(|data| !data.is_empty())
    }

    /// Upstream `KeyboardInsertedContent.fromJson`.
    pub fn from_json(metadata: &[(Value, Value)]) -> Option<KeyboardInsertedContent> {
        Some(KeyboardInsertedContent {
            mime_type: string_entry(metadata, "mimeType")?,
            uri: string_entry(metadata, "uri")?,
            data: match entry(metadata, "data") {
                Some(Value::Bytes(bytes)) => Some(bytes.clone()),
                // Upstream builds the list from any iterable of ints, which
                // is what the standard codec's int list arrives as when the
                // platform did not send it as bytes.
                Some(Value::I32List(numbers)) => {
                    Some(numbers.iter().map(|number| *number as u8).collect())
                }
                Some(Value::List(items)) => Some(
                    items
                        .iter()
                        .filter_map(|item| match item {
                            Value::I32(number) => Some(*number as u8),
                            Value::I64(number) => Some(*number as u8),
                            _ => None,
                        })
                        .collect(),
                ),
                _ => None,
            },
        })
    }
}

/// Upstream `SwipeEdge`: which side of the screen a back gesture began at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SwipeEdge {
    #[default]
    Left,
    Right,
}

/// Upstream `PredictiveBackEvent`: an Android back gesture in progress.
///
/// The gesture is reported while it is happening so that an application can
/// show where it is heading and the reader can change their mind -- which is
/// what makes it predictive, and why `progress` is a fraction rather than a
/// completion.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PredictiveBackEvent {
    /// Where the finger is. Absent when there is no finger, which is a back
    /// *button* rather than a gesture.
    pub touch_offset: Option<Offset>,
    /// How far through the gesture is, from nothing to all the way.
    pub progress: f32,
    pub swipe_edge: SwipeEdge,
}

impl PredictiveBackEvent {
    pub fn new(
        touch_offset: Option<Offset>,
        progress: f32,
        swipe_edge: SwipeEdge,
    ) -> PredictiveBackEvent {
        debug_assert!(
            (0.0..=1.0).contains(&progress),
            "a back gesture's progress is a fraction"
        );
        PredictiveBackEvent {
            touch_offset,
            progress,
            swipe_edge,
        }
    }

    /// Upstream `isButtonEvent`: whether this is the back *button* rather
    /// than a gesture.
    ///
    /// Upstream's comment explains the second half. Android's documentation
    /// says the touch coordinates are NaN for a button press; in practice
    /// they come through as zero, so a zero offset at zero progress counts as
    /// a button too. That was checked against an emulator on API 34, and the
    /// check stays because the documentation and the device disagree.
    pub fn is_button_event(&self) -> bool {
        match self.touch_offset {
            None => true,
            Some(offset) => self.progress == 0.0 && offset == Offset::ZERO,
        }
    }

    /// Upstream `PredictiveBackEvent.fromMap`.
    pub fn from_map(map: &[(Value, Value)]) -> Option<PredictiveBackEvent> {
        let touch_offset = match entry(map, "touchOffset") {
            Some(Value::List(items)) if items.len() >= 2 => {
                let number = |value: &Value| match value {
                    Value::I32(number) => Some(*number as f32),
                    Value::I64(number) => Some(*number as f32),
                    Value::F64(number) => Some(*number as f32),
                    _ => None,
                };
                Some(Offset::new(number(&items[0])?, number(&items[1])?))
            }
            Some(Value::F64List(numbers)) if numbers.len() >= 2 => {
                Some(Offset::new(numbers[0] as f32, numbers[1] as f32))
            }
            _ => None,
        };
        let swipe_edge = match number_entry(map, "swipeEdge")? as i64 {
            0 => SwipeEdge::Left,
            1 => SwipeEdge::Right,
            // Upstream indexes `SwipeEdge.values` and would throw. There are
            // two edges and the platform sends one of them; anything else is
            // a malformed message, and refusing it beats a panic.
            _ => return None,
        };
        Some(PredictiveBackEvent {
            touch_offset,
            progress: number_entry(map, "progress")? as f32,
            swipe_edge,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, Value)]) -> Vec<(Value, Value)> {
        pairs
            .iter()
            .map(|(key, value)| (Value::String((*key).to_string()), value.clone()))
            .collect()
    }

    #[test]
    fn inserted_content_needs_both_a_type_and_a_uri() {
        let full = map(&[
            ("mimeType", Value::String("image/gif".to_string())),
            ("uri", Value::String("content://gif/1".to_string())),
            ("data", Value::Bytes(vec![1, 2, 3])),
        ]);
        let content = KeyboardInsertedContent::from_json(&full).expect("a content");
        assert_eq!(content.mime_type, "image/gif");
        assert_eq!(content.data.as_deref(), Some(&[1u8, 2, 3][..]));

        // Without one of the two there is nothing to insert and nothing to
        // fetch, so it is not content.
        assert_eq!(
            KeyboardInsertedContent::from_json(&map(&[(
                "mimeType",
                Value::String("image/gif".to_string())
            )])),
            None
        );
    }

    #[test]
    fn content_with_no_bytes_is_still_content() {
        // The ordinary case for anything large: the platform sends a URI and
        // leaves the fetching to the application.
        let content = KeyboardInsertedContent::from_json(&map(&[
            ("mimeType", Value::String("image/png".to_string())),
            ("uri", Value::String("content://png/1".to_string())),
        ]))
        .expect("a content");
        assert_eq!(content.data, None);
        assert!(!content.has_data());
    }

    #[test]
    fn empty_bytes_count_as_no_data() {
        // Upstream's `data?.isNotEmpty ?? false`: absent and empty give the
        // same answer, because a zero-length attachment is nothing to insert
        // either way.
        assert!(!KeyboardInsertedContent::new("image/gif", "uri").has_data());
        assert!(
            !KeyboardInsertedContent::new("image/gif", "uri")
                .with_data(Vec::new())
                .has_data()
        );
        assert!(
            KeyboardInsertedContent::new("image/gif", "uri")
                .with_data(vec![0])
                .has_data()
        );
    }

    #[test]
    fn a_back_gesture_reports_where_the_finger_is_and_how_far_it_got() {
        let event = PredictiveBackEvent::from_map(&map(&[
            (
                "touchOffset",
                Value::List(vec![Value::F64(12.0), Value::F64(34.0)]),
            ),
            ("progress", Value::F64(0.5)),
            ("swipeEdge", Value::I32(1)),
        ]))
        .expect("an event");
        assert_eq!(event.touch_offset, Some(Offset::new(12.0, 34.0)));
        assert_eq!(event.progress, 0.5);
        assert_eq!(event.swipe_edge, SwipeEdge::Right);
        assert!(!event.is_button_event());
    }

    #[test]
    fn the_back_button_arrives_as_a_gesture_that_never_moved() {
        // Two ways, and upstream accepts both. No touch at all is a button;
        // and so is a touch at the origin with no progress, because Android
        // documents NaN coordinates for a button press and in practice sends
        // zeroes. The device and the documentation disagree, so the check
        // covers what the device does.
        assert!(PredictiveBackEvent::new(None, 0.0, SwipeEdge::Left).is_button_event());
        assert!(
            PredictiveBackEvent::new(Some(Offset::ZERO), 0.0, SwipeEdge::Left).is_button_event()
        );
        // A gesture that started at the origin and has moved is not a button.
        assert!(
            !PredictiveBackEvent::new(Some(Offset::ZERO), 0.3, SwipeEdge::Left).is_button_event()
        );
    }

    #[test]
    fn a_swipe_edge_the_platform_could_not_have_sent_is_refused() {
        // Upstream indexes `SwipeEdge.values` and would throw. There are two
        // edges; anything else is a malformed message, and refusing it beats
        // a panic on the back gesture.
        assert_eq!(
            PredictiveBackEvent::from_map(&map(&[
                ("progress", Value::F64(0.0)),
                ("swipeEdge", Value::I32(7)),
            ])),
            None
        );
        // Progress is required -- an event that does not say how far it got
        // says nothing.
        assert_eq!(
            PredictiveBackEvent::from_map(&map(&[("swipeEdge", Value::I32(0))])),
            None
        );
    }
}
