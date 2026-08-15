// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What a platform message carries.
//!
//! A platform message is a channel name and a byte buffer. The bytes mean
//! nothing to the engine, which copies them across the language boundary and
//! stops there -- the two ends agree on a *codec*, and that agreement is the
//! whole of a channel's type system.
//!
//! Four of them exist upstream and all four are here, because the choice is not
//! ours to make: a channel's codec is fixed by whoever defined the channel, and
//! `flutter/platform` speaks JSON while `flutter/mousecursor` speaks the
//! standard binary format. An existing plugin's Android and iOS halves are
//! already written against one of them.
//!
//! | Codec | Upstream | Used by |
//! |---|---|---|
//! | [`StandardMessageCodec`] | `standard_message_codec.dart` | most plugins, `flutter/mousecursor`, `flutter/accessibility` |
//! | [`JsonMessageCodec`] | `message_codecs.dart` | `flutter/platform`, `flutter/navigation`, `flutter/system` |
//! | [`StringCodec`] | `message_codecs.dart` | `flutter/lifecycle` |
//! | [`BinaryCodec`] | `message_codecs.dart` | anything that wants the bytes |
//!
//! # One value type, not two
//!
//! Upstream every codec deals in `Object?`, because Dart has one dynamic type
//! and JSON and the standard format are two ways of writing it down. [`Value`]
//! is that type. The standard codec can write all of it; the JSON codec cannot
//! write the typed lists and says so rather than writing something a reader
//! would silently misread.

use std::fmt;

// -- Values -------------------------------------------------------------------

/// A value a codec can carry.
///
/// The variants are upstream's `StandardMessageCodec` type tags, which is what
/// makes the mapping decidable in both directions. Dart's `Object?` reaches the
/// same set through runtime types.
///
/// `Map` is a list of pairs rather than a `HashMap` for two reasons, and both
/// are about being able to say what went over the wire: Dart maps keep their
/// insertion order and allow any object as a key, including `null` and doubles,
/// which a Rust hash map cannot hold at all.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// Written as the standard codec's 32-bit integer.
    I32(i32),
    /// Written as the standard codec's 64-bit integer.
    ///
    /// Not narrowed to [`Value::I32`] when it would fit. Dart's encoder narrows
    /// because a Dart `int` has no width; the C++ encoder does not, because
    /// `EncodableValue` distinguishes them the way this does. Either is readable
    /// by the other end -- both tags decode to a Dart `int`.
    I64(i64),
    F64(f64),
    String(String),
    /// `Uint8List`. The one list type a plugin can rely on being cheap.
    Bytes(Vec<u8>),
    I32List(Vec<i32>),
    I64List(Vec<i64>),
    F32List(Vec<f32>),
    F64List(Vec<f64>),
    List(Vec<Value>),
    /// Insertion-ordered pairs. See the note on [`Value`].
    Map(Vec<(Value, Value)>),
}

