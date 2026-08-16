// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Text input, and with it the IME.
//!
//! A text field does not read the keyboard. It asks the platform to start
//! editing on its behalf and is told what the text now is -- because on every
//! platform there is something between the keys and the characters, and on
//! Windows that something is the IME. Typing 中文 is four or five keystrokes
//! that produce no text at all, then a candidate list, then one character; no
//! amount of key handling in the framework can turn the first into the last.
//!
//! So this is not a convenience over [`crate::keyboard`]. It is the only way
//! text input can work, and it is why upstream's `EditableText` talks to
//! `TextInput` rather than to `RawKeyboardListener`.
//!
//! # The conversation
//!
//! Upstream's `flutter/textinput`, unchanged, because the platform half of it
//! is already written on four operating systems:
//!
//! | Framework says | Meaning |
//! |---|---|
//! | `TextInput.setClient` | this field is now the one being edited |
//! | `TextInput.show` / `.hide` | raise or drop the on-screen keyboard |
//! | `TextInput.setEditingState` | the field's contents changed from this side |
//! | `TextInput.setEditableSizeAndTransform` | where the field is in the window |
//! | `TextInput.setMarkedTextRect` | where the caret is inside the field |
//! | `TextInput.clearClient` | editing is over |
//!
//! | Platform says | Meaning |
//! |---|---|
//! | `TextInputClient.updateEditingState` | the text is now this |
//! | `TextInputClient.performAction` | the reader pressed Enter |
//!
//! The last two arrive as `[clientId, payload]`, and the client id is what
//! makes more than one field possible: a stale update for a field that has
//! since lost focus is recognised and dropped rather than applied to whatever
//! is focused now.
//!
//! # Where the caret rectangle goes
//!
//! `setMarkedTextRect` and `setEditableSizeAndTransform` look like decoration
//! and are not: they are what tells the IME where to put its candidate list.
//! Without them the list of characters the reader is choosing from appears in
//! the corner of the window instead of under the word being typed.

use super::codec::{JsonMethodCodec, MethodCall, Value};
use super::MethodChannel;

/// What a text field holds, and where the caret is in it.
///
/// Upstream's `TextEditingValue`. Every offset is in UTF-16 code units,
/// because that is what the platform counts in -- Windows, Java and
/// JavaScript all do -- and a framework that counted in bytes or in `char`s
/// would disagree with the platform about where the caret is the first time
/// somebody types an emoji.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEditingValue {
    pub text: String,
    /// Where the selection starts, in UTF-16 code units.
    pub selection_base: i32,
    /// Where it ends. Equal to the base when there is no selection, which is
    /// the ordinary case: that is the caret.
    pub selection_extent: i32,
    /// The text the IME is still composing, or `-1` for neither.
    ///
    /// This is the underlined run -- the pinyin the reader has typed but not
    /// yet turned into characters. It is part of `text`, and it is not
    /// committed: the next keystroke can replace all of it.
    pub composing_base: i32,
    pub composing_extent: i32,
}

impl Default for TextEditingValue {
    fn default() -> TextEditingValue {
        TextEditingValue {
            text: String::new(),
            selection_base: 0,
            selection_extent: 0,
            composing_base: -1,
            composing_extent: -1,
        }
    }
}

impl TextEditingValue {
    /// A field holding `text` with the caret at the end and nothing composing.
    pub fn new(text: impl Into<String>) -> TextEditingValue {
        let text = text.into();
        let end = text.encode_utf16().count() as i32;
        TextEditingValue {
            text,
            selection_base: end,
            selection_extent: end,
            composing_base: -1,
            composing_extent: -1,
        }
    }

    /// Whether the IME is part-way through a word.
    pub fn is_composing(&self) -> bool {
        self.composing_base >= 0
            && self.composing_extent >= 0
            && self.composing_base != self.composing_extent
    }

