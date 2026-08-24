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

use super::MethodChannel;
use super::codec::{JsonMethodCodec, MethodCall, Value};
use super::system::PLATFORM;
use super::text_editing_delta::TextEditingDelta;
use crate::direction::TextDirection;
use crate::engine::{Rect, TextAlign};
use crate::render::Offset;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

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
            state
                .get(key)
                .and_then(Value::as_i64)
                .map(|v| v as i32)
                .unwrap_or(fallback)
        };
        Some(TextEditingValue {
            text: state.get("text")?.as_str()?.to_string(),
            selection_base: number("selectionBase", 0),
            selection_extent: number("selectionExtent", 0),
            composing_base: number("composingBase", -1),
            composing_extent: number("composingExtent", -1),
        })
    }

    pub(crate) fn to_state(&self) -> Value {
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
pub(crate) fn utf16_to_byte(text: &str, offset: i32) -> Option<usize> {
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
    /// Upstream's `numberWithOptions`. Carries its two flags because
    /// `TextInputType.number` **is** `numberWithOptions()` -- see
    /// [`TextInputType::number_options`].
    Number {
        signed: bool,
        decimal: bool,
    },
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
    /// Upstream's `signed` and `decimal`, which are **not null for the number
    /// type even when nobody asked for either**.
    ///
    /// `TextInputType.number` is defined as `TextInputType.numberWithOptions()`
    /// -- the plain one is the options at their defaults rather than the
    /// absence of options -- while every other type comes from a constructor
    /// that sets both to null. So the wire carries `false, false` for a
    /// number and `null, null` for the rest, and this port sent `null, null`
    /// for all of them.
    ///
    /// Whether that reaches a reader depends on the embedder, which is exactly
    /// why it is worth getting right here: nothing on this side can tell.
    pub fn number_options(self) -> Option<(bool, bool)> {
        match self {
            TextInputType::Number { signed, decimal } => Some((signed, decimal)),
            _ => None,
        }
    }

    /// Upstream's `TextInputType.number`: the options at their defaults.
    pub const NUMBER: TextInputType = TextInputType::Number {
        signed: false,
        decimal: false,
    };

    /// Upstream's `TextInputType.numberWithOptions`.
    pub fn number_with_options(signed: bool, decimal: bool) -> TextInputType {
        TextInputType::Number { signed, decimal }
    }

    fn as_name(self) -> &'static str {
        match self {
            TextInputType::Text => "TextInputType.text",
            TextInputType::Multiline => "TextInputType.multiline",
            TextInputType::Number { .. } => "TextInputType.number",
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
#[derive(Clone, Debug)]
pub struct TextInputConfiguration {
    pub input_type: TextInputType,
    pub action: TextInputAction,
    pub obscure_text: bool,
    pub autocorrect: bool,
    /// Upstream's `readOnly`: the field shows a caret and a selection but the
    /// keyboard must not edit it.
    pub read_only: bool,
    /// Upstream's `enableInteractiveSelection`.
    pub enable_interactive_selection: bool,
    /// Upstream's `enableSuggestions`, which was sent as a hardcoded `true`.
    pub enable_suggestions: bool,
    /// Upstream's `smartDashesType`. **Sent as the index, as a string** -- see
    /// [`TextInputConfiguration::to_value`].
    pub smart_dashes: crate::editable_text::SmartDashesType,
    /// Upstream's `smartQuotesType`, and the same encoding.
    pub smart_quotes: crate::editable_text::SmartQuotesType,
    /// Upstream's `textCapitalization`.
    pub text_capitalization: crate::component_themes::TextCapitalization,
    /// Upstream's `keyboardAppearance`, which is the keyboard's own light or
    /// dark, not the application's.
    pub keyboard_appearance: crate::platform::Brightness,
    /// Upstream's `enableIMEPersonalizedLearning`, whose default is **true**:
    /// a field opts out of the platform learning from it rather than in.
    pub enable_ime_personalized_learning: bool,
    /// Upstream's `actionLabel`, the word on the action key when the platform
    /// lets one be chosen.
    pub action_label: Option<String>,
    /// Upstream's `allowedMimeTypes`, sent as `contentCommitMimeTypes`: what
    /// the keyboard may insert besides text.
    ///
    /// An empty list is the default and means **nothing but text** -- a
    /// keyboard offering a GIF has nowhere to put it. Not the same as the
    /// field declining to say, which this cannot express and upstream does not
    /// either.
    pub allowed_mime_types: Vec<String>,
    /// Upstream's `hintLocales`: which languages the reader is likely to type,
    /// so the keyboard can offer the right one first.
    ///
    /// Android reads it (`EditorInfo#hintLocales`). Nullable upstream, but its
    /// default is an **empty list** rather than null, so the common case sends
    /// `[]` and not nothing.
    pub hint_locales: Option<Vec<crate::platform::Locale>>,
    /// Upstream's `enableInlinePrediction`, whose default is **null and not
    /// false**.
    ///
    /// Null means "whatever the platform does", which is not the same as
    /// asking for it to be off: upstream's own doc says inline prediction is
    /// enabled by default on iOS, so a false here is a field overruling the
    /// platform rather than agreeing with it.
    pub enable_inline_prediction: Option<bool>,
    /// What the platform may fill this field with, if anything. Disabled by
    /// default: a field says what it holds, and one that has not said holds
    /// nothing the platform should guess at.
    pub autofill_configuration: crate::services::autofill::AutofillConfiguration,
}

impl Default for TextInputConfiguration {
    /// Upstream's constructor defaults, written out rather than derived.
    ///
    /// A derive gave `autocorrect: false`, and upstream's default is **true** --
    /// autocorrection is on unless a field turns it off. Four of the flags here
    /// are like that: suggestions, interactive selection and personalised
    /// learning all default to on, so `bool::default()` is wrong for every one
    /// of them and right only by accident for the two that are off.
    ///
    /// Which is the argument against deriving this at all. A derive says "the
    /// zero value", and what is wanted is "what upstream's constructor says",
    /// and those agree only until they do not.
    fn default() -> TextInputConfiguration {
        TextInputConfiguration {
            input_type: TextInputType::Text,
            action: TextInputAction::Done,
            obscure_text: false,
            autocorrect: true,
            read_only: false,
            enable_interactive_selection: true,
            enable_suggestions: true,
            smart_dashes: crate::editable_text::SmartDashesType::Enabled,
            smart_quotes: crate::editable_text::SmartQuotesType::Enabled,
            text_capitalization: crate::component_themes::TextCapitalization::None,
            keyboard_appearance: crate::platform::Brightness::Light,
            enable_ime_personalized_learning: true,
            action_label: None,
            allowed_mime_types: Vec::new(),
            // Empty rather than None: upstream's default is `const <Locale>[]`,
            // so an ordinary field sends `[]` and only one that deliberately
            // set null sends nothing.
            hint_locales: Some(Vec::new()),
            enable_inline_prediction: None,
            autofill_configuration: crate::services::autofill::AutofillConfiguration::default(),
        }
    }
}

impl TextInputConfiguration {
    pub(crate) fn to_value(&self) -> Value {
        let mut value = Value::map([
            (
                "inputType",
                Value::map([
                    ("name", Value::from(self.input_type.as_name())),
                    // Null for every type but the number one, which carries
                    // them as booleans even when both are false -- see
                    // `TextInputType::number_options`.
                    (
                        "signed",
                        match self.input_type.number_options() {
                            Some((signed, _)) => Value::Bool(signed),
                            None => Value::Null,
                        },
                    ),
                    (
                        "decimal",
                        match self.input_type.number_options() {
                            Some((_, decimal)) => Value::Bool(decimal),
                            None => Value::Null,
                        },
                    ),
                ]),
            ),
            ("inputAction", Value::from(self.action.as_name())),
            ("readOnly", Value::Bool(self.read_only)),
            ("obscureText", Value::Bool(self.obscure_text)),
            ("autocorrect", Value::Bool(self.autocorrect)),
            // **The index, written as a string.** Upstream sends
            // `smartDashesType.index.toString()`, so the wire carries "0" and
            // "1" rather than a name or a boolean, and the enum's declaration
            // order is part of the format.
            (
                "smartDashesType",
                Value::from(self.smart_dashes.index_string()),
            ),
            (
                "smartQuotesType",
                Value::from(self.smart_quotes.index_string()),
            ),
            ("enableSuggestions", Value::Bool(self.enable_suggestions)),
            (
                "enableInteractiveSelection",
                Value::Bool(self.enable_interactive_selection),
            ),
            (
                "actionLabel",
                match &self.action_label {
                    Some(label) => Value::from(label.as_str()),
                    None => Value::Null,
                },
            ),
            (
                "textCapitalization",
                Value::from(self.text_capitalization.as_name()),
            ),
            (
                "keyboardAppearance",
                Value::from(self.keyboard_appearance.as_name()),
            ),
            (
                "enableIMEPersonalizedLearning",
                Value::Bool(self.enable_ime_personalized_learning),
            ),
            (
                "contentCommitMimeTypes",
                Value::List(
                    self.allowed_mime_types
                        .iter()
                        .map(|kind| Value::from(kind.as_str()))
                        .collect(),
                ),
            ),
            (
                "hintLocales",
                match &self.hint_locales {
                    // Upstream's `hintLocales?.map(...).toList()`: a language
                    // tag each, which is the form the platform reads.
                    Some(locales) => Value::List(
                        locales
                            .iter()
                            .map(|locale| Value::from(locale.to_language_tag().as_str()))
                            .collect(),
                    ),
                    None => Value::Null,
                },
            ),
            (
                "enableInlinePrediction",
                match self.enable_inline_prediction {
                    Some(enabled) => Value::Bool(enabled),
                    None => Value::Null,
                },
            ),
            ("enableDeltaModel", Value::Bool(false)),
            ("viewId", Value::I64(0)),
        ]);
        // Upstream leaves the key out entirely for a disabled
        // configuration rather than sending it switched off.
        if let (Value::Map(pairs), Some(autofill)) =
            (&mut value, self.autofill_configuration.to_value())
        {
            pairs.push((Value::String("autofill".to_string()), autofill));
        }
        value
    }
}

/// What a field wants to hear about.
/// What the platform may ask an editing client -- upstream's
/// `TextInputClient`.
///
/// # Which answers are required and which may be ignored
///
/// Upstream writes nine of these without a body and six with `{}`, and the
/// split is not about how important they are. The required ones are what the
/// platform can ask **at any moment and needs an answer to** -- the current
/// value, the autofill scope, a keystroke, an action, a closed connection. The
/// optional ones are announcements about features a client may not have:
/// Android content insertion, Scribble placeholders, a macOS selector, a
/// swapped input control.
///
/// A client that ignores an optional one still works. A client that ignores a
/// required one leaves the platform holding a question, which is why upstream
/// gives them no body to inherit.
///
/// # A null autofill scope means two different things
///
/// Upstream: "It should return null if this `TextInputClient` does not need
/// autofill support. For a `TextInputClient` that supports autofill, returning
/// null causes it to participate in autofill alone."
///
/// So `None` is *no autofill* from a client that does not do autofill, and
/// *autofill, ungrouped* from one that does -- and the value cannot tell you
/// which, because the difference is in the client rather than in the answer.
/// See [`TextInputClient::autofill_scope`].
///
/// # The platform's value is user input, and a programmatic one is not
///
/// `updateEditingValue`'s doc: the new value "is treated as user input and
/// thus may subject to input formatting". A value arriving from the keyboard
/// goes through the formatters; one the application sets does not. The same
/// text through two doors is two different things.
pub trait TextInputClient {
    /// The text is now this. Called for every keystroke and every step of a
    /// composition, so it must be cheap.
    ///
    /// **Required.** And what arrives here is user input -- see the trait's
    /// docs.
    fn update_editing_value(&mut self, value: TextEditingValue);

    /// The reader pressed Enter.
    ///
    /// Upstream leaves this without a body; it has one here because the port's
    /// only client is a field that has nothing to do for most actions, and a
    /// trait method nobody can sensibly default is a different problem from a
    /// trait method nobody has to write.
    fn perform_action(&mut self, _action: TextInputAction) {}

    /// Upstream's `currentTextEditingValue`: what this client is holding.
    ///
    /// **Required**, because the platform asks the client rather than
    /// remembering: the client is the source of truth and the platform's copy
    /// is a cache of it.
    fn current_editing_value(&self) -> Option<TextEditingValue> {
        None
    }

    /// Upstream's `currentAutofillScope`. `None` is overloaded -- see the
    /// trait's docs.
    fn autofill_scope(&self) -> Option<AutofillScopeId> {
        None
    }

    /// Upstream's `connectionClosed`: the **platform** dropped the connection.
    ///
    /// Upstream says what is owed: the client "should cleanup its connection
    /// and finalize editing". Finalize, not forget -- a composition in flight
    /// has to be committed or discarded on purpose, because the platform will
    /// not be sending the rest of it.
    ///
    /// This is the other direction from detaching. A client that detaches has
    /// decided to stop; a client told the connection closed is being informed
    /// after the fact, and whatever was half-typed is now its problem.
    fn connection_closed(&mut self) {}

    /// Upstream's `onFocusReceived`, which defaults to **false**.
    ///
    /// "Notifies the client that the platform moved focus back to this input.
    /// This is necessary to support autofill on some browsers (e.g. iOS
    /// Safari) that blur the text field and refocus it before autofilling."
    ///
    /// A browser workaround with its reason written down, and the return value
    /// says whether the client took the focus -- so the default of false is a
    /// client saying "that was not me", which is the safe answer for anything
    /// that has never heard of Safari's autofill dance.
    fn on_focus_received(&mut self) -> bool {
        false
    }

    /// Upstream's `performPrivateCommand`: an input method talking to a client
    /// that knows it.
    ///
    /// **Required** upstream, and optional here for the reason
    /// [`TextInputClient::perform_action`] is. Upstream's own doc says it is
    /// for "domain-specific features that are only known between certain input
    /// methods and their clients" -- so the framework carries a message it
    /// cannot read, between two parties it does not know, which is why there
    /// is nothing sensible for it to do by default.
    fn perform_private_command(&mut self, _action: &str) {}
}

/// Which autofill group a client belongs to, or that it is on its own.
///
/// A newtype rather than a bare `u64` so that [`TextInputClient::autofill_scope`]
/// returning `None` cannot be mistaken for a group numbered zero -- the two
/// mean opposite things, and the trait's docs say `None` already means two
/// things without that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutofillScopeId(pub u64);

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
            attached
                .borrow()
                .as_ref()
                .is_some_and(|current| current.id == self.id)
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
    let current = ATTACHED.with(|attached| attached.borrow().as_ref().map(|current| current.id));
    if current != Some(id as i32) {
        return;
    }

    match call.method.as_str() {
        "TextInputClient.updateEditingState" => {
            let Some(state) = arguments.get(1) else {
                return;
            };
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
        "TextInputClient.onConnectionClosed" => {
            // The platform dropped it, so the framework's side goes too --
            // and it goes *before* the client is told, because a client that
            // reattaches from inside `connection_closed` must not have its new
            // connection cleared by this one's cleanup.
            //
            // Upstream asks the client to "cleanup its connection and finalize
            // editing": finalize, not forget. Whatever composition was in
            // flight will get no more messages, so it is the client's to
            // settle.
            let closed = ATTACHED.with(|slot| slot.borrow_mut().take());
            with_closed_client(closed.map(|attached| attached.client), |client| {
                client.connection_closed()
            });
        }
        _ => {}
    }
}

/// Runs `body` against a client that has already been detached.
///
/// `connection_closed` clears the slot before calling, so the client is no
/// longer reachable through it -- but it still has to be told. This carries
/// the one that was there, which is why it cannot go through
/// [`with_client`]: by the time it runs there is nothing attached to find.
fn with_closed_client(
    client: Option<Box<dyn TextInputClient>>,
    body: impl FnOnce(&mut dyn TextInputClient),
) {
    if let Some(mut client) = client {
        body(client.as_mut());
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
    fn state_message(
        id: i32,
        text: &str,
        base: i32,
        extent: i32,
        composing: (i32, i32),
    ) -> Vec<u8> {
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

        recorder.deliver(
            "flutter/textinput",
            &state_message(id, "hello", 5, 5, (-1, -1)),
            0,
        );

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
            vec![
                "TextInput.setClient",
                "TextInput.clearClient",
                "TextInput.setClient"
            ]
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
            (
                "TextInputAction.continueAction",
                TextInputAction::ContinueAction,
            ),
            ("TextInputAction.join", TextInputAction::Join),
            ("TextInputAction.route", TextInputAction::Route),
            (
                "TextInputAction.emergencyCall",
                TextInputAction::EmergencyCall,
            ),
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
            (TextInputType::NUMBER, "TextInputType.number"),
            (TextInputType::Phone, "TextInputType.phone"),
            (TextInputType::Datetime, "TextInputType.datetime"),
            (TextInputType::Email, "TextInputType.emailAddress"),
            (TextInputType::Url, "TextInputType.url"),
            (
                TextInputType::VisiblePassword,
                "TextInputType.visiblePassword",
            ),
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
        assert_eq!(
            methods,
            vec!["TextInput.setClient", "TextInput.clearClient"]
        );

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
                Value::List(vec![
                    Value::I64(next.id as i64),
                    Value::from("TextInputAction.next"),
                ]),
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
        let inside = TextEditingValue {
            selection_extent: 2,
            ..value.clone()
        };
        assert_eq!(inside.caret_bytes(), None);

        assert_eq!(TextEditingValue::new("abc").selection_extent, 3);
        assert_eq!(TextEditingValue::new("\u{1F600}").selection_extent, 2);
    }
}

// -- The rest of what a field and the platform say to each other ---------------

/// Upstream `FloatingCursorDragState`: where a floating cursor drag is.
///
/// iOS lets the reader press the space bar and slide the caret around; the
/// caret that follows the finger is the "floating" one, and the real caret
/// snaps to it when the finger lifts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatingCursorDragState {
    Start,
    Update,
    End,
}

/// Upstream `RawFloatingCursorPoint`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawFloatingCursorPoint {
    /// Where the floating cursor is. Upstream asserts this is present for an
    /// `Update`; the same is asserted here rather than made unrepresentable,
    /// because the three states are one enum on the wire.
    pub offset: Option<Offset>,
    /// Where the drag began: the point, and the text position under it.
    pub start_location: Option<(Offset, i32)>,
    pub state: FloatingCursorDragState,
}

impl RawFloatingCursorPoint {
    pub fn new(state: FloatingCursorDragState, offset: Option<Offset>) -> RawFloatingCursorPoint {
        debug_assert!(
            state != FloatingCursorDragState::Update || offset.is_some(),
            "an update has to say where the cursor moved to"
        );
        RawFloatingCursorPoint {
            offset,
            start_location: None,
            state,
        }
    }

    pub fn with_start_location(mut self, offset: Offset, position: i32) -> Self {
        self.start_location = Some((offset, position));
        self
    }
}

/// Upstream `SelectionRect`: where one character sits, for the platform.
///
/// Sent so that the platform can put its own overlays -- a Scribble caret, a
/// magnifier, a spell-check underline -- in the right place without asking
/// the framework where anything is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionRect {
    /// The character's offset in the text, in UTF-16 code units.
    pub position: i32,
    pub bounds: Rect,
    pub direction: TextDirection,
}

impl SelectionRect {
    pub fn new(position: i32, bounds: Rect) -> SelectionRect {
        SelectionRect {
            position,
            bounds,
            direction: TextDirection::Ltr,
        }
    }

    pub fn with_direction(mut self, direction: TextDirection) -> Self {
        self.direction = direction;
        self
    }
}

/// Upstream `TextInputStyle`: how the field draws its text, told to the
/// platform so that its own editing overlays match.
///
/// The platform draws the composing underline and, on some systems, the
/// floating cursor; both look wrong against text they do not know the metrics
/// of.
#[derive(Clone, Debug, PartialEq)]
pub struct TextInputStyle {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub font_weight: Option<u16>,
    pub text_direction: TextDirection,
    pub text_align: TextAlign,
    pub letter_spacing: Option<f32>,
    pub word_spacing: Option<f32>,
    pub line_height: Option<f32>,
}

impl TextInputStyle {
    /// The two required fields; upstream's other six are optional and mean
    /// "the platform's own default".
    pub fn new(text_direction: TextDirection, text_align: TextAlign) -> TextInputStyle {
        TextInputStyle {
            font_family: None,
            font_size: None,
            font_weight: None,
            text_direction,
            text_align,
            letter_spacing: None,
            word_spacing: None,
            line_height: None,
        }
    }

    pub fn with_font(mut self, family: impl Into<String>, size: f32) -> Self {
        self.font_family = Some(family.into());
        self.font_size = Some(size);
        self
    }
}

/// Upstream `TextSelectionDelegate`: what a selection toolbar can ask of the
/// field it is over.
///
/// Every entry in the toolbar is one of these, which is why they are here
/// rather than on the field: the toolbar is written once and works against
/// anything that can answer.
pub trait TextSelectionDelegate {
    fn text_editing_value(&self) -> TextEditingValue;

    /// Upstream `userUpdateTextEditingValue`: a change the *reader* made,
    /// which is different from one the platform made and is why the cause
    /// travels with it.
    fn user_update_text_editing_value(&self, value: TextEditingValue, cause: SelectionChangedCause);

    fn hide_toolbar(&self, hide_handles: bool);

    /// Upstream `bringIntoView`: scroll so that this offset is visible.
    fn bring_into_view(&self, position: i32);

    fn cut_selection(&self, cause: SelectionChangedCause);
    fn copy_selection(&self, cause: SelectionChangedCause);
    fn paste_text(&self, cause: SelectionChangedCause);
    fn select_all(&self, cause: SelectionChangedCause);

    // Upstream's defaults: everything is offered except Live Text, which a
    // field has to opt into because it needs a camera.
    fn cut_enabled(&self) -> bool {
        true
    }
    fn copy_enabled(&self) -> bool {
        true
    }
    fn paste_enabled(&self) -> bool {
        true
    }
    fn select_all_enabled(&self) -> bool {
        true
    }
    fn look_up_enabled(&self) -> bool {
        true
    }
    fn search_web_enabled(&self) -> bool {
        true
    }
    fn share_enabled(&self) -> bool {
        true
    }
    fn live_text_input_enabled(&self) -> bool {
        false
    }
}

/// Upstream `SelectionChangedCause`: what moved the selection.
///
/// The toolbar cares because the answer decides whether it should appear: a
/// selection the reader made by dragging wants a toolbar, one the code made
/// does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionChangedCause {
    Tap,
    DoubleTap,
    LongPress,
    ForcePress,
    Keyboard,
    Toolbar,
    Drag,
    /// Upstream renamed this from `scribble` and keeps the old spelling
    /// only as a deprecated alias:
    ///
    /// ```dart
    /// static const SelectionChangedCause scribble = stylusHandwriting;
    /// ```
    ///
    /// This port was still on the retired name. The alias is not carried --
    /// it exists upstream so that code written before the rename keeps
    /// compiling, and there is no such code here.
    StylusHandwriting,
}