impl Value {
    /// The value stored under a string key, for the map-shaped arguments that
    /// nearly every method call uses.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries
                .iter()
                .find(|(name, _)| matches!(name, Value::String(name) if name == key))
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The value as an integer, whichever width it arrived in.
    ///
    /// The width a number crossed in is the sender's business, not the
    /// reader's: Dart picks it from the magnitude, so the same argument can be
    /// an `int32` today and an `int64` when it grows.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::I32(value) => Some(*value as i64),
            Value::I64(value) => Some(*value),
            _ => None,
        }
    }

    /// The value as a double, accepting either integer width.
    ///
    /// Same reason as [`Value::as_i64`], one step further: JSON has no integers
    /// at all, so `1` and `1.0` are the same number arriving under different
    /// tags, and a reader that insisted on `F64` would reject half of them.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(value) => Some(*value),
            Value::I32(value) => Some(*value as f64),
            Value::I64(value) => Some(*value as f64),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// A map from string keys, which is the shape of most arguments.
    pub fn map<K: Into<String>, I: IntoIterator<Item = (K, Value)>>(entries: I) -> Value {
        Value::Map(
            entries
                .into_iter()
                .map(|(key, value)| (Value::String(key.into()), value))
                .collect(),
        )
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Value {
        Value::Bool(value)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Value {
        Value::I32(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Value {
        Value::I64(value)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Value {
        Value::F64(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Value {
        Value::String(value.to_string())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Value {
        Value::String(value)
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Value {
        Value::Bytes(value)
    }
}

// -- Errors -------------------------------------------------------------------

/// Bytes that are not what the codec expected.
///
/// One type rather than a variant per failure, because there is one thing to do
/// about any of them: the message is malformed, and the only honest reply is an
/// empty one. Upstream throws `FormatException` for the same reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecError(pub String);

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for CodecError {}

fn malformed(what: &str) -> CodecError {
    CodecError(what.to_string())
}

// -- Method calls -------------------------------------------------------------

/// One `invokeMethod`: a name and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct MethodCall {
    pub method: String,
    pub arguments: Value,
}

impl MethodCall {
    pub fn new(method: impl Into<String>, arguments: Value) -> MethodCall {
        MethodCall { method: method.into(), arguments }
    }

    /// The named argument, for the map-shaped arguments most calls use.
    pub fn argument(&self, key: &str) -> Option<&Value> {
        self.arguments.get(key)
    }
}

/// What the far end said a call failed with.
///
/// Upstream this is `PlatformException`, and the three fields are the same
/// three: a code the caller can branch on, a message for a human, and whatever
/// details the platform wanted to attach.
#[derive(Clone, Debug, PartialEq)]
pub struct MethodError {
    pub code: String,
    pub message: Option<String>,
    pub details: Value,
}

impl MethodError {
    pub fn new(code: impl Into<String>, message: Option<String>) -> MethodError {
        MethodError { code: code.into(), message, details: Value::Null }
    }
}

impl fmt::Display for MethodError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(message) => write!(formatter, "{}: {}", self.code, message),
            None => write!(formatter, "{}", self.code),
        }
    }
}

/// The answer to a method call.
///
/// Upstream this is the three-way `MethodChannel` handler contract -- `reply`,
/// `error`, and *nothing at all*. The third is not an error: an empty reply
/// means the far end has no handler for the channel, which is how
/// `MissingPluginException` is raised and how `OptionalMethodChannel` knows to
/// stay quiet about it.
#[derive(Clone, Debug, PartialEq)]
pub enum MethodResult {
    Success(Value),
    Error(MethodError),
    /// No handler. On the wire this is an empty message, not an envelope.
    NotImplemented,
}

// -- The codec traits ---------------------------------------------------------

/// Turns a value into bytes and back.
pub trait MessageCodec {
    fn encode(&self, value: &Value) -> Result<Vec<u8>, CodecError>;
    fn decode(&self, bytes: &[u8]) -> Result<Value, CodecError>;
}

/// Turns a method call and its result into bytes and back.
///
/// Split from [`MessageCodec`] the way upstream splits `MethodCodec` from
/// `MessageCodec`: an envelope is not a value, and the two directions of a call
/// are not the same shape.
pub trait MethodCodec {
    fn encode_method_call(&self, call: &MethodCall) -> Result<Vec<u8>, CodecError>;
    fn decode_method_call(&self, bytes: &[u8]) -> Result<MethodCall, CodecError>;
    fn encode_success_envelope(&self, result: &Value) -> Result<Vec<u8>, CodecError>;
    fn encode_error_envelope(&self, error: &MethodError) -> Result<Vec<u8>, CodecError>;
    /// Unwraps a reply. `Ok(None)` is an empty reply -- no handler on the far
    /// end -- which is a distinct outcome from a successful `null`.
    fn decode_envelope(&self, bytes: &[u8]) -> Result<Option<Value>, MethodError>;

    /// The bytes for one [`MethodResult`], or `None` for
    /// [`MethodResult::NotImplemented`], which is an empty reply rather than an
    /// envelope.
    fn encode_result(&self, result: &MethodResult) -> Option<Vec<u8>> {
        match result {
            MethodResult::Success(value) => self.encode_success_envelope(value).ok(),
            MethodResult::Error(error) => self.encode_error_envelope(error).ok(),
            MethodResult::NotImplemented => None,
        }
    }
}

// -- The standard binary codec ------------------------------------------------

/// Type tags, from `standard_message_codec.dart`. The wire is these bytes.
mod tag {
    pub const NULL: u8 = 0;
    pub const TRUE: u8 = 1;
    pub const FALSE: u8 = 2;
    pub const INT32: u8 = 3;
    pub const INT64: u8 = 4;
    pub const FLOAT64: u8 = 6;
    pub const STRING: u8 = 7;
    pub const UINT8_LIST: u8 = 8;
    pub const INT32_LIST: u8 = 9;
    pub const INT64_LIST: u8 = 10;
    pub const FLOAT64_LIST: u8 = 11;
    pub const LIST: u8 = 12;
    pub const MAP: u8 = 13;
    pub const FLOAT32_LIST: u8 = 14;
}

/// Tag 5, `largeInt`. Removed from Dart's encoder long ago but still read,
/// because messages written by an old plugin are still messages.
const TAG_LARGE_INT: u8 = 5;

/// Flutter's own binary format.
///
/// The one to use for anything new. Compact, typed, and the format every
/// generated plugin binding speaks.
///
/// # Alignment is part of the format
///
/// A typed list is padded so its first element lands on a multiple of its own
/// size *counted from the start of the buffer*. That is not decoration: the
/// Dart reader hands the bytes straight to a `Float64List.view`, which throws
/// on an unaligned offset. Writer and reader both have to count from the same
/// origin, which is why the padding is computed against the whole buffer rather
/// than against the value being written.
#[derive(Clone, Copy, Debug, Default)]
pub struct StandardMessageCodec;

impl StandardMessageCodec {
    pub const fn new() -> StandardMessageCodec {
        StandardMessageCodec
    }

    /// Writes one value, tag first. Public so the method codec can build an
    /// envelope out of three of them.
    pub fn write_value(buffer: &mut Vec<u8>, value: &Value) {
        match value {
            Value::Null => buffer.push(tag::NULL),
            Value::Bool(true) => buffer.push(tag::TRUE),
            Value::Bool(false) => buffer.push(tag::FALSE),
            Value::I32(number) => {
                buffer.push(tag::INT32);
                buffer.extend_from_slice(&number.to_le_bytes());
            }
            Value::I64(number) => {
                buffer.push(tag::INT64);
                buffer.extend_from_slice(&number.to_le_bytes());
            }
            Value::F64(number) => {
                buffer.push(tag::FLOAT64);
                write_alignment(buffer, 8);
                buffer.extend_from_slice(&number.to_le_bytes());
            }
            Value::String(text) => {
                buffer.push(tag::STRING);
                let bytes = text.as_bytes();
                write_size(buffer, bytes.len());
                buffer.extend_from_slice(bytes);
            }
            Value::Bytes(bytes) => {
                buffer.push(tag::UINT8_LIST);
                write_size(buffer, bytes.len());
                buffer.extend_from_slice(bytes);
            }
            Value::I32List(numbers) => {
                buffer.push(tag::INT32_LIST);
                write_size(buffer, numbers.len());
                write_alignment(buffer, 4);
                for number in numbers {
                    buffer.extend_from_slice(&number.to_le_bytes());
                }
            }
            Value::I64List(numbers) => {
                buffer.push(tag::INT64_LIST);
                write_size(buffer, numbers.len());
                write_alignment(buffer, 8);
                for number in numbers {
                    buffer.extend_from_slice(&number.to_le_bytes());
                }
            }
            Value::F32List(numbers) => {
                buffer.push(tag::FLOAT32_LIST);
                write_size(buffer, numbers.len());
                write_alignment(buffer, 4);
                for number in numbers {
                    buffer.extend_from_slice(&number.to_le_bytes());
                }
            }
            Value::F64List(numbers) => {
                buffer.push(tag::FLOAT64_LIST);
                write_size(buffer, numbers.len());
                write_alignment(buffer, 8);
                for number in numbers {
                    buffer.extend_from_slice(&number.to_le_bytes());
                }
            }
            Value::List(items) => {
                buffer.push(tag::LIST);
                write_size(buffer, items.len());
                for item in items {
                    StandardMessageCodec::write_value(buffer, item);
                }
            }
            Value::Map(entries) => {
                buffer.push(tag::MAP);
                write_size(buffer, entries.len());
                for (key, value) in entries {
                    StandardMessageCodec::write_value(buffer, key);
                    StandardMessageCodec::write_value(buffer, value);
                }
            }
        }
    }

    /// Reads one value, advancing `offset` past it.
    pub fn read_value(bytes: &[u8], offset: &mut usize) -> Result<Value, CodecError> {
        let tag = *bytes
            .get(*offset)
            .ok_or_else(|| malformed("message ends where a type was expected"))?;
        *offset += 1;
        match tag {
            tag::NULL => Ok(Value::Null),
            tag::TRUE => Ok(Value::Bool(true)),
            tag::FALSE => Ok(Value::Bool(false)),
            tag::INT32 => Ok(Value::I32(i32::from_le_bytes(read_array(bytes, offset)?))),
            tag::INT64 => Ok(Value::I64(i64::from_le_bytes(read_array(bytes, offset)?))),
            TAG_LARGE_INT => {
                // Written as decimal text by Dart versions predating int64.
                let size = read_size(bytes, offset)?;
                let text = read_utf8(bytes, offset, size)?;
                text.parse::<i64>()
                    .map(Value::I64)
                    .map_err(|_| malformed("largeInt is not a number"))
            }
            tag::FLOAT64 => {
                read_alignment(bytes, offset, 8)?;
                Ok(Value::F64(f64::from_le_bytes(read_array(bytes, offset)?)))
            }
            tag::STRING => {
                let size = read_size(bytes, offset)?;
                Ok(Value::String(read_utf8(bytes, offset, size)?))
            }
            tag::UINT8_LIST => {
                let size = read_size(bytes, offset)?;
                Ok(Value::Bytes(read_slice(bytes, offset, size)?.to_vec()))
            }
            tag::INT32_LIST => {
                let count = read_size(bytes, offset)?;
                read_alignment(bytes, offset, 4)?;
                let mut numbers = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    numbers.push(i32::from_le_bytes(read_array(bytes, offset)?));
                }
                Ok(Value::I32List(numbers))
            }
            tag::INT64_LIST => {
                let count = read_size(bytes, offset)?;
                read_alignment(bytes, offset, 8)?;
                let mut numbers = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    numbers.push(i64::from_le_bytes(read_array(bytes, offset)?));
                }
                Ok(Value::I64List(numbers))
            }
            tag::FLOAT32_LIST => {
                let count = read_size(bytes, offset)?;
                read_alignment(bytes, offset, 4)?;
                let mut numbers = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    numbers.push(f32::from_le_bytes(read_array(bytes, offset)?));
                }
                Ok(Value::F32List(numbers))
            }
            tag::FLOAT64_LIST => {
                let count = read_size(bytes, offset)?;
                read_alignment(bytes, offset, 8)?;
                let mut numbers = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    numbers.push(f64::from_le_bytes(read_array(bytes, offset)?));
                }
                Ok(Value::F64List(numbers))
            }
            tag::LIST => {
                let count = read_size(bytes, offset)?;
                let mut items = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    items.push(StandardMessageCodec::read_value(bytes, offset)?);
                }
                Ok(Value::List(items))
            }
            tag::MAP => {
                let count = read_size(bytes, offset)?;
                let mut entries = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    let key = StandardMessageCodec::read_value(bytes, offset)?;
                    let value = StandardMessageCodec::read_value(bytes, offset)?;
                    entries.push((key, value));
                }
                Ok(Value::Map(entries))
            }
            other => Err(CodecError(format!("unknown type tag {other}"))),
        }
    }
}

