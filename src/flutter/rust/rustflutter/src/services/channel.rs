// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Channels: a name, a codec, and a direction.
//!
//! A [`Messenger`](super::Messenger) moves bytes. A channel is the agreement
//! about what those bytes mean -- which codec, and which of the three
//! conversation shapes upstream defines:
//!
//! | Shape | For |
//! |---|---|
//! | [`BasicMessageChannel`] | messages, either way, optionally answered |
//! | [`MethodChannel`] | a named call with arguments and a result |
//! | [`EventChannel`] | a stream the platform pushes until it is cancelled |
//!
//! All three are thin: a channel holds a name and a codec and nothing else, so
//! it can be a constant. That is how upstream declares `SystemChannels`, and it
//! matters -- a channel with state would have to be found before it could be
//! used, and a plugin would need a handle to something.

use std::borrow::Cow;

use super::Responder;
use super::codec::{MessageCodec, MethodCall, MethodCodec, MethodError, MethodResult, Value};

/// The answer to a message, when a channel deals in values rather than bytes.
///
/// `None` is the empty reply -- nobody was listening -- which stays distinct
/// from `Some(Value::Null)` all the way down. See [`super::ReplyData`].
pub type ValueReply = Option<Value>;

/// Turns a handler's answer back into bytes.
///
/// Carried by the responder rather than looked up, so that a handler which
/// answers minutes later still encodes with the codec its channel was declared
/// with -- not whatever that channel is declared with by then.
type ValueEncoder = Box<dyn Fn(&Value) -> Option<Vec<u8>>>;

/// The same, for a method channel's three-way result.
type ResultEncoder = Box<dyn Fn(&MethodResult) -> Option<Vec<u8>>>;

/// Messages on a channel, encoded with one codec.
///
/// Upstream's `BasicMessageChannel`. Use it when there is no call and no
/// result, only something to say: `flutter/lifecycle` is one string, and
/// `flutter/keyevent` is one map.
#[derive(Clone, Debug)]
pub struct BasicMessageChannel<C> {
    name: Cow<'static, str>,
    codec: C,
}