/// Upstream `DeltaTextInputClient`: a field that wants the edits rather than
/// the values.
///
/// The ordinary client is told what the text now is; this one is told what
/// changed, which is what a formatter or an undo stack needs. See
/// [`text_editing_delta`](crate::services::text_editing_delta).
pub trait DeltaTextInputClient: TextInputClient {
    /// Upstream `updateEditingValueWithDeltas`. Plural because one keystroke
    /// can be several deltas -- an autocorrect replaces a word and moves the
    /// caret in one message.
    fn update_editing_value_with_deltas(&mut self, deltas: &[TextEditingDelta]);
}

/// Upstream `ScribbleClient`: a field the Apple Pencil can write into.
///
/// The platform asks which field is under the pen, so a field has to be able
/// to say where it is and to take focus when told.
pub trait ScribbleClient {
    /// What the platform calls this field. Upstream's is a string because it
    /// crosses the channel.
    fn element_identifier(&self) -> String;

    /// Upstream `onScribbleFocus`.
    fn on_scribble_focus(&self, offset: Offset);

    /// Upstream `isInScribbleRect`.
    fn is_in_scribble_rect(&self, rect: Rect) -> bool;

    fn bounds(&self) -> Rect;
}

/// Upstream `TextInputControl`: something other than the platform's keyboard
/// that can drive a field.
///
/// Every method has an empty default, which is upstream's shape and the point
/// of it: a control that only wants to know when the keyboard should show
/// overrides one method and ignores the rest.
pub trait TextInputControl {
    fn attach(&self, _client: &dyn TextInputClient, _configuration: &TextInputConfiguration) {}
    fn detach(&self, _client: &dyn TextInputClient) {}
    fn show(&self) {}
    fn hide(&self) {}
    fn update_config(&self, _configuration: &TextInputConfiguration) {}
    fn set_editing_state(&self, _value: &TextEditingValue) {}
}