    /// The composing run as a byte range into `text`, for drawing it
    /// underlined. `None` when nothing is composing or the range is not a
    /// character boundary.
    pub fn composing_bytes(&self) -> Option<std::ops::Range<usize>> {
        if !self.is_composing() {
            return None;
        }
        let start = self.composing_base.min(self.composing_extent);
        let end = self.composing_base.max(self.composing_extent);
        Some(utf16_to_byte(&self.text, start)?..utf16_to_byte(&self.text, end)?)
    }

    /// The caret as a byte offset into `text`, for measuring the run before it.
    pub fn caret_bytes(&self) -> Option<usize> {
        utf16_to_byte(&self.text, self.selection_extent)
    }

    /// Whether anything is selected, as opposed to the caret merely sitting
    /// somewhere. Upstream's `TextSelection.isCollapsed`, negated.
    pub fn has_selection(&self) -> bool {
        self.selection_base != self.selection_extent
    }

    /// The selected run as a byte range into `text`, for drawing it
    /// highlighted.
    ///
    /// Ordered, which the wire values are not: dragging right to left sends a
    /// base after the extent, and the direction is the platform's business
    /// rather than the painter's.
    pub fn selection_bytes(&self) -> Option<std::ops::Range<usize>> {
        if !self.has_selection() {
            return None;
        }
        let start = self.selection_base.min(self.selection_extent);
        let end = self.selection_base.max(self.selection_extent);
        Some(utf16_to_byte(&self.text, start)?..utf16_to_byte(&self.text, end)?)
    }

    fn from_state(state: &Value) -> Option<TextEditingValue> {
        let number = |key: &str, fallback: i32| {
            state.get(key).and_then(Value::as_i64).map(|v| v as i32).unwrap_or(fallback)
        };
        Some(TextEditingValue {
            text: state.get("text")?.as_str()?.to_string(),
            selection_base: number("selectionBase", 0),
            selection_extent: number("selectionExtent", 0),
            composing_base: number("composingBase", -1),
            composing_extent: number("composingExtent", -1),
        })
    }

    fn to_state(&self) -> Value {
        Value::map([
            ("text", Value::from(self.text.as_str())),
            ("selectionBase", Value::I64(self.selection_base as i64)),
            ("selectionExtent", Value::I64(self.selection_extent as i64)),
            ("selectionAffinity", Value::from("TextAffinity.downstream")),
            ("selectionIsDirectional", Value::Bool(false)),
            ("composingBase", Value::I64(self.composing_base as i64)),
            ("composingExtent", Value::I64(self.composing_extent as i64)),
        ])
    }
}

/// A UTF-16 offset as a byte offset into `text`.
///
/// Returns `None` if the offset is out of range or lands inside a character,
/// which a correct platform never sends but a malformed message might.
fn utf16_to_byte(text: &str, offset: i32) -> Option<usize> {
    if offset < 0 {
        return None;
    }
    let mut remaining = offset as usize;
    if remaining == 0 {
        return Some(0);
    }
    for (index, character) in text.char_indices() {
        let units = character.len_utf16();
        if remaining < units {
            return None;
        }
        remaining -= units;
        if remaining == 0 {
            return Some(index + character.len_utf8());
        }
    }
    None
}

/// What kind of text a field wants, which decides the keyboard on a phone and
/// the Enter behaviour everywhere.
///
/// Upstream's `TextInputType`, in the order its `values` list them. The wire
/// name of `streetAddress` is `TextInputType.address` -- upstream's `_names`
/// table -- and everything else is the variant's own name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextInputType {
    #[default]
    Text,
    Multiline,
    Number,
    Phone,
    Datetime,
    Email,
    Url,
    VisiblePassword,
    Name,
    StreetAddress,
    /// Prevent the OS from showing the on-screen virtual keyboard.
    None,
    WebSearch,
    Twitter,
}