/// Length prefix: one byte under 254, otherwise an escape and a wider integer.
///
/// 254 and 255 rather than 255 alone because both wide forms have to be
/// distinguishable, and a length of exactly 254 or 255 still has to be sayable.
fn write_size(buffer: &mut Vec<u8>, size: usize) {
    if size < 254 {
        buffer.push(size as u8);
    } else if size <= u16::MAX as usize {
        buffer.push(254);
        buffer.extend_from_slice(&(size as u16).to_le_bytes());
    } else {
        buffer.push(255);
        buffer.extend_from_slice(&(size as u32).to_le_bytes());
    }
}

fn read_size(bytes: &[u8], offset: &mut usize) -> Result<usize, CodecError> {
    let first = *bytes
        .get(*offset)
        .ok_or_else(|| malformed("message ends where a size was expected"))?;
    *offset += 1;
    match first {
        254 => Ok(u16::from_le_bytes(read_array(bytes, offset)?) as usize),
        255 => Ok(u32::from_le_bytes(read_array(bytes, offset)?) as usize),
        size => Ok(size as usize),
    }
}

/// Pads so the next byte written sits on a multiple of `alignment`, counted
/// from the start of the buffer. See the note on [`StandardMessageCodec`].
fn write_alignment(buffer: &mut Vec<u8>, alignment: usize) {
    let padding = buffer.len() % alignment;
    if padding != 0 {
        buffer.resize(buffer.len() + alignment - padding, 0);
    }
}

fn read_alignment(bytes: &[u8], offset: &mut usize, alignment: usize) -> Result<(), CodecError> {
    let padding = *offset % alignment;
    if padding != 0 {
        *offset += alignment - padding;
    }
    if *offset > bytes.len() {
        return Err(malformed("message ends inside alignment padding"));
    }
    Ok(())
}

fn read_slice<'a>(
    bytes: &'a [u8],
    offset: &mut usize,
    length: usize,
) -> Result<&'a [u8], CodecError> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| malformed("message claims a length that overflows"))?;
    if end > bytes.len() {
        return Err(malformed("message claims more bytes than it holds"));
    }
    let slice = &bytes[*offset..end];
    *offset = end;
    Ok(slice)
}

fn read_array<const N: usize>(bytes: &[u8], offset: &mut usize) -> Result<[u8; N], CodecError> {
    let slice = read_slice(bytes, offset, N)?;
    let mut array = [0u8; N];
    array.copy_from_slice(slice);
    Ok(array)
}

fn read_utf8(bytes: &[u8], offset: &mut usize, length: usize) -> Result<String, CodecError> {
    let slice = read_slice(bytes, offset, length)?;
    String::from_utf8(slice.to_vec()).map_err(|_| malformed("string is not valid UTF-8"))
}