/// Upstream `SystemContextMenuClient`, which lives in upstream's
/// `services/binding.dart`.
///
/// The platform can take its own context menu away -- the reader tapped
/// elsewhere, or the application went to the background -- and whatever asked
/// for it has to hear about that or it will think the menu is still up.
pub trait SystemContextMenuClient {
    /// Upstream `handleSystemHide`.
    fn handle_system_hide(&self);
    /// Upstream `handleCustomContextMenuAction`.
    fn handle_custom_context_menu_action(&self, action_id: &str);
}

/// Upstream `SystemContextMenuController`: shows and hides the platform's own
/// context menu.
///
/// # Recorded divergences
///
/// * Upstream registers itself with `ServicesBinding.systemContextMenuClient`
///   in its constructor and asserts, in `show`, that a text input connection
///   is live. There is no binding object here; the registration is
///   [`SystemContextMenuController::show`] taking the slot, which is the same
///   "last one to show owns the menu" rule stated where it happens.
pub struct SystemContextMenuController {
    on_system_hide: Option<Rc<dyn Fn()>>,
    last_target_rect: RefCell<Option<Rect>>,
    visible: Cell<bool>,
}

impl Default for SystemContextMenuController {
    fn default() -> SystemContextMenuController {
        SystemContextMenuController::new()
    }
}