impl TextInputType {
    fn as_name(self) -> &'static str {
        match self {
            TextInputType::Text => "TextInputType.text",
            TextInputType::Multiline => "TextInputType.multiline",
            TextInputType::Number => "TextInputType.number",
            TextInputType::Phone => "TextInputType.phone",
            TextInputType::Datetime => "TextInputType.datetime",
            TextInputType::Email => "TextInputType.emailAddress",
            TextInputType::Url => "TextInputType.url",
            TextInputType::VisiblePassword => "TextInputType.visiblePassword",
            TextInputType::Name => "TextInputType.name",
            TextInputType::StreetAddress => "TextInputType.address",
            TextInputType::None => "TextInputType.none",
            TextInputType::WebSearch => "TextInputType.webSearch",
            TextInputType::Twitter => "TextInputType.twitter",
        }
    }
}

/// What the Enter key does.
///
/// Upstream's `TextInputAction`, in the order the enum declares its members.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextInputAction {
    /// There is no relevant input action for the current input source.
    None,
    /// Let the OS decide which action is most appropriate.
    Unspecified,
    #[default]
    Done,
    Go,
    Search,
    Send,
    Next,
    Previous,
    ContinueAction,
    Join,
    Route,
    EmergencyCall,
    Newline,
}

impl TextInputAction {
    fn as_name(self) -> &'static str {
        match self {
            TextInputAction::None => "TextInputAction.none",
            TextInputAction::Unspecified => "TextInputAction.unspecified",
            TextInputAction::Done => "TextInputAction.done",
            TextInputAction::Go => "TextInputAction.go",
            TextInputAction::Search => "TextInputAction.search",
            TextInputAction::Send => "TextInputAction.send",
            TextInputAction::Next => "TextInputAction.next",
            TextInputAction::Previous => "TextInputAction.previous",
            TextInputAction::ContinueAction => "TextInputAction.continueAction",
            TextInputAction::Join => "TextInputAction.join",
            TextInputAction::Route => "TextInputAction.route",
            TextInputAction::EmergencyCall => "TextInputAction.emergencyCall",
            TextInputAction::Newline => "TextInputAction.newline",
        }
    }

    /// The action a platform message named, or `None` when the name is not one
    /// upstream defines.
    ///
    /// Upstream's `_TextInputActionEnumMapper` maps the names it knows and
    /// throws on the rest; a wrong name here is dropped rather than reported
    /// to a field as an action the reader never pressed.
    fn from_name(name: &str) -> Option<TextInputAction> {
        match name {
            "TextInputAction.none" => Some(TextInputAction::None),
            "TextInputAction.unspecified" => Some(TextInputAction::Unspecified),
            "TextInputAction.done" => Some(TextInputAction::Done),
            "TextInputAction.go" => Some(TextInputAction::Go),
            "TextInputAction.search" => Some(TextInputAction::Search),
            "TextInputAction.send" => Some(TextInputAction::Send),
            "TextInputAction.next" => Some(TextInputAction::Next),
            "TextInputAction.previous" => Some(TextInputAction::Previous),
            "TextInputAction.continueAction" => Some(TextInputAction::ContinueAction),
            "TextInputAction.join" => Some(TextInputAction::Join),
            "TextInputAction.route" => Some(TextInputAction::Route),
            "TextInputAction.emergencyCall" => Some(TextInputAction::EmergencyCall),
            "TextInputAction.newline" => Some(TextInputAction::Newline),
            _ => None,
        }
    }
}

/// How a field asks to be edited.
#[derive(Clone, Copy, Debug, Default)]
pub struct TextInputConfiguration {
    pub input_type: TextInputType,
    pub action: TextInputAction,
    pub obscure_text: bool,
    pub autocorrect: bool,
}

impl TextInputConfiguration {
    fn to_value(self) -> Value {
        Value::map([
            (
                "inputType",
                Value::map([
                    ("name", Value::from(self.input_type.as_name())),
                    ("signed", Value::Null),
                    ("decimal", Value::Null),
                ]),
            ),
            ("inputAction", Value::from(self.action.as_name())),
            ("obscureText", Value::Bool(self.obscure_text)),
            ("autocorrect", Value::Bool(self.autocorrect)),
            ("enableSuggestions", Value::Bool(true)),
            ("enableDeltaModel", Value::Bool(false)),
            ("viewId", Value::I64(0)),
        ])
    }
}