impl<C: MessageCodec + Copy + 'static> BasicMessageChannel<C> {
    /// A channel with a name known at compile time, so it can be a constant.
    pub const fn new(name: &'static str, codec: C) -> BasicMessageChannel<C> {
        BasicMessageChannel {
            name: Cow::Borrowed(name),
            codec,
        }
    }

    /// A channel named at run time -- one per instance of something, which is
    /// what a plugin that can be opened twice needs.
    pub fn named(name: impl Into<String>, codec: C) -> BasicMessageChannel<C> {
        BasicMessageChannel {
            name: Cow::Owned(name.into()),
            codec,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sends a message and does not wait.
    pub fn send(&self, message: &Value) {
        let Ok(bytes) = self.codec.encode(message) else {
            return;
        };
        super::send(&self.name, &bytes);
    }

    /// Sends a message and hands the decoded reply to `callback`.
    ///
    /// `callback` runs exactly once, with `None` if nothing answered or the
    /// answer could not be decoded -- an unreadable reply and an absent one are
    /// the same amount of information.
    pub fn send_with_reply(&self, message: &Value, callback: impl FnOnce(ValueReply) + 'static) {
        let Ok(bytes) = self.codec.encode(message) else {
            callback(None);
            return;
        };
        let codec = self.codec;
        super::send_with_reply(
            &self.name,
            &bytes,
            Box::new(move |reply| callback(reply.and_then(|bytes| codec.decode(bytes).ok()))),
        );
    }

    /// Handles messages arriving on this channel.
    ///
    /// The handler is given the decoded message and a way to answer. It must
    /// call the responder exactly once -- now or later; a handler that has to
    /// wait for something is why the answer is a callback rather than a return
    /// value.
    pub fn set_handler(&self, mut handler: impl FnMut(Value, ValueResponder) + 'static) {
        let codec = self.codec;
        super::set_handler(
            &self.name,
            Box::new(move |bytes, respond| {
                let Ok(message) = codec.decode(bytes) else {
                    // Undecodable: answer empty rather than guess. The sender
                    // learns nothing handled it, which is true.
                    respond(None);
                    return;
                };
                handler(
                    message,
                    ValueResponder {
                        respond,
                        encode: Box::new(move |value| codec.encode(value).ok()),
                    },
                );
            }),
        );
    }

    pub fn clear_handler(&self) {
        super::clear_handler(&self.name);
    }
}

/// How a [`BasicMessageChannel`] handler answers.
///
/// It carries its channel's codec, so a handler answers in values and never
/// sees a byte.
pub struct ValueResponder {
    respond: Responder,
    encode: ValueEncoder,
}

impl ValueResponder {
    /// Answers with a value. `None` means "no answer", which the far end reads
    /// as nobody having handled the message.
    pub fn reply(self, value: Option<&Value>) {
        match value {
            Some(value) => match (self.encode)(value) {
                Some(bytes) => (self.respond)(Some(&bytes)),
                None => (self.respond)(None),
            },
            None => (self.respond)(None),
        }
    }
}

/// Named calls with arguments and a result.
///
/// Upstream's `MethodChannel`, and by far the most used of the three: nearly
/// every plugin is a method channel with a `switch` on the method name at each
/// end.
#[derive(Clone, Debug)]
pub struct MethodChannel<C> {
    name: Cow<'static, str>,
    codec: C,
}

/// What an `invoke` came back with.
///
/// Three outcomes, not two, and the third is the one that is easy to lose:
/// `Ok(None)` means the far end has no handler for this channel at all.
/// Upstream that is `MissingPluginException`, and the reason it is not an error
/// is that it is routine -- a plugin the application did not install, a method
/// the platform does not have, an `OptionalMethodChannel` that expects it.
pub type MethodReply = Result<Option<Value>, MethodError>;

impl<C: MethodCodec + Copy + 'static> MethodChannel<C> {
    pub const fn new(name: &'static str, codec: C) -> MethodChannel<C> {
        MethodChannel {
            name: Cow::Borrowed(name),
            codec,
        }
    }

    pub fn named(name: impl Into<String>, codec: C) -> MethodChannel<C> {
        MethodChannel {
            name: Cow::Owned(name.into()),
            codec,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Calls a method and ignores whatever comes back.
    ///
    /// Right for the many platform calls that have nothing to return:
    /// `SystemNavigator.pop`, `SystemSound.play`, `HapticFeedback.vibrate`.
    /// Asking for a reply that is never read costs the shell a response handle
    /// and a thread hop.
    pub fn invoke(&self, method: &str, arguments: Value) {
        let call = MethodCall::new(method, arguments);
        let Ok(bytes) = self.codec.encode_method_call(&call) else {
            return;
        };
        super::send(&self.name, &bytes);
    }

    /// Calls a method and hands the result to `callback`, exactly once.
    pub fn invoke_with_reply(
        &self,
        method: &str,
        arguments: Value,
        callback: impl FnOnce(MethodReply) + 'static,
    ) {
        let call = MethodCall::new(method, arguments);
        let Ok(bytes) = self.codec.encode_method_call(&call) else {
            callback(Err(MethodError::new(
                "malformed-call",
                Some(format!("{method} could not be encoded")),
            )));
            return;
        };
        let codec = self.codec;
        super::send_with_reply(
            &self.name,
            &bytes,
            Box::new(move |reply| {
                callback(match reply {
                    Some(bytes) => codec.decode_envelope(bytes),
                    None => Ok(None),
                })
            }),
        );
    }

    /// Handles calls arriving on this channel.
    pub fn set_handler(&self, mut handler: impl FnMut(MethodCall, MethodResponder) + 'static) {
        let codec = self.codec;
        super::set_handler(
            &self.name,
            Box::new(move |bytes, respond| {
                let Ok(call) = codec.decode_method_call(bytes) else {
                    respond(None);
                    return;
                };
                handler(
                    call,
                    MethodResponder {
                        respond,
                        encode: Box::new(move |result| codec.encode_result(result)),
                    },
                );
            }),
        );
    }

    pub fn clear_handler(&self) {
        super::clear_handler(&self.name);
    }
}

/// How a [`MethodChannel`] handler answers.
pub struct MethodResponder {
    respond: Responder,
    encode: ResultEncoder,
}

impl MethodResponder {
    pub fn success(self, value: Value) {
        self.finish(&MethodResult::Success(value));
    }

    pub fn error(self, error: MethodError) {
        self.finish(&MethodResult::Error(error));
    }

    /// Says this channel does not implement the method. On the wire it is an
    /// empty reply, which is what raises `MissingPluginException` at the far
    /// end -- and what an optional channel quietly expects.
    pub fn not_implemented(self) {
        self.finish(&MethodResult::NotImplemented);
    }

    pub fn finish(self, result: &MethodResult) {
        match (self.encode)(result) {
            Some(bytes) => (self.respond)(Some(&bytes)),
            None => (self.respond)(None),
        }
    }
}

/// A stream the platform pushes.
///
/// Upstream's `EventChannel`. The transport underneath is a method channel: the
/// framework calls `listen` to start and `cancel` to stop, and in between the
/// platform sends *envelopes* on the same channel -- a success envelope per
/// event, an error envelope for a failure, and an empty message to say the
/// stream is finished.
///
/// The shape is worth understanding rather than memorising: a stream is not a
/// second transport, it is a method channel used in both directions at once.
#[derive(Clone, Debug)]
pub struct EventChannel<C> {
    name: Cow<'static, str>,
    codec: C,
}

/// Where a stream's events go.
///
/// Three callbacks because a stream can end two ways, and a listener usually
/// cares which: `done` is the platform saying there is no more, `error` is it
/// saying something went wrong.
pub struct EventSink {
    pub event: Box<dyn FnMut(Value)>,
    pub error: Box<dyn FnMut(MethodError)>,
    pub done: Box<dyn FnMut()>,
}

impl EventSink {
    /// A sink that only cares about events, which is the common case.
    pub fn new(event: impl FnMut(Value) + 'static) -> EventSink {
        EventSink {
            event: Box::new(event),
            error: Box::new(|_error| {}),
            done: Box::new(|| {}),
        }
    }

    pub fn on_error(mut self, error: impl FnMut(MethodError) + 'static) -> EventSink {
        self.error = Box::new(error);
        self
    }

    pub fn on_done(mut self, done: impl FnMut() + 'static) -> EventSink {
        self.done = Box::new(done);
        self
    }
}

impl<C: MethodCodec + Copy + 'static> EventChannel<C> {
    pub const fn new(name: &'static str, codec: C) -> EventChannel<C> {
        EventChannel {
            name: Cow::Borrowed(name),
            codec,
        }
    }

    pub fn named(name: impl Into<String>, codec: C) -> EventChannel<C> {
        EventChannel {
            name: Cow::Owned(name.into()),
            codec,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Starts listening, and asks the platform to start sending.
    ///
    /// The handler goes on before the `listen` call, not after: the platform is
    /// entitled to send its first event from inside `listen` -- a stream of
    /// battery level starts by saying what the level is -- and registering
    /// afterwards would drop it.
    pub fn listen(&self, arguments: Value, mut sink: EventSink) {
        let codec = self.codec;
        super::set_handler(
            &self.name,
            Box::new(move |bytes, respond| {
                if bytes.is_empty() {
                    (sink.done)();
                } else {
                    match codec.decode_envelope(bytes) {
                        Ok(Some(event)) => (sink.event)(event),
                        Ok(None) => (sink.done)(),
                        Err(error) => (sink.error)(error),
                    }
                }
                // The platform is not waiting on an answer to an event, but the
                // handle has to be released either way.
                respond(None);
            }),
        );
        self.method_channel().invoke("listen", arguments);
    }

    /// Stops listening and tells the platform to stop sending.
    pub fn cancel(&self, arguments: Value) {
        self.method_channel().invoke("cancel", arguments);
        super::clear_handler(&self.name);
    }

    fn method_channel(&self) -> MethodChannel<C> {
        MethodChannel {
            name: self.name.clone(),
            codec: self.codec,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests_support::install;
    use super::*;
    use crate::services::codec::{JsonMethodCodec, StandardMessageCodec, StandardMethodCodec};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn a_method_call_goes_out_encoded_and_its_reply_comes_back_decoded() {
        let recorder = install();
        let channel = MethodChannel::new("test/method", StandardMethodCodec::new());

        let answer = Rc::new(RefCell::new(None));
        let recorded = answer.clone();
        channel.invoke_with_reply("add", Value::map([("a", Value::I32(1))]), move |reply| {
            *recorded.borrow_mut() = Some(reply)
        });

        let (name, bytes, response_id) = recorder.sent().remove(0);
        assert_eq!(name, "test/method");
        assert_eq!(
            StandardMethodCodec.decode_method_call(&bytes).unwrap(),
            MethodCall::new("add", Value::map([("a", Value::I32(1))]))
        );

        let envelope = StandardMethodCodec
            .encode_success_envelope(&Value::I32(2))
            .unwrap();
        recorder.reply(response_id, Some(&envelope));
        assert_eq!(*answer.borrow(), Some(Ok(Some(Value::I32(2)))));
    }

    #[test]
    fn an_error_envelope_comes_back_as_an_error() {
        let recorder = install();
        let channel = MethodChannel::new("test/method", JsonMethodCodec::new());

        let answer = Rc::new(RefCell::new(None));
        let recorded = answer.clone();
        channel.invoke_with_reply("fail", Value::Null, move |reply| {
            *recorded.borrow_mut() = Some(reply)
        });

        let (_, _, response_id) = recorder.sent().remove(0);
        let error = MethodError::new("unavailable", Some("no".to_string()));
        let envelope = JsonMethodCodec.encode_error_envelope(&error).unwrap();
        recorder.reply(response_id, Some(&envelope));
        assert_eq!(*answer.borrow(), Some(Err(error)));
    }

    #[test]
    fn an_empty_reply_is_no_handler_rather_than_a_null_result() {
        // The distinction MissingPluginException is built on. A channel nobody
        // implements has to be tellable from one that answered null.
        let recorder = install();
        let channel = MethodChannel::new("test/absent", StandardMethodCodec::new());

        let answer = Rc::new(RefCell::new(None));
        let recorded = answer.clone();
        channel.invoke_with_reply("anything", Value::Null, move |reply| {
            *recorded.borrow_mut() = Some(reply)
        });

        let (_, _, response_id) = recorder.sent().remove(0);
        recorder.reply(response_id, None);
        assert_eq!(*answer.borrow(), Some(Ok(None)));
    }

    #[test]
    fn an_incoming_call_reaches_its_handler_and_the_answer_goes_back_encoded() {
        let recorder = install();
        let channel = MethodChannel::new("test/incoming", StandardMethodCodec::new());
        channel.set_handler(|call, respond| {
            assert_eq!(call.method, "ping");
            respond.success(Value::from("pong"));
        });

        let call = StandardMethodCodec
            .encode_method_call(&MethodCall::new("ping", Value::Null))
            .unwrap();
        recorder.deliver("test/incoming", &call, 3);

        let (response_id, reply) = recorder.responses().remove(0);
        assert_eq!(response_id, 3);
        let reply = reply.expect("answered");
        assert_eq!(
            StandardMethodCodec.decode_envelope(&reply),
            Ok(Some(Value::from("pong")))
        );
    }

    #[test]
    fn a_handler_that_does_not_implement_a_method_answers_with_nothing_at_all() {
        let recorder = install();
        let channel = MethodChannel::new("test/partial", StandardMethodCodec::new());
        channel.set_handler(|call, respond| {
            if call.method == "known" {
                respond.success(Value::Null);
            } else {
                respond.not_implemented();
            }
        });

        let call = StandardMethodCodec
            .encode_method_call(&MethodCall::new("unknown", Value::Null))
            .unwrap();
        recorder.deliver("test/partial", &call, 8);
        assert_eq!(recorder.responses(), vec![(8, None)]);
    }

    #[test]
    fn a_basic_message_round_trips_through_its_codec() {
        let recorder = install();
        let channel = BasicMessageChannel::new("test/basic", StandardMessageCodec::new());

        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorded = seen.clone();
        channel.set_handler(move |message, respond| {
            recorded.borrow_mut().push(message);
            respond.reply(Some(&Value::from("ok")));
        });

        let message = StandardMessageCodec.encode(&Value::from("hello")).unwrap();
        recorder.deliver("test/basic", &message, 1);
        assert_eq!(seen.borrow().as_slice(), &[Value::from("hello")]);

        let (_, reply) = recorder.responses().remove(0);
        assert_eq!(
            StandardMessageCodec
                .decode(&reply.expect("answered"))
                .unwrap(),
            Value::from("ok")
        );
    }

    #[test]
    fn an_event_channel_listens_before_it_asks_to_be_sent_to() {
        // A platform stream is entitled to answer `listen` by sending its first
        // event synchronously, so the handler has to already be in place. This
        // asserts the order, then feeds an event through the same seam.
        let recorder = install();
        let channel = EventChannel::new("test/events", StandardMethodCodec::new());

        let events = Rc::new(RefCell::new(Vec::new()));
        let ended = Rc::new(RefCell::new(false));
        let recorded = events.clone();
        let finished = ended.clone();
        channel.listen(
            Value::Null,
            EventSink::new(move |event| recorded.borrow_mut().push(event))
                .on_done(move || *finished.borrow_mut() = true),
        );

        let (name, bytes, _) = recorder.sent().remove(0);
        assert_eq!(name, "test/events");
        assert_eq!(
            StandardMethodCodec
                .decode_method_call(&bytes)
                .unwrap()
                .method,
            "listen"
        );

        let event = StandardMethodCodec
            .encode_success_envelope(&Value::I32(1))
            .unwrap();
        recorder.deliver("test/events", &event, 0);
        assert_eq!(events.borrow().as_slice(), &[Value::I32(1)]);

        // An empty message is the stream ending, not an event with no value.
        recorder.deliver("test/events", &[], 0);
        assert!(*ended.borrow());
    }

    #[test]
    fn an_error_envelope_on_a_stream_reaches_the_error_callback() {
        // A stream can end two ways and a listener usually cares which: an
        // error is the platform saying something went wrong, not that there is
        // no more to come.
        let recorder = install();
        let channel = EventChannel::new("test/stream", StandardMethodCodec::new());

        let errors = Rc::new(RefCell::new(Vec::new()));
        let events = Rc::new(RefCell::new(Vec::new()));
        let recorded_errors = errors.clone();
        let recorded_events = events.clone();
        channel.listen(
            Value::Null,
            EventSink::new(move |event| recorded_events.borrow_mut().push(event))
                .on_error(move |error| recorded_errors.borrow_mut().push(error)),
        );

        let failure = MethodError::new("sensor-gone", Some("unplugged".to_string()));
        let envelope = StandardMethodCodec.encode_error_envelope(&failure).unwrap();
        recorder.deliver("test/stream", &envelope, 0);

        assert_eq!(errors.borrow().as_slice(), &[failure]);
        assert!(events.borrow().is_empty(), "an error is not an event");
    }

    #[test]
    fn cancelling_tells_the_platform_and_stops_listening() {
        let recorder = install();
        let channel = EventChannel::new("test/stream", StandardMethodCodec::new());

        let events = Rc::new(RefCell::new(Vec::new()));
        let recorded = events.clone();
        channel.listen(
            Value::Null,
            EventSink::new(move |event| recorded.borrow_mut().push(event)),
        );
        channel.cancel(Value::Null);

        let methods: Vec<String> = recorder
            .sent()
            .iter()
            .map(|(_, bytes, _)| {
                StandardMethodCodec
                    .decode_method_call(bytes)
                    .unwrap()
                    .method
            })
            .collect();
        assert_eq!(methods, vec!["listen", "cancel"]);

        // Anything still in flight when cancel was sent must not be delivered:
        // the sink it would go to is what the caller has just given up.
        let event = StandardMethodCodec
            .encode_success_envelope(&Value::I32(9))
            .unwrap();
        recorder.deliver("test/stream", &event, 0);
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn a_stream_can_be_cancelled_from_inside_its_own_event() {
        // The shape a "first value only" listener takes, and the one that needs
        // the handler to be able to unregister itself from within.
        let recorder = install();
        let channel = EventChannel::new("test/first", StandardMethodCodec::new());

        let events = Rc::new(RefCell::new(Vec::new()));
        let recorded = events.clone();
        let once = channel.clone();
        channel.listen(
            Value::Null,
            EventSink::new(move |event| {
                recorded.borrow_mut().push(event);
                once.cancel(Value::Null);
            }),
        );

        let event = StandardMethodCodec
            .encode_success_envelope(&Value::I32(1))
            .unwrap();
        recorder.deliver("test/first", &event, 0);
        let second = StandardMethodCodec
            .encode_success_envelope(&Value::I32(2))
            .unwrap();
        recorder.deliver("test/first", &second, 0);

        assert_eq!(events.borrow().as_slice(), &[Value::I32(1)]);
    }
}