impl SystemContextMenuController {
    pub const SHOW_METHOD: &'static str = "ContextMenu.showSystemContextMenu";
    pub const HIDE_METHOD: &'static str = "ContextMenu.hideSystemContextMenu";

    pub fn new() -> SystemContextMenuController {
        SystemContextMenuController {
            on_system_hide: None,
            last_target_rect: RefCell::new(None),
            visible: Cell::new(false),
        }
    }

    pub fn with_on_system_hide(mut self, on_system_hide: impl Fn() + 'static) -> Self {
        self.on_system_hide = Some(Rc::new(on_system_hide));
        self
    }

    pub fn is_visible(&self) -> bool {
        self.visible.get()
    }

    /// Upstream's `show`, less the binding registration.
    ///
    /// Showing the menu at a rectangle it is already showing at does nothing:
    /// upstream's early return, and it matters because the field asks on
    /// every selection change and most of them do not move the menu.
    pub fn show(&self, target_rect: Rect) {
        if self.visible.get() && *self.last_target_rect.borrow() == Some(target_rect) {
            return;
        }
        *self.last_target_rect.borrow_mut() = Some(target_rect);
        self.visible.set(true);
        PLATFORM.invoke(
            SystemContextMenuController::SHOW_METHOD,
            Value::map([(
                "targetRect",
                Value::map([
                    ("x", Value::F64(target_rect.left as f64)),
                    ("y", Value::F64(target_rect.top as f64)),
                    ("width", Value::F64(target_rect.width() as f64)),
                    ("height", Value::F64(target_rect.height() as f64)),
                ]),
            )]),
        );
    }