/// What a field wants to hear about.
pub trait TextInputClient {
    /// The text is now this. Called for every keystroke and every step of a
    /// composition, so it must be cheap.
    fn update_editing_value(&mut self, value: TextEditingValue);

    /// The reader pressed Enter.
    fn perform_action(&mut self, _action: TextInputAction) {}
}

/// The channel, spelled once.
const CHANNEL: MethodChannel<JsonMethodCodec> =
    MethodChannel::new("flutter/textinput", JsonMethodCodec::new());

thread_local! {
    /// The field being edited, if any, and the id the platform knows it by.
    ///
    /// One at a time, which is not a simplification: focus is single, and
    /// upstream's `TextInput` likewise keeps one `_currentConnection`.
    static ATTACHED: std::cell::RefCell<Option<Attached>> =
        const { std::cell::RefCell::new(None) };

    /// The next client id to hand out. Ids are never reused, so an update
    /// arriving for a field that has since been detached is recognisable.
    static NEXT_ID: std::cell::Cell<i32> = const { std::cell::Cell::new(1) };
}

struct Attached {
    id: i32,
    client: Box<dyn TextInputClient>,
}

/// An open editing session. Closing it ends the platform's editing.
///
/// Deliberately not `Drop`-based: closing sends a message, and a message sent
/// from a destructor during teardown would reach a shell that is already gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextInputConnection {
    id: i32,
}

impl TextInputConnection {
    /// True while this connection is the one the platform is editing.
    ///
    /// A field that lost focus still holds its connection object; asking this
    /// is how it knows not to act on it.
    pub fn is_attached(&self) -> bool {
        ATTACHED.with(|attached| {
            attached.borrow().as_ref().is_some_and(|current| current.id == self.id)
        })
    }

    /// Asks the platform to start editing -- on a phone, to raise the keyboard.
    pub fn show(&self) {
        if self.is_attached() {
            CHANNEL.invoke("TextInput.show", Value::Null);
        }
    }

    pub fn hide(&self) {
        if self.is_attached() {
            CHANNEL.invoke("TextInput.hide", Value::Null);
        }
    }

    /// Tells the platform what the field now holds.
    ///
    /// Needed whenever the framework changes the text without the platform
    /// doing it: a paste, a clear button, a form being filled in. The platform
    /// keeps its own copy of the text -- it has to, to run the IME against it
    /// -- and this is what keeps the two the same.
    pub fn set_editing_state(&self, value: &TextEditingValue) {
        if self.is_attached() {
            CHANNEL.invoke("TextInput.setEditingState", value.to_state());
        }
    }

    /// Where the field is in the window, in physical pixels.
    ///
    /// Sent as a 4x4 transform because that is the channel's shape; only the
    /// translation is meaningful to an IME.
    pub fn set_editable_transform(&self, dx: f64, dy: f64) {
        if !self.is_attached() {
            return;
        }
        let mut matrix = vec![Value::F64(0.0); 16];
        matrix[0] = Value::F64(1.0);
        matrix[5] = Value::F64(1.0);
        matrix[10] = Value::F64(1.0);
        matrix[15] = Value::F64(1.0);
        matrix[12] = Value::F64(dx);
        matrix[13] = Value::F64(dy);
        CHANNEL.invoke(
            "TextInput.setEditableSizeAndTransform",
            Value::map([("transform", Value::List(matrix))]),
        );
    }

    /// Where the caret is inside the field, in the field's own coordinates.
    ///
    /// This plus the transform is where the candidate list goes.
    pub fn set_caret_rect(&self, x: f64, y: f64, width: f64, height: f64) {
        if !self.is_attached() {
            return;
        }
        CHANNEL.invoke(
            "TextInput.setMarkedTextRect",
            Value::map([
                ("x", Value::F64(x)),
                ("y", Value::F64(y)),
                ("width", Value::F64(width)),
                ("height", Value::F64(height)),
            ]),
        );
    }