impl MessageCodec for StandardMessageCodec {
    fn encode(&self, value: &Value) -> Result<Vec<u8>, CodecError> {
        let mut buffer = Vec::new();
        StandardMessageCodec::write_value(&mut buffer, value);
        Ok(buffer)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Value, CodecError> {
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        let mut offset = 0;
        let value = StandardMessageCodec::read_value(bytes, &mut offset)?;
        if offset != bytes.len() {
            return Err(malformed("message has bytes left over"));
        }
        Ok(value)
    }
}

/// Method calls in the standard binary format.
///
/// An envelope is one byte and then values: `0` and the result, or `1` and the
/// code, message and details. The failure case is three values rather than one
/// because the reader has to be able to rebuild a `PlatformException` without
/// parsing a string.
#[derive(Clone, Copy, Debug, Default)]
pub struct StandardMethodCodec;

impl StandardMethodCodec {
    pub const fn new() -> StandardMethodCodec {
        StandardMethodCodec
    }
}

impl MethodCodec for StandardMethodCodec {
    fn encode_method_call(&self, call: &MethodCall) -> Result<Vec<u8>, CodecError> {
        let mut buffer = Vec::new();
        StandardMessageCodec::write_value(&mut buffer, &Value::String(call.method.clone()));
        StandardMessageCodec::write_value(&mut buffer, &call.arguments);
        Ok(buffer)
    }

    fn decode_method_call(&self, bytes: &[u8]) -> Result<MethodCall, CodecError> {
        let mut offset = 0;
        let method = StandardMessageCodec::read_value(bytes, &mut offset)?;
        let arguments = StandardMessageCodec::read_value(bytes, &mut offset)?;
        match method {
            Value::String(method) if offset == bytes.len() => Ok(MethodCall { method, arguments }),
            Value::String(_) => Err(malformed("method call has bytes left over")),
            _ => Err(malformed("method name is not a string")),
        }
    }

    fn encode_success_envelope(&self, result: &Value) -> Result<Vec<u8>, CodecError> {
        let mut buffer = vec![0];
        StandardMessageCodec::write_value(&mut buffer, result);
        Ok(buffer)
    }

    fn encode_error_envelope(&self, error: &MethodError) -> Result<Vec<u8>, CodecError> {
        let mut buffer = vec![1];
        StandardMessageCodec::write_value(&mut buffer, &Value::String(error.code.clone()));
        let message = match &error.message {
            Some(message) => Value::String(message.clone()),
            None => Value::Null,
        };
        StandardMessageCodec::write_value(&mut buffer, &message);
        StandardMessageCodec::write_value(&mut buffer, &error.details);
        Ok(buffer)
    }

    fn decode_envelope(&self, bytes: &[u8]) -> Result<Option<Value>, MethodError> {
        if bytes.is_empty() {
            return Ok(None);
        }
        let mut offset = 1;
        let read = |offset: &mut usize| {
            StandardMessageCodec::read_value(bytes, offset)
                .map_err(|error| MethodError::new("malformed-envelope", Some(error.0)))
        };
        match bytes[0] {
            0 => Ok(Some(read(&mut offset)?)),
            1 => {
                let code = read(&mut offset)?;
                let message = read(&mut offset)?;
                let details = read(&mut offset)?;
                Err(MethodError {
                    code: match code {
                        Value::String(code) => code,
                        _ => "error".to_string(),
                    },
                    message: match message {
                        Value::String(message) => Some(message),
                        _ => None,
                    },
                    details,
                })
            }
            _ => Err(MethodError::new(
                "malformed-envelope",
                Some("envelope tag is neither success nor error".to_string()),
            )),
        }
    }
}

// -- JSON ---------------------------------------------------------------------

/// UTF-8 JSON.
///
/// The codec the engine's own channels speak. It exists because those channels
/// predate the standard format and their platform halves are written in four
/// languages; changing it now would break every one of them at once.
///
/// Typed lists have no JSON spelling and are refused rather than flattened: a
/// `Float64List` written as an array of numbers reads back as a plain list, and
/// a caller that round-trips one would silently get a different type than it
/// put in.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonMessageCodec;

impl JsonMessageCodec {
    pub const fn new() -> JsonMessageCodec {
        JsonMessageCodec
    }
}

impl MessageCodec for JsonMessageCodec {
    fn encode(&self, value: &Value) -> Result<Vec<u8>, CodecError> {
        let mut text = String::new();
        json::write(&mut text, value)?;
        Ok(text.into_bytes())
    }

    fn decode(&self, bytes: &[u8]) -> Result<Value, CodecError> {
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| malformed("JSON is not valid UTF-8"))?;
        json::parse(text)
    }
}

/// Method calls as JSON.
///
/// A call is `{"method": name, "args": arguments}`; a reply is a one-element
/// array on success and a three-element one on failure. The array is not an
/// arbitrary choice -- JSON has no way to tag an envelope, so the *shape* is the
/// tag, and one element cannot be mistaken for three.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonMethodCodec;

impl JsonMethodCodec {
    pub const fn new() -> JsonMethodCodec {
        JsonMethodCodec
    }
}

impl MethodCodec for JsonMethodCodec {
    fn encode_method_call(&self, call: &MethodCall) -> Result<Vec<u8>, CodecError> {
        JsonMessageCodec.encode(&Value::map([
            ("method", Value::String(call.method.clone())),
            ("args", call.arguments.clone()),
        ]))
    }

    fn decode_method_call(&self, bytes: &[u8]) -> Result<MethodCall, CodecError> {
        let value = JsonMessageCodec.decode(bytes)?;
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("JSON method call has no method name"))?
            .to_string();
        let arguments = value.get("args").cloned().unwrap_or(Value::Null);
        Ok(MethodCall { method, arguments })
    }

    fn encode_success_envelope(&self, result: &Value) -> Result<Vec<u8>, CodecError> {
        JsonMessageCodec.encode(&Value::List(vec![result.clone()]))
    }

    fn encode_error_envelope(&self, error: &MethodError) -> Result<Vec<u8>, CodecError> {
        JsonMessageCodec.encode(&Value::List(vec![
            Value::String(error.code.clone()),
            match &error.message {
                Some(message) => Value::String(message.clone()),
                None => Value::Null,
            },
            error.details.clone(),
        ]))
    }

    fn decode_envelope(&self, bytes: &[u8]) -> Result<Option<Value>, MethodError> {
        if bytes.is_empty() {
            return Ok(None);
        }
        let value = JsonMessageCodec
            .decode(bytes)
            .map_err(|error| MethodError::new("malformed-envelope", Some(error.0)))?;
        match value.as_list() {
            Some([result]) => Ok(Some(result.clone())),
            Some([code, message, details]) => Err(MethodError {
                code: code.as_str().unwrap_or("error").to_string(),
                message: message.as_str().map(str::to_string),
                details: details.clone(),
            }),
            _ => Err(MethodError::new(
                "malformed-envelope",
                Some("JSON envelope is neither one nor three elements".to_string()),
            )),
        }
    }
}