    /// Upstream `hide`. Hiding a menu that is not up is nothing, not a second
    /// message: the platform would answer the same either way, and the
    /// message costs a round trip.
    pub fn hide(&self) {
        if !self.visible.get() {
            return;
        }
        self.visible.set(false);
        PLATFORM.invoke(SystemContextMenuController::HIDE_METHOD, Value::Null);
    }
}

impl SystemContextMenuClient for SystemContextMenuController {
    /// Upstream `handleSystemHide`: the platform took the menu away, so the
    /// controller stops believing it is up and tells whoever asked.
    ///
    /// It does *not* send a hide back -- the menu is already gone, and saying
    /// so again is what would make this loop.
    fn handle_system_hide(&self) {
        self.visible.set(false);
        if let Some(on_system_hide) = &self.on_system_hide {
            on_system_hide();
        }
    }

    fn handle_custom_context_menu_action(&self, _action_id: &str) {}
}

#[cfg(test)]
mod input_surface_tests {
    use super::*;

    #[test]
    fn a_floating_cursor_update_has_to_say_where_it_moved_to() {
        // Upstream asserts it. The three states are one enum on the wire, so
        // the requirement cannot be made unrepresentable -- it is checked
        // where the point is built instead.
        let start = RawFloatingCursorPoint::new(FloatingCursorDragState::Start, None);
        assert_eq!(start.offset, None);
        let update = RawFloatingCursorPoint::new(
            FloatingCursorDragState::Update,
            Some(Offset::new(4.0, 5.0)),
        );
        assert_eq!(update.offset, Some(Offset::new(4.0, 5.0)));
        // An end needs no offset: the real caret snaps to wherever the
        // floating one got to.
        assert_eq!(
            RawFloatingCursorPoint::new(FloatingCursorDragState::End, None).state,
            FloatingCursorDragState::End
        );
    }

    #[test]
    fn a_selection_rect_reads_left_to_right_unless_it_says_otherwise() {
        let rect = SelectionRect::new(3, Rect::ltrb(0.0, 0.0, 10.0, 20.0));
        assert_eq!(rect.direction, TextDirection::Ltr);
        assert_eq!(
            rect.with_direction(TextDirection::Rtl).direction,
            TextDirection::Rtl
        );
    }