    /// Ends editing. Anything the IME was composing is committed first, by the
    /// platform, and arrives as one last editing state.
    pub fn close(&self) {
        if !self.is_attached() {
            return;
        }
        CHANNEL.invoke("TextInput.clearClient", Value::Null);
        ATTACHED.with(|attached| *attached.borrow_mut() = None);
    }
}

/// Starts editing `client`, detaching whatever was being edited before.
///
/// Returns the connection. The field is expected to follow it with
/// [`TextInputConnection::set_editing_state`] and `show`, which is the order
/// upstream's `TextInput.attach` documents: the platform needs a client before
/// it has anywhere to put a state.
pub fn attach(
    client: Box<dyn TextInputClient>,
    configuration: TextInputConfiguration,
) -> TextInputConnection {
    // The previous field is told the platform has stopped editing it before the
    // new one takes over, so two fields are never both attached.
    let previous = ATTACHED.with(|attached| attached.borrow_mut().take());
    if previous.is_some() {
        CHANNEL.invoke("TextInput.clearClient", Value::Null);
    }

    let id = NEXT_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    });

    ensure_handler();
    ATTACHED.with(|attached| *attached.borrow_mut() = Some(Attached { id, client }));

    CHANNEL.invoke(
        "TextInput.setClient",
        Value::List(vec![Value::I64(id as i64), configuration.to_value()]),
    );
    TextInputConnection { id }
}

/// Registers the handler for what the platform sends back, once.
///
/// Once because registering again would drop whatever arrived and was buffered
/// -- and because the handler is stateless: it looks the client up each time
/// rather than capturing one.
fn ensure_handler() {
    if super::has_handler(CHANNEL.name()) {
        return;
    }
    CHANNEL.set_handler(|call, respond| {
        dispatch(&call);
        respond.success(Value::Null);
    });
}

fn dispatch(call: &MethodCall) {
    let arguments = call.arguments.as_list().unwrap_or(&[]);
    let Some(id) = arguments.first().and_then(Value::as_i64) else {
        return;
    };
    // A message for a field that has since been detached. Dropping it is the
    // point of the id: applying it would put one field's text into another.
    let current = ATTACHED.with(|attached| {
        attached.borrow().as_ref().map(|current| current.id)
    });
    if current != Some(id as i32) {
        return;
    }

    match call.method.as_str() {
        "TextInputClient.updateEditingState" => {
            let Some(state) = arguments.get(1) else { return };
            let Some(value) = TextEditingValue::from_state(state) else {
                return;
            };
            with_client(|client| client.update_editing_value(value.clone()));
        }
        "TextInputClient.performAction" => {
            // An action the framework has no variant for is not a "done" the
            // reader never pressed, so it is dropped here rather than mapped.
            let Some(action) = arguments
                .get(1)
                .and_then(Value::as_str)
                .and_then(TextInputAction::from_name)
            else {
                return;
            };
            with_client(|client| client.perform_action(action));
        }
        _ => {}
    }
}

/// Runs `body` against the attached client, with the client taken out.
///
/// Out because the client is entitled to attach a different field from inside
/// its own callback -- a form that moves to the next field on Enter does
/// exactly that -- and the cell cannot be borrowed twice.
fn with_client(body: impl FnOnce(&mut dyn TextInputClient)) {
    let Some(mut attached) = ATTACHED.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    body(attached.client.as_mut());
    ATTACHED.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            // Nothing replaced it while it ran, so it is still the field.
            *slot = Some(attached);
        }
    });
}

/// The channel, for an application that wants to speak it directly.
pub fn channel() -> MethodChannel<JsonMethodCodec> {
    CHANNEL
}

/// Whether anything is being edited. What a host would ask before deciding the
/// keyboard belongs to a shortcut rather than to a field.
pub fn is_editing() -> bool {
    ATTACHED.with(|attached| attached.borrow().is_some())
}