/// Just enough JSON for the engine's channels.
///
/// Hand-written for the same reason `[dependencies]` is empty: the framework
/// links the engine and nothing else, and a JSON reader is a day's work against
/// a permanent dependency. Upstream's C++ side does the same --
/// `json_message_codec.cc` wraps rapidjson, which the engine already vendors,
/// rather than adding one.
mod json {
    use super::{CodecError, Value, malformed};
    use std::fmt::Write as _;

    pub fn write(out: &mut String, value: &Value) -> Result<(), CodecError> {
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::I32(number) => {
                let _ = write!(out, "{number}");
            }
            Value::I64(number) => {
                let _ = write!(out, "{number}");
            }
            Value::F64(number) => {
                // JSON has no infinity and no NaN, and a writer that emits them
                // produces something no parser will read back. Dart's
                // json.encode throws here; refusing is the same answer with a
                // recoverable shape.
                if !number.is_finite() {
                    return Err(malformed("JSON cannot carry an infinite or NaN number"));
                }
                // An integral double must keep its point, or a reader tags it
                // as an integer and the round trip changes the type. Rust's
                // Display drops the point for every integral value, at any
                // magnitude, so the test is on the number rather than on its
                // size -- an earlier cutoff at 1e15 let exactly the large ones
                // through, which are the ones a size test was supposed to
                // catch.
                if number.fract() == 0.0 {
                    // `{:.1}` on a huge double spells out all 309 digits.
                    // Beyond what a double can represent exactly there is no
                    // integer to be mistaken for anyway, so those go out in
                    // exponent form -- which is also what Dart writes.
                    const EXACT_INTEGER_LIMIT: f64 = 9.007199254740992e15;
                    if number.abs() <= EXACT_INTEGER_LIMIT {
                        let _ = write!(out, "{number:.1}");
                    } else {
                        let _ = write!(out, "{number:e}");
                    }
                } else {
                    let _ = write!(out, "{number}");
                }
            }
            Value::String(text) => write_string(out, text),
            Value::List(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write(out, item)?;
                }
                out.push(']');
            }
            Value::Map(entries) => {
                out.push('{');
                for (index, (key, item)) in entries.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    // JSON keys are strings. Dart's encoder stringifies
                    // whatever it is handed; refusing says which key was wrong,
                    // which is the more useful answer at the call site.
                    match key {
                        Value::String(name) => write_string(out, name),
                        _ => return Err(malformed("JSON object keys must be strings")),
                    }
                    out.push(':');
                    write(out, item)?;
                }
                out.push('}');
            }
            Value::Bytes(_)
            | Value::I32List(_)
            | Value::I64List(_)
            | Value::F32List(_)
            | Value::F64List(_) => {
                return Err(malformed("JSON cannot carry a typed list"));
            }
        }
        Ok(())
    }

    fn write_string(out: &mut String, text: &str) {
        out.push('"');
        for character in text.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{08}' => out.push_str("\\b"),
                '\u{0c}' => out.push_str("\\f"),
                // Everything below space has to be escaped; everything above
                // goes out as UTF-8, which is what the far end decodes.
                control if control < ' ' => {
                    let _ = write!(out, "\\u{:04x}", control as u32);
                }
                other => out.push(other),
            }
        }
        out.push('"');
    }

    pub fn parse(text: &str) -> Result<Value, CodecError> {
        let characters: Vec<char> = text.chars().collect();
        let mut parser = Parser { input: &characters, offset: 0 };
        parser.skip_whitespace();
        let value = parser.value()?;
        parser.skip_whitespace();
        if parser.offset != parser.input.len() {
            return Err(malformed("JSON has trailing characters"));
        }
        Ok(value)
    }

    struct Parser<'a> {
        input: &'a [char],
        offset: usize,
    }

    impl Parser<'_> {
        fn peek(&self) -> Option<char> {
            self.input.get(self.offset).copied()
        }

        fn skip_whitespace(&mut self) {
            while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
                self.offset += 1;
            }
        }

        fn expect(&mut self, expected: char) -> Result<(), CodecError> {
            if self.peek() == Some(expected) {
                self.offset += 1;
                Ok(())
            } else {
                Err(CodecError(format!("expected '{expected}' in JSON")))
            }
        }

        fn literal(&mut self, word: &str) -> Result<(), CodecError> {
            for expected in word.chars() {
                self.expect(expected)?;
            }
            Ok(())
        }

        fn value(&mut self) -> Result<Value, CodecError> {
            match self.peek() {
                Some('n') => {
                    self.literal("null")?;
                    Ok(Value::Null)
                }
                Some('t') => {
                    self.literal("true")?;
                    Ok(Value::Bool(true))
                }
                Some('f') => {
                    self.literal("false")?;
                    Ok(Value::Bool(false))
                }
                Some('"') => Ok(Value::String(self.string()?)),
                Some('[') => self.array(),
                Some('{') => self.object(),
                Some(character) if character == '-' || character.is_ascii_digit() => self.number(),
                Some(character) => Err(CodecError(format!("unexpected '{character}' in JSON"))),
                None => Err(malformed("JSON ends where a value was expected")),
            }
        }

        fn array(&mut self) -> Result<Value, CodecError> {
            self.expect('[')?;
            let mut items = Vec::new();
            self.skip_whitespace();
            if self.peek() == Some(']') {
                self.offset += 1;
                return Ok(Value::List(items));
            }
            loop {
                self.skip_whitespace();
                items.push(self.value()?);
                self.skip_whitespace();
                match self.peek() {
                    Some(',') => self.offset += 1,
                    Some(']') => {
                        self.offset += 1;
                        return Ok(Value::List(items));
                    }
                    _ => return Err(malformed("unterminated JSON array")),
                }
            }
        }

        fn object(&mut self) -> Result<Value, CodecError> {
            self.expect('{')?;
            let mut entries = Vec::new();
            self.skip_whitespace();
            if self.peek() == Some('}') {
                self.offset += 1;
                return Ok(Value::Map(entries));
            }
            loop {
                self.skip_whitespace();
                let key = self.string()?;
                self.skip_whitespace();
                self.expect(':')?;
                self.skip_whitespace();
                let value = self.value()?;
                entries.push((Value::String(key), value));
                self.skip_whitespace();
                match self.peek() {
                    Some(',') => self.offset += 1,
                    Some('}') => {
                        self.offset += 1;
                        return Ok(Value::Map(entries));
                    }
                    _ => return Err(malformed("unterminated JSON object")),
                }
            }
        }

        fn string(&mut self) -> Result<String, CodecError> {
            self.expect('"')?;
            let mut text = String::new();
            loop {
                let character =
                    self.peek().ok_or_else(|| malformed("unterminated JSON string"))?;
                self.offset += 1;
                match character {
                    '"' => return Ok(text),
                    '\\' => {
                        let escape =
                            self.peek().ok_or_else(|| malformed("unterminated JSON escape"))?;
                        self.offset += 1;
                        match escape {
                            '"' => text.push('"'),
                            '\\' => text.push('\\'),
                            '/' => text.push('/'),
                            'b' => text.push('\u{08}'),
                            'f' => text.push('\u{0c}'),
                            'n' => text.push('\n'),
                            'r' => text.push('\r'),
                            't' => text.push('\t'),
                            'u' => text.push(self.escaped_code_point()?),
                            other => {
                                return Err(CodecError(format!(
                                    "unknown JSON escape '\\{other}'"
                                )));
                            }
                        }
                    }
                    other => text.push(other),
                }
            }
        }

        /// One `\uXXXX`, plus its low surrogate if it was a high one.
        ///
        /// JSON escapes are UTF-16 code units, so anything outside the basic
        /// plane arrives as a surrogate pair. Rust's `char` cannot hold half of
        /// one, so the pair has to be recombined here or not read at all.
        fn escaped_code_point(&mut self) -> Result<char, CodecError> {
            let first = self.hex4()?;
            if !(0xD800..0xDC00).contains(&first) {
                return char::from_u32(first).ok_or_else(|| malformed("invalid JSON escape"));
            }
            self.expect('\\')?;
            self.expect('u')?;
            let second = self.hex4()?;
            if !(0xDC00..0xE000).contains(&second) {
                return Err(malformed("JSON high surrogate is not followed by a low one"));
            }
            let combined = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
            char::from_u32(combined).ok_or_else(|| malformed("invalid JSON surrogate pair"))
        }

        fn hex4(&mut self) -> Result<u32, CodecError> {
            let mut value = 0u32;
            for _ in 0..4 {
                let digit = self.peek().ok_or_else(|| malformed("truncated JSON escape"))?;
                let digit =
                    digit.to_digit(16).ok_or_else(|| malformed("bad JSON escape digit"))?;
                self.offset += 1;
                value = value * 16 + digit;
            }
            Ok(value)
        }

        /// A JSON number, read as an integer when it was written as one.
        ///
        /// JSON does not distinguish them, but the far end does:
        /// `flutter/settings` sends `textScaleFactor` as a double and
        /// `flutter/keyevent` sends `keyCode` as an integer, and a reader that
        /// turned every number into a double would make the second one unusable
        /// as an index. The spelling is the only evidence available, so the
        /// spelling is what decides -- which is what Dart's `json.decode` does.
        fn number(&mut self) -> Result<Value, CodecError> {
            let start = self.offset;
            if self.peek() == Some('-') {
                self.offset += 1;
            }
            let mut floating = false;
            while let Some(character) = self.peek() {
                match character {
                    '0'..='9' => self.offset += 1,
                    '.' | 'e' | 'E' | '+' | '-' => {
                        floating = true;
                        self.offset += 1;
                    }
                    _ => break,
                }
            }
            let text: String = self.input[start..self.offset].iter().collect();
            // A number written without a point, a sign or an exponent, and that
            // fits, is an integer. Anything else falls through to a double --
            // including an integer too large for one, which is what Dart does
            // with it too.
            let integral = if floating { None } else { text.parse::<i64>().ok() };
            if let Some(number) = integral {
                return Ok(Value::I64(number));
            }
            text.parse::<f64>()
                .map(Value::F64)
                .map_err(|_| CodecError(format!("'{text}' is not a JSON number")))
        }
    }
}