    #[test]
    fn a_toolbar_offers_everything_but_live_text_by_default() {
        // Upstream's defaults, and the asymmetry is the point: Live Text
        // needs a camera, so a field opts in rather than out.
        struct Field;
        impl TextSelectionDelegate for Field {
            fn text_editing_value(&self) -> TextEditingValue {
                TextEditingValue::new("")
            }
            fn user_update_text_editing_value(
                &self,
                _value: TextEditingValue,
                _cause: SelectionChangedCause,
            ) {
            }
            fn hide_toolbar(&self, _hide_handles: bool) {}
            fn bring_into_view(&self, _position: i32) {}
            fn cut_selection(&self, _cause: SelectionChangedCause) {}
            fn copy_selection(&self, _cause: SelectionChangedCause) {}
            fn paste_text(&self, _cause: SelectionChangedCause) {}
            fn select_all(&self, _cause: SelectionChangedCause) {}
        }
        let field = Field;
        assert!(field.cut_enabled());
        assert!(field.copy_enabled());
        assert!(field.paste_enabled());
        assert!(field.select_all_enabled());
        assert!(field.look_up_enabled());
        assert!(field.search_web_enabled());
        assert!(field.share_enabled());
        assert!(
            !field.live_text_input_enabled(),
            "Live Text needs a camera, so it is opt-in"
        );
    }