/// Forgets the attached field without telling the platform.
///
/// For tests, and for teardown, where sending would reach a shell that has
/// gone. [`TextInputConnection::close`] is what an application wants.
pub fn reset() {
    ATTACHED.with(|attached| *attached.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::super::codec::MethodCodec;
    use super::super::tests_support::install;
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Recorded {
        values: Vec<TextEditingValue>,
        actions: Vec<TextInputAction>,
    }

    struct Field(Rc<RefCell<Recorded>>);

    impl TextInputClient for Field {
        fn update_editing_value(&mut self, value: TextEditingValue) {
            self.0.borrow_mut().values.push(value);
        }

        fn perform_action(&mut self, action: TextInputAction) {
            self.0.borrow_mut().actions.push(action);
        }
    }

    fn attach_field() -> (Rc<RefCell<Recorded>>, TextInputConnection) {
        let recorded = Rc::new(RefCell::new(Recorded::default()));
        let connection = attach(
            Box::new(Field(recorded.clone())),
            TextInputConfiguration::default(),
        );
        (recorded, connection)
    }

    /// The state message the host sends, in the host's own shape.
    fn state_message(id: i32, text: &str, base: i32, extent: i32, composing: (i32, i32)) -> Vec<u8> {
        JsonMethodCodec
            .encode_method_call(&MethodCall::new(
                "TextInputClient.updateEditingState",
                Value::List(vec![
                    Value::I64(id as i64),
                    Value::map([
                        ("selectionAffinity", Value::from("TextAffinity.downstream")),
                        ("selectionBase", Value::I64(base as i64)),
                        ("selectionExtent", Value::I64(extent as i64)),
                        ("selectionIsDirectional", Value::Bool(false)),
                        ("composingBase", Value::I64(composing.0 as i64)),
                        ("composingExtent", Value::I64(composing.1 as i64)),
                        ("text", Value::from(text)),
                    ]),
                ]),
            ))
            .unwrap()
    }

    #[test]
    fn attaching_sends_the_client_and_its_configuration() {
        let recorder = install();
        reset();
        let (_recorded, _connection) = attach_field();

        let (channel, bytes, _) = recorder.sent().remove(0);
        assert_eq!(channel, "flutter/textinput");
        let call = JsonMethodCodec.decode_method_call(&bytes).unwrap();
        assert_eq!(call.method, "TextInput.setClient");

        let arguments = call.arguments.as_list().expect("a pair");
        assert_eq!(arguments.len(), 2, "the client id and its configuration");
        assert!(arguments[0].as_i64().is_some());
        assert_eq!(
            arguments[1].get("inputAction").and_then(Value::as_str),
            Some("TextInputAction.done")
        );
        assert_eq!(
            arguments[1]
                .get("inputType")
                .and_then(|t| t.get("name"))
                .and_then(Value::as_str),
            Some("TextInputType.text")
        );
    }

    #[test]
    fn an_editing_state_from_the_platform_reaches_the_field() {
        let recorder = install();
        reset();
        let (recorded, connection) = attach_field();
        let id = connection.id;

        recorder.deliver("flutter/textinput", &state_message(id, "hello", 5, 5, (-1, -1)), 0);

        let values = &recorded.borrow().values;
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].text, "hello");
        assert_eq!(values[0].selection_extent, 5);
        assert!(!values[0].is_composing());
    }

    #[test]
    fn a_composing_run_survives_the_trip() {
        // The IME's half-typed word. It is part of the text and it is
        // underlined rather than committed, which is the whole reason the
        // range travels separately.
        let recorder = install();
        reset();
        let (recorded, connection) = attach_field();

        recorder.deliver(
            "flutter/textinput",
            &state_message(connection.id, "zhong", 5, 5, (0, 5)),
            0,
        );

        let values = &recorded.borrow().values;
        assert!(values[0].is_composing());
        assert_eq!(values[0].composing_bytes(), Some(0..5));
    }

    #[test]
    fn a_stale_update_for_a_detached_field_is_dropped() {
        // Focus moved. An update in flight for the old field must not be
        // applied to the new one, which is what the client id is for.
        let recorder = install();
        reset();
        let (first, first_connection) = attach_field();
        let (second, _second_connection) = attach_field();

        recorder.deliver(
            "flutter/textinput",
            &state_message(first_connection.id, "stale", 5, 5, (-1, -1)),
            0,
        );

        assert!(first.borrow().values.is_empty());
        assert!(second.borrow().values.is_empty());
    }

    #[test]
    fn attaching_a_second_field_detaches_the_first() {
        let recorder = install();
        reset();
        let (_first, first_connection) = attach_field();
        let (_second, second_connection) = attach_field();

        assert!(!first_connection.is_attached());
        assert!(second_connection.is_attached());

        let methods: Vec<String> = recorder
            .sent()
            .iter()
            .map(|(_, bytes, _)| JsonMethodCodec.decode_method_call(bytes).unwrap().method)
            .collect();
        assert_eq!(
            methods,
            vec!["TextInput.setClient", "TextInput.clearClient", "TextInput.setClient"]
        );
    }

    #[test]
    fn an_action_reaches_the_field() {
        let recorder = install();
        reset();
        let (recorded, connection) = attach_field();

        let call = JsonMethodCodec
            .encode_method_call(&MethodCall::new(
                "TextInputClient.performAction",
                Value::List(vec![
                    Value::I64(connection.id as i64),
                    Value::from("TextInputAction.done"),
                ]),
            ))
            .unwrap();
        recorder.deliver("flutter/textinput", &call, 0);

        assert_eq!(recorded.borrow().actions, vec![TextInputAction::Done]);
    }

    #[test]
    fn every_upstream_action_name_round_trips_and_unknown_ones_do_not() {
        // The names upstream's `_TextInputActionEnumMapper` knows, exactly.
        // A name it does not know is `None` rather than a quiet `done`: a
        // coercion there would report to a field that the reader pressed
        // Enter when the platform sent something the framework has no word
        // for.
        let pairs = [
            ("TextInputAction.none", TextInputAction::None),
            ("TextInputAction.unspecified", TextInputAction::Unspecified),
            ("TextInputAction.done", TextInputAction::Done),
            ("TextInputAction.go", TextInputAction::Go),
            ("TextInputAction.search", TextInputAction::Search),
            ("TextInputAction.send", TextInputAction::Send),
            ("TextInputAction.next", TextInputAction::Next),
            ("TextInputAction.previous", TextInputAction::Previous),
            ("TextInputAction.continueAction", TextInputAction::ContinueAction),
            ("TextInputAction.join", TextInputAction::Join),
            ("TextInputAction.route", TextInputAction::Route),
            ("TextInputAction.emergencyCall", TextInputAction::EmergencyCall),
            ("TextInputAction.newline", TextInputAction::Newline),
        ];
        for (name, action) in pairs {
            assert_eq!(action.as_name(), name);
            assert_eq!(TextInputAction::from_name(name), Some(action));
        }
        assert_eq!(TextInputAction::from_name("TextInputAction.date"), None);
        assert_eq!(TextInputAction::from_name("TextInputAction.call"), None);
        assert_eq!(TextInputAction::from_name(""), None);
        assert_eq!(TextInputAction::from_name("done"), None);
    }

    #[test]
    fn an_unknown_action_name_is_dropped_rather_than_become_done() {
        // What the field is spared: a "done" it was never sent.
        let recorder = install();
        reset();
        let (recorded, connection) = attach_field();

        let call = JsonMethodCodec
            .encode_method_call(&MethodCall::new(
                "TextInputClient.performAction",
                Value::List(vec![
                    Value::I64(connection.id as i64),
                    Value::from("TextInputAction.date"),
                ]),
            ))
            .unwrap();
        recorder.deliver("flutter/textinput", &call, 0);

        assert!(recorded.borrow().actions.is_empty());
    }

    #[test]
    fn every_upstream_input_type_name_is_the_one_the_platform_expects() {
        let pairs = [
            (TextInputType::Text, "TextInputType.text"),
            (TextInputType::Multiline, "TextInputType.multiline"),
            (TextInputType::Number, "TextInputType.number"),
            (TextInputType::Phone, "TextInputType.phone"),
            (TextInputType::Datetime, "TextInputType.datetime"),
            (TextInputType::Email, "TextInputType.emailAddress"),
            (TextInputType::Url, "TextInputType.url"),
            (TextInputType::VisiblePassword, "TextInputType.visiblePassword"),
            (TextInputType::Name, "TextInputType.name"),
            // Upstream's `_names` table says "address", not "streetAddress".
            (TextInputType::StreetAddress, "TextInputType.address"),
            (TextInputType::None, "TextInputType.none"),
            (TextInputType::WebSearch, "TextInputType.webSearch"),
            (TextInputType::Twitter, "TextInputType.twitter"),
        ];
        for (input_type, name) in pairs {
            assert_eq!(input_type.as_name(), name);
        }
    }

    #[test]
    fn closing_stops_editing_and_says_so() {
        let recorder = install();
        reset();
        let (recorded, connection) = attach_field();
        connection.close();
        assert!(!connection.is_attached());
        assert!(!is_editing());

        let methods: Vec<String> = recorder
            .sent()
            .iter()
            .map(|(_, bytes, _)| JsonMethodCodec.decode_method_call(bytes).unwrap().method)
            .collect();
        assert_eq!(methods, vec!["TextInput.setClient", "TextInput.clearClient"]);

        // Anything still in flight is for a field nobody is editing.
        recorder.deliver(
            "flutter/textinput",
            &state_message(connection.id, "late", 4, 4, (-1, -1)),
            0,
        );
        assert!(recorded.borrow().values.is_empty());
    }

    #[test]
    fn a_field_can_hand_over_to_another_from_inside_its_own_callback() {
        // A form that moves to the next field on Enter. The client is out of
        // the cell while it runs, so this is the case a naive borrow breaks.
        let recorder = install();
        reset();
        let (_first, connection) = attach_field();

        struct Handover;
        impl TextInputClient for Handover {
            fn update_editing_value(&mut self, _value: TextEditingValue) {}
            fn perform_action(&mut self, _action: TextInputAction) {
                attach(
                    Box::new(Field(Rc::new(RefCell::new(Recorded::default())))),
                    TextInputConfiguration::default(),
                );
            }
        }

        let next = attach(Box::new(Handover), TextInputConfiguration::default());
        let _ = connection;

        let call = JsonMethodCodec
            .encode_method_call(&MethodCall::new(
                "TextInputClient.performAction",
                Value::List(vec![Value::I64(next.id as i64), Value::from("TextInputAction.next")]),
            ))
            .unwrap();
        recorder.deliver("flutter/textinput", &call, 0);

        assert!(!next.is_attached(), "the handler attached somebody else");
        assert!(is_editing());
    }

    #[test]
    fn utf16_offsets_are_counted_the_way_the_platform_counts_them() {
        // The platform counts UTF-16 code units. A framework counting bytes or
        // chars disagrees with it the first time an emoji is typed -- and the
        // caret lands in the wrong place, or inside a character.
        let value = TextEditingValue {
            text: "a\u{1F600}b".to_string(),
            selection_base: 3,
            selection_extent: 3,
            composing_base: -1,
            composing_extent: -1,
        };
        // 'a' is one unit, the emoji is two, so offset 3 is just before 'b'.
        assert_eq!(value.caret_bytes(), Some(5));

        // An offset landing inside the surrogate pair is not a position.
        let inside = TextEditingValue { selection_extent: 2, ..value.clone() };
        assert_eq!(inside.caret_bytes(), None);

        assert_eq!(TextEditingValue::new("abc").selection_extent, 3);
        assert_eq!(TextEditingValue::new("\u{1F600}").selection_extent, 2);
    }
}