// -- The two trivial codecs ---------------------------------------------------

/// UTF-8 text, and nothing else.
///
/// `flutter/lifecycle` uses it: the whole message is the string
/// `AppLifecycleState.resumed`, with no envelope, because there is nothing else
/// it could ever need to say.
#[derive(Clone, Copy, Debug, Default)]
pub struct StringCodec;

impl StringCodec {
    pub const fn new() -> StringCodec {
        StringCodec
    }
}

impl MessageCodec for StringCodec {
    fn encode(&self, value: &Value) -> Result<Vec<u8>, CodecError> {
        match value {
            Value::Null => Ok(Vec::new()),
            Value::String(text) => Ok(text.as_bytes().to_vec()),
            _ => Err(malformed("StringCodec can only carry a string")),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<Value, CodecError> {
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        std::str::from_utf8(bytes)
            .map(|text| Value::String(text.to_string()))
            .map_err(|_| malformed("message is not valid UTF-8"))
    }
}

/// The bytes, unchanged.
///
/// For a channel whose payload is already a format of its own -- an image, a
/// file, a protobuf -- where a second encoding would be a copy for nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct BinaryCodec;

impl BinaryCodec {
    pub const fn new() -> BinaryCodec {
        BinaryCodec
    }
}

impl MessageCodec for BinaryCodec {
    fn encode(&self, value: &Value) -> Result<Vec<u8>, CodecError> {
        match value {
            Value::Null => Ok(Vec::new()),
            Value::Bytes(bytes) => Ok(bytes.clone()),
            _ => Err(malformed("BinaryCodec can only carry bytes")),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<Value, CodecError> {
        Ok(Value::Bytes(bytes.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(value: Value) -> Value {
        let bytes = StandardMessageCodec.encode(&value).expect("encodes");
        StandardMessageCodec.decode(&bytes).expect("decodes")
    }

    #[test]
    fn the_standard_codec_writes_the_bytes_upstream_writes() {
        // Checked against standard_message_codec.dart: a tag, then the value,
        // little-endian. These are the bytes an Android or iOS plugin reads, so
        // they are not ours to choose.
        assert_eq!(StandardMessageCodec.encode(&Value::Null).unwrap(), vec![0]);
        assert_eq!(StandardMessageCodec.encode(&Value::Bool(true)).unwrap(), vec![1]);
        assert_eq!(StandardMessageCodec.encode(&Value::Bool(false)).unwrap(), vec![2]);
        assert_eq!(StandardMessageCodec.encode(&Value::I32(7)).unwrap(), vec![3, 7, 0, 0, 0]);
        assert_eq!(
            StandardMessageCodec.encode(&Value::I64(-2)).unwrap(),
            vec![4, 254, 255, 255, 255, 255, 255, 255, 255]
        );
        // 7, length 2, then the UTF-8.
        assert_eq!(
            StandardMessageCodec.encode(&Value::from("hi")).unwrap(),
            vec![7, 2, b'h', b'i']
        );
    }

    #[test]
    fn a_double_is_padded_to_its_own_alignment() {
        // Tag, then seven bytes of padding, because the eight bytes of the
        // double have to start at offset 8. Dart reads them with a
        // Float64List.view, which throws on a misaligned offset -- this padding
        // is the difference between a message being read and an exception.
        let bytes = StandardMessageCodec.encode(&Value::F64(1.0)).unwrap();
        assert_eq!(bytes.len(), 16);
        assert_eq!(bytes[0], 6);
        assert_eq!(&bytes[1..8], &[0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(f64::from_le_bytes(bytes[8..16].try_into().unwrap()), 1.0);
    }

    #[test]
    fn a_typed_list_is_aligned_from_the_start_of_the_buffer() {
        // Inside a list, so the element start is not where it would be on its
        // own -- which is the case a writer that aligned relative to the value
        // rather than the buffer would get wrong.
        let value = Value::List(vec![Value::Bool(true), Value::I32List(vec![1, 2])]);
        let bytes = StandardMessageCodec.encode(&value).unwrap();
        let position = bytes.iter().position(|byte| *byte == 9).expect("has an int32 list");
        // tag, size, then padding up to a multiple of four.
        let elements = position + 2 + ((4 - (position + 2) % 4) % 4);
        assert_eq!(elements % 4, 0);
        assert_eq!(round_trip(value.clone()), value);
    }

    #[test]
    fn sizes_escape_at_the_two_boundaries() {
        // Under 254 is one byte; 254 and up needs the escape, and both wide
        // forms have to stay distinguishable -- which is why there are two
        // escapes rather than one.
        let mut buffer = Vec::new();
        write_size(&mut buffer, 253);
        assert_eq!(buffer, vec![253]);

        buffer.clear();
        write_size(&mut buffer, 254);
        assert_eq!(buffer, vec![254, 254, 0]);

        buffer.clear();
        write_size(&mut buffer, 70000);
        assert_eq!(buffer, vec![255, 0x70, 0x11, 1, 0]);

        for size in [0usize, 1, 253, 254, 255, 65535, 65536, 70000] {
            let mut buffer = Vec::new();
            write_size(&mut buffer, size);
            let mut offset = 0;
            assert_eq!(read_size(&buffer, &mut offset).unwrap(), size);
            assert_eq!(offset, buffer.len());
        }
    }

    #[test]
    fn every_standard_value_survives_a_round_trip() {
        let values = vec![
            Value::Null,
            Value::Bool(true),
            Value::I32(i32::MIN),
            Value::I64(i64::MAX),
            Value::F64(-0.5),
            Value::String("h\u{e9}llo \u{4e16}\u{754c}".to_string()),
            Value::Bytes(vec![0, 1, 254, 255]),
            Value::I32List(vec![-1, 0, 1]),
            Value::I64List(vec![i64::MIN, i64::MAX]),
            Value::F32List(vec![0.5, -0.25]),
            Value::F64List(vec![1.5, 2.5, 3.5]),
            Value::List(vec![Value::Null, Value::from("x"), Value::I32(3)]),
            Value::map([("a", Value::I32(1)), ("b", Value::from("two"))]),
        ];
        for value in values {
            assert_eq!(round_trip(value.clone()), value, "round trip of {value:?}");
        }
    }

    #[test]
    fn a_long_string_crosses_the_size_escape() {
        let long = "x".repeat(300);
        assert_eq!(round_trip(Value::String(long.clone())), Value::String(long));
    }

    #[test]
    fn a_truncated_message_is_refused_rather_than_read_past() {
        let bytes = StandardMessageCodec.encode(&Value::from("hello")).unwrap();
        for cut in 1..bytes.len() {
            assert!(
                StandardMessageCodec.decode(&bytes[..cut]).is_err(),
                "{cut} bytes of a 7-byte string should not decode"
            );
        }
    }

    #[test]
    fn a_standard_method_call_round_trips() {
        let call = MethodCall::new(
            "SystemSound.play",
            Value::map([("type", Value::from("SystemSoundType.click"))]),
        );
        let bytes = StandardMethodCodec.encode_method_call(&call).unwrap();
        assert_eq!(StandardMethodCodec.decode_method_call(&bytes).unwrap(), call);
    }

    #[test]
    fn a_standard_envelope_carries_success_failure_and_absence() {
        let success = StandardMethodCodec.encode_success_envelope(&Value::I32(42)).unwrap();
        assert_eq!(success[0], 0);
        assert_eq!(StandardMethodCodec.decode_envelope(&success), Ok(Some(Value::I32(42))));

        let error = MethodError {
            code: "unavailable".to_string(),
            message: Some("no clipboard".to_string()),
            details: Value::Null,
        };
        let encoded = StandardMethodCodec.encode_error_envelope(&error).unwrap();
        assert_eq!(encoded[0], 1);
        assert_eq!(StandardMethodCodec.decode_envelope(&encoded), Err(error));

        // The third outcome, and the one that is easy to lose: an empty reply
        // is "nobody is listening", which is how MissingPluginException is
        // raised. It is not a successful null.
        assert_eq!(StandardMethodCodec.decode_envelope(&[]), Ok(None));
    }

    #[test]
    fn json_writes_what_the_engine_channels_expect() {
        let value = Value::map([
            ("textScaleFactor", Value::F64(1.0)),
            ("alwaysUse24HourFormat", Value::Bool(true)),
            ("platformBrightness", Value::from("dark")),
        ]);
        let text = String::from_utf8(JsonMessageCodec.encode(&value).unwrap()).unwrap();
        assert_eq!(
            text,
            r#"{"textScaleFactor":1.0,"alwaysUse24HourFormat":true,"platformBrightness":"dark"}"#
        );
    }

    #[test]
    fn an_integral_double_keeps_its_point() {
        // Otherwise it reads back as an integer, and a channel that declared a
        // double gets a different type than it sent. Dart's json.encode does
        // the same.
        let text = String::from_utf8(JsonMessageCodec.encode(&Value::F64(2.0)).unwrap()).unwrap();
        assert_eq!(text, "2.0");
        assert_eq!(JsonMessageCodec.decode(text.as_bytes()).unwrap(), Value::F64(2.0));
    }

    #[test]
    fn an_integral_double_keeps_its_point_at_every_magnitude() {
        // The interesting values are the large ones. Rust's Display writes
        // 1e15 as "1000000000000000" -- no point, so a reader takes it for an
        // integer and the type changes across the round trip. Every one of
        // these has to come back a double.
        for number in [
            0.0,
            -0.0,
            2.0,
            1e14,
            1e15,
            9.007199254740992e15,
            1e16,
            1e300,
            f64::MAX,
            f64::MIN,
        ] {
            let bytes = JsonMessageCodec.encode(&Value::F64(number)).unwrap();
            let text = String::from_utf8(bytes.clone()).unwrap();
            match JsonMessageCodec.decode(&bytes).unwrap() {
                Value::F64(back) => assert_eq!(
                    back.to_bits(),
                    number.to_bits(),
                    "{number:?} was written as {text} and read back as {back:?}"
                ),
                other => panic!("{number:?} was written as {text} and read back as {other:?}"),
            }
        }
    }

    #[test]
    fn json_keeps_integers_and_doubles_apart() {
        // How it was written is the only evidence there is, and the two are not
        // interchangeable to the reader: a keyCode has to stay an integer.
        assert_eq!(JsonMessageCodec.decode(b"3").unwrap(), Value::I64(3));
        assert_eq!(JsonMessageCodec.decode(b"3.0").unwrap(), Value::F64(3.0));
        assert_eq!(JsonMessageCodec.decode(b"-4e2").unwrap(), Value::F64(-400.0));
    }

    #[test]
    fn json_strings_survive_escapes_and_astral_characters() {
        let value = Value::from("a\"b\\c\nd\te\u{1}f \u{4e16} \u{1F600}");
        let bytes = JsonMessageCodec.encode(&value).unwrap();
        assert_eq!(JsonMessageCodec.decode(&bytes).unwrap(), value);

        // A pair of UTF-16 escapes is one Rust char, which is the case a naive
        // reader loses: JSON escapes are code *units*, so anything outside the
        // basic plane arrives as a surrogate pair that has to be recombined.
        let pair = "\"\\ud83d\\ude00\"";
        assert_eq!(JsonMessageCodec.decode(pair.as_bytes()).unwrap(), Value::from("\u{1F600}"));
    }

    #[test]
    fn json_refuses_what_it_cannot_carry() {
        assert!(JsonMessageCodec.encode(&Value::Bytes(vec![1])).is_err());
        assert!(JsonMessageCodec.encode(&Value::F64(f64::NAN)).is_err());
        assert!(JsonMessageCodec.encode(&Value::Map(vec![(Value::I32(1), Value::Null)])).is_err());
        assert!(JsonMessageCodec.decode(b"{\"a\":1,}").is_err());
        assert!(JsonMessageCodec.decode(b"[1] junk").is_err());
    }

    #[test]
    fn a_json_method_call_round_trips_in_the_engines_shape() {
        let call = MethodCall::new("SystemNavigator.pop", Value::Null);
        let bytes = JsonMethodCodec.encode_method_call(&call).unwrap();
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            r#"{"method":"SystemNavigator.pop","args":null}"#
        );
        assert_eq!(JsonMethodCodec.decode_method_call(&bytes).unwrap(), call);
    }

    #[test]
    fn a_json_envelope_is_one_element_or_three() {
        // The shape is the tag: JSON has nowhere to put a type byte.
        let success = JsonMethodCodec.encode_success_envelope(&Value::from("ok")).unwrap();
        assert_eq!(String::from_utf8(success.clone()).unwrap(), r#"["ok"]"#);
        assert_eq!(JsonMethodCodec.decode_envelope(&success), Ok(Some(Value::from("ok"))));

        let error = MethodError::new("bad", Some("no".to_string()));
        let encoded = JsonMethodCodec.encode_error_envelope(&error).unwrap();
        assert_eq!(String::from_utf8(encoded.clone()).unwrap(), r#"["bad","no",null]"#);
        assert_eq!(JsonMethodCodec.decode_envelope(&encoded), Err(error));

        assert_eq!(JsonMethodCodec.decode_envelope(&[]), Ok(None));
    }

    #[test]
    fn the_string_and_binary_codecs_are_the_identity_they_claim_to_be() {
        let encoded = StringCodec.encode(&Value::from("AppLifecycleState.resumed")).unwrap();
        assert_eq!(encoded, b"AppLifecycleState.resumed");
        assert_eq!(
            StringCodec.decode(&encoded).unwrap(),
            Value::from("AppLifecycleState.resumed")
        );
        // An empty message is the absence of one, not an empty string.
        assert_eq!(StringCodec.decode(&[]).unwrap(), Value::Null);

        assert_eq!(BinaryCodec.encode(&Value::Bytes(vec![1, 2, 3])).unwrap(), vec![1, 2, 3]);
        assert_eq!(BinaryCodec.decode(&[1, 2, 3]).unwrap(), Value::Bytes(vec![1, 2, 3]));
    }
}