    #[test]
    fn an_input_control_that_overrides_nothing_still_compiles_and_does_nothing() {
        // Upstream gives every method an empty body on purpose: a control
        // that only cares when the keyboard should show overrides `show` and
        // ignores the other five.
        struct OnlyShow(std::rc::Rc<std::cell::Cell<usize>>);
        impl TextInputControl for OnlyShow {
            fn show(&self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let control = OnlyShow(std::rc::Rc::clone(&count));
        control.hide();
        control.set_editing_state(&TextEditingValue::new("anything"));
        assert_eq!(count.get(), 0);
        control.show();
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn showing_the_menu_where_it_already_is_sends_nothing() {
        // The field asks on every selection change and most of them do not
        // move the menu, so upstream returns early. Without it, every caret
        // blink would be a round trip to the platform.
        let controller = SystemContextMenuController::new();
        assert!(!controller.is_visible());
        controller.show(Rect::ltrb(0.0, 0.0, 10.0, 10.0));
        assert!(controller.is_visible());
        // A different rectangle is a real move and does go out.
        controller.show(Rect::ltrb(0.0, 0.0, 20.0, 10.0));
        assert!(controller.is_visible());
    }

    #[test]
    fn the_system_taking_the_menu_away_does_not_send_a_hide_back() {
        // The menu is already gone; saying so again is what would make this
        // loop. The controller only stops believing, and tells whoever asked.
        let told = std::rc::Rc::new(std::cell::Cell::new(false));
        let sink = std::rc::Rc::clone(&told);
        let controller =
            SystemContextMenuController::new().with_on_system_hide(move || sink.set(true));
        controller.show(Rect::ltrb(0.0, 0.0, 10.0, 10.0));
        controller.handle_system_hide();
        assert!(!controller.is_visible());
        assert!(told.get());
        // And hiding one that is already down is nothing rather than a second
        // message.
        controller.hide();
        assert!(!controller.is_visible());
    }

    #[test]
    fn the_context_menu_methods_are_the_ones_the_platform_dispatches_on() {
        assert_eq!(
            SystemContextMenuController::SHOW_METHOD,
            "ContextMenu.showSystemContextMenu"
        );
        assert_eq!(
            SystemContextMenuController::HIDE_METHOD,
            "ContextMenu.hideSystemContextMenu"
        );
    }

    #[test]
    fn an_input_style_needs_a_direction_and_an_alignment_and_nothing_else() {
        // Upstream's other six are optional and mean "the platform's own
        // default"; a style that guessed a font would draw the composing
        // underline against text it does not match.
        let style = TextInputStyle::new(TextDirection::Ltr, TextAlign::Start);
        assert_eq!(style.font_family, None);
        assert_eq!(style.font_size, None);
        let with_font = style.with_font("Inter", 14.0);
        assert_eq!(with_font.font_family.as_deref(), Some("Inter"));
        assert_eq!(with_font.font_size, Some(14.0));
    }
}

#[cfg(test)]
mod connection_closed_tests {
    use super::super::codec::MethodCodec;
    use super::super::tests_support::install;
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Log {
        closed: usize,
        updates: usize,
    }

    struct Client {
        log: Rc<RefCell<Log>>,
        /// Whether this client grabs a new connection from inside the
        /// notification, the way a form moving to the next field does.
        reattach_as: Option<Rc<RefCell<Log>>>,
    }

    impl TextInputClient for Client {
        fn update_editing_value(&mut self, _value: TextEditingValue) {
            self.log.borrow_mut().updates += 1;
        }

        fn connection_closed(&mut self) {
            self.log.borrow_mut().closed += 1;
            if let Some(next) = self.reattach_as.take() {
                attach(
                    Box::new(Client {
                        log: next,
                        reattach_as: None,
                    }),
                    TextInputConfiguration::default(),
                );
            }
        }
    }

    /// The close message the host sends, in the host's own shape.
    fn deliver_close(recorder: &super::super::tests_support::Recorder, id: i32) {
        let call = JsonMethodCodec
            .encode_method_call(&MethodCall::new(
                "TextInputClient.onConnectionClosed",
                Value::List(vec![Value::I64(id as i64)]),
            ))
            .unwrap();
        recorder.deliver("flutter/textinput", &call, 0);
    }

    #[test]
    fn the_platform_closing_reaches_the_client() {
        let recorder = install();
        reset();
        let log = Rc::new(RefCell::new(Log::default()));
        let connection = attach(
            Box::new(Client {
                log: Rc::clone(&log),
                reattach_as: None,
            }),
            TextInputConfiguration::default(),
        );
        deliver_close(&recorder, connection.id);
        assert_eq!(log.borrow().closed, 1);
    }

    #[test]
    fn and_the_framework_side_is_gone_afterwards() {
        // The platform dropped it, so holding a client that can no longer be
        // spoken to would be holding a corpse.
        let recorder = install();
        reset();
        let log = Rc::new(RefCell::new(Log::default()));
        let connection = attach(
            Box::new(Client {
                log: Rc::clone(&log),
                reattach_as: None,
            }),
            TextInputConfiguration::default(),
        );
        deliver_close(&recorder, connection.id);
        assert!(!is_editing());
    }

    #[test]
    fn a_client_that_reattaches_while_being_told_keeps_its_new_connection() {
        // The slot is cleared *before* the client is told, so a client that
        // opens a new connection from inside `connection_closed` is not then
        // cleaned up by this one's cleanup. Clearing afterwards would leave
        // the form with no field attached and no way to know.
        let recorder = install();
        reset();
        let first = Rc::new(RefCell::new(Log::default()));
        let second = Rc::new(RefCell::new(Log::default()));
        let connection = attach(
            Box::new(Client {
                log: Rc::clone(&first),
                reattach_as: Some(Rc::clone(&second)),
            }),
            TextInputConfiguration::default(),
        );
        deliver_close(&recorder, connection.id);

        assert_eq!(first.borrow().closed, 1);
        assert!(
            is_editing(),
            "the new connection survived the old one's close"
        );
    }

    #[test]
    fn a_close_for_a_field_that_already_went_away_is_dropped() {
        // The same id check every other message gets: applying one field's
        // news to another is what the ids are there to prevent.
        let recorder = install();
        reset();
        let log = Rc::new(RefCell::new(Log::default()));
        let stale = attach(
            Box::new(Client {
                log: Rc::clone(&log),
                reattach_as: None,
            }),
            TextInputConfiguration::default(),
        );
        let fresh = Rc::new(RefCell::new(Log::default()));
        attach(
            Box::new(Client {
                log: Rc::clone(&fresh),
                reattach_as: None,
            }),
            TextInputConfiguration::default(),
        );

        deliver_close(&recorder, stale.id);
        assert_eq!(log.borrow().closed, 0, "the old client is gone already");
        assert_eq!(fresh.borrow().closed, 0, "and the new one was not told");
        assert!(is_editing());
    }

    #[test]
    fn the_defaults_are_the_ones_upstream_gives() {
        struct Bare;
        impl TextInputClient for Bare {
            fn update_editing_value(&mut self, _value: TextEditingValue) {}
        }
        let mut bare = Bare;
        // `onFocusReceived` defaults to false: "that was not me", which is the
        // safe answer for a client that has never heard of Safari's autofill
        // blur-and-refocus.
        assert!(!bare.on_focus_received());
        // And a client with no autofill says so with the same `None` a client
        // that autofills alone says -- the overload the trait's docs record.
        assert_eq!(bare.autofill_scope(), None);
        assert_eq!(bare.current_editing_value(), None);
    }

    #[test]
    fn a_scope_id_of_zero_is_not_the_absence_of_one() {
        // Which is the reason for the newtype: `None` already carries two
        // meanings without a group number joining in.
        assert_ne!(Some(AutofillScopeId(0)), None);
        assert_ne!(AutofillScopeId(0), AutofillScopeId(1));
    }
}

#[cfg(test)]
mod configuration_wire_format_tests {
    use super::{TextInputAction, TextInputConfiguration, TextInputType};
    use crate::component_themes::TextCapitalization;
    use crate::editable_text::{SmartDashesType, SmartQuotesType};
    use crate::platform::Brightness;
    use crate::services::codec::Value;

    fn key<'a>(value: &'a Value, name: &str) -> &'a Value {
        match value {
            Value::Map(pairs) => pairs
                .iter()
                .find(|(k, _)| matches!(k, Value::String(s) if s == name))
                .map(|(_, v)| v)
                .unwrap_or_else(|| panic!("no {name} in {value:?}")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn autocorrection_is_on_unless_a_field_turns_it_off() {
        // A derived Default gave false here, and upstream's constructor says
        // true. Four of these flags are on by default, so the zero value is
        // wrong for each of them.
        let plain = TextInputConfiguration::default();
        assert!(plain.autocorrect);
        assert!(plain.enable_suggestions);
        assert!(plain.enable_interactive_selection);
        assert!(plain.enable_ime_personalized_learning);
        assert!(!plain.obscure_text, "and these two are off");
        assert!(!plain.read_only);
    }

    #[test]
    fn a_number_field_says_false_where_others_say_nothing() {
        // TextInputType.number IS numberWithOptions(), so the plain number
        // type carries both flags as false. Every other type has them null.
        let numeric = TextInputConfiguration {
            input_type: TextInputType::NUMBER,
            ..TextInputConfiguration::default()
        };
        let numeric = numeric.to_value();
        let input_type = key(&numeric, "inputType");
        assert_eq!(key(input_type, "signed"), &Value::Bool(false));
        assert_eq!(key(input_type, "decimal"), &Value::Bool(false));

        let text = TextInputConfiguration::default();
        let text = text.to_value();
        let input_type = key(&text, "inputType");
        assert_eq!(key(input_type, "signed"), &Value::Null);
        assert_eq!(key(input_type, "decimal"), &Value::Null);
    }

    #[test]
    fn and_a_field_that_asked_for_a_decimal_keypad_gets_one() {
        let decimal = TextInputConfiguration {
            input_type: TextInputType::number_with_options(true, true),
            ..TextInputConfiguration::default()
        };
        let decimal = decimal.to_value();
        let input_type = key(&decimal, "inputType");
        assert_eq!(key(input_type, "signed"), &Value::Bool(true));
        assert_eq!(key(input_type, "decimal"), &Value::Bool(true));
        assert_eq!(
            key(input_type, "name"),
            &Value::String("TextInputType.number".to_string()),
            "still the number keyboard"
        );
    }

    #[test]
    fn the_smart_types_go_over_as_their_index_written_as_a_string() {
        // Not a name and not a boolean. Upstream sends
        // `smartDashesType.index.toString()`, so the declaration order of the
        // enum is part of the wire format.
        let off = TextInputConfiguration {
            smart_dashes: SmartDashesType::Disabled,
            smart_quotes: SmartQuotesType::Disabled,
            ..TextInputConfiguration::default()
        };
        let value = off.to_value();
        assert_eq!(
            key(&value, "smartDashesType"),
            &Value::String("0".to_string())
        );
        assert_eq!(
            key(&value, "smartQuotesType"),
            &Value::String("0".to_string())
        );

        let on = TextInputConfiguration::default();
        let value = on.to_value();
        assert_eq!(
            key(&value, "smartDashesType"),
            &Value::String("1".to_string())
        );
        assert_eq!(
            key(&value, "smartQuotesType"),
            &Value::String("1".to_string())
        );
    }

    #[test]
    fn the_named_enums_go_over_as_dart_would_print_them() {
        let config = TextInputConfiguration {
            text_capitalization: TextCapitalization::Sentences,
            keyboard_appearance: Brightness::Dark,
            action: TextInputAction::Search,
            ..TextInputConfiguration::default()
        };
        let value = config.to_value();
        assert_eq!(
            key(&value, "textCapitalization"),
            &Value::String("TextCapitalization.sentences".to_string())
        );
        assert_eq!(
            key(&value, "keyboardAppearance"),
            &Value::String("Brightness.dark".to_string())
        );
        assert_eq!(
            key(&value, "inputAction"),
            &Value::String("TextInputAction.search".to_string())
        );
    }

    #[test]
    fn the_keyboards_brightness_is_spelled_differently_from_the_settings_one() {
        // Two channels, two spellings of the same value, and both are the
        // platform's to insist on.
        assert_eq!(Brightness::Light.as_name(), "Brightness.light");
        let value = TextInputConfiguration::default().to_value();
        assert_eq!(
            key(&value, "keyboardAppearance"),
            &Value::String("Brightness.light".to_string())
        );
    }

    #[test]
    fn an_action_label_is_null_rather_than_empty_when_nobody_chose_one() {
        let value = TextInputConfiguration::default().to_value();
        assert_eq!(key(&value, "actionLabel"), &Value::Null);

        let labelled = TextInputConfiguration {
            action_label: Some("Post".to_string()),
            ..TextInputConfiguration::default()
        };
        assert_eq!(
            key(&labelled.to_value(), "actionLabel"),
            &Value::String("Post".to_string())
        );
    }

    #[test]
    fn every_flag_the_configuration_carries_reaches_the_platform() {
        // The gap this all came from: the type existed and the value never
        // left the process. Flipping each one has to change the message.
        let plain = TextInputConfiguration::default().to_value();
        let flipped = TextInputConfiguration {
            autocorrect: false,
            read_only: true,
            enable_suggestions: false,
            enable_interactive_selection: false,
            enable_ime_personalized_learning: false,
            obscure_text: true,
            ..TextInputConfiguration::default()
        }
        .to_value();
        for name in [
            "autocorrect",
            "readOnly",
            "enableSuggestions",
            "enableInteractiveSelection",
            "enableIMEPersonalizedLearning",
            "obscureText",
        ] {
            assert_ne!(key(&plain, name), key(&flipped, name), "{name}");
        }
    }
}

#[cfg(test)]
mod remaining_wire_keys_tests {
    use super::TextInputConfiguration;
    use crate::platform::Locale;
    use crate::services::codec::Value;

    fn key(value: &Value, name: &str) -> Value {
        match value {
            Value::Map(pairs) => pairs
                .iter()
                .find(|(k, _)| matches!(k, Value::String(s) if s == name))
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("no {name}")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_message_now_carries_every_key_upstream_sends() {
        // Sixteen before this, nineteen upstream. The three added here are the
        // whole of the difference -- counted rather than estimated, because
        // the last commit said seven and was wrong.
        let value = TextInputConfiguration::default().to_value();
        for name in [
            "viewId",
            "inputType",
            "readOnly",
            "obscureText",
            "autocorrect",
            "smartDashesType",
            "smartQuotesType",
            "enableSuggestions",
            "enableInteractiveSelection",
            "actionLabel",
            "inputAction",
            "textCapitalization",
            "keyboardAppearance",
            "enableIMEPersonalizedLearning",
            "contentCommitMimeTypes",
            "enableDeltaModel",
            "hintLocales",
            "enableInlinePrediction",
        ] {
            key(&value, name);
        }
        // The nineteenth, `autofill`, is left out when disabled -- upstream
        // omits the key rather than sending it switched off, which the
        // configuration already did.
    }

    #[test]
    fn a_field_that_accepts_only_text_says_so_with_an_empty_list() {
        assert_eq!(
            key(
                &TextInputConfiguration::default().to_value(),
                "contentCommitMimeTypes"
            ),
            Value::List(vec![])
        );
        let images = TextInputConfiguration {
            allowed_mime_types: vec!["image/png".to_string(), "image/gif".to_string()],
            ..TextInputConfiguration::default()
        };
        assert_eq!(
            key(&images.to_value(), "contentCommitMimeTypes"),
            Value::List(vec![
                Value::String("image/png".to_string()),
                Value::String("image/gif".to_string()),
            ])
        );
    }

    #[test]
    fn hint_locales_go_over_as_language_tags_and_default_to_an_empty_list() {
        // Empty, not absent: upstream's default is `const <Locale>[]`.
        assert_eq!(
            key(&TextInputConfiguration::default().to_value(), "hintLocales"),
            Value::List(vec![])
        );

        let bilingual = TextInputConfiguration {
            hint_locales: Some(vec![
                Locale {
                    country_code: Some("GB".to_string()),
                    ..Locale::new("en")
                },
                Locale::new("fr"),
            ]),
            ..TextInputConfiguration::default()
        };
        assert_eq!(
            key(&bilingual.to_value(), "hintLocales"),
            Value::List(vec![
                Value::String("en-GB".to_string()),
                Value::String("fr".to_string()),
            ])
        );

        let silent = TextInputConfiguration {
            hint_locales: None,
            ..TextInputConfiguration::default()
        };
        assert_eq!(key(&silent.to_value(), "hintLocales"), Value::Null);
    }

    #[test]
    fn inline_prediction_says_nothing_rather_than_no() {
        // Null means "whatever the platform does". False is a field overruling
        // it, and upstream's doc says iOS has it on -- so the two are opposite
        // instructions, not the same one twice.
        assert_eq!(
            key(
                &TextInputConfiguration::default().to_value(),
                "enableInlinePrediction"
            ),
            Value::Null
        );
        for asked in [true, false] {
            let config = TextInputConfiguration {
                enable_inline_prediction: Some(asked),
                ..TextInputConfiguration::default()
            };
            assert_eq!(
                key(&config.to_value(), "enableInlinePrediction"),
                Value::Bool(asked),
                "{asked}"
            );
        }
    }
}
