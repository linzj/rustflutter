// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Platform messages: how the framework talks to the thing it is embedded in.
//!
//! A platform message is a channel name, a byte buffer and, optionally, a reply.
//! Everything the framework cannot do for itself travels this way -- the
//! clipboard, the mouse cursor, the system's idea of what time format to use,
//! and every plugin ever written. It is the only extension point the engine has,
//! and it is deliberately language-agnostic: the Android and iOS halves of an
//! existing plugin work against this without knowing Dart has been replaced.
//!
//! # The shape upstream
//!
//! Three layers, and they are three because each one is useful without the one
//! above it:
//!
//! | Layer | Upstream | Here |
//! |---|---|---|
//! | bytes on a channel | `BinaryMessenger` | this module's free functions |
//! | values, via a codec | `BasicMessageChannel` | [`BasicMessageChannel`] |
//! | calls and replies | `MethodChannel`, `EventChannel` | [`MethodChannel`], [`EventChannel`] |
//!
//! # Both directions, and why the reply is a callback
//!
//! A message can go either way and either direction may be answered. The answer
//! is a callback rather than a return value because the far end is not obliged
//! to be ready: an embedder asked for the clipboard has to talk to the operating
//! system, and a framework asked to handle a key has to wait for a build. In
//! Dart both directions are `Future`s; here they are `FnOnce`, which is the same
//! contract minus a runtime.
//!
//! # Messages that arrive before anybody is listening
//!
//! The embedder sends `flutter/lifecycle` as soon as the framework exists,
//! which is before any application code has run and therefore before any
//! handler is registered. Upstream keeps such messages in `ChannelBuffers` -- a
//! one-deep queue per channel, drained when a handler appears. That is ported
//! here for the same reason it exists there: without it, an application would
//! come up believing it is in whatever state it assumed by default, because the
//! message saying otherwise arrived a millisecond too early.
//!
//! Not every engine-owned channel reaches this far. `Engine::DispatchPlatform-
//! Message` answers `flutter/settings` and `flutter/localization` itself and
//! stops there, forwarding what they said through `SetUserSettingsData` and
//! `SetLocales` instead; `flutter/lifecycle` is explicitly *not* consumed
//! (`HandleLifecyclePlatformMessage` returns false so the framework sees it
//! too). That split is upstream's and is not ours to change.
//!
//! # One messenger, and never borrowed while user code runs
//!
//! There is exactly one messenger per application, because a channel is named
//! by a string and two messengers would mean two meanings for one name. It
//! lives in a `RefCell`, and every entry point below borrows it only long
//! enough to move something in or out -- never across a call into a handler or
//! a reply. That is not tidiness: a handler answering one call by making
//! another is ordinary, and a messenger borrowed across it would panic on the
//! way back in.
//!
//! Two consequences of that are worth writing down rather than defending. A
//! message arriving while a handler runs queues behind it without a limit --
//! the buffer's capacity is for messages nobody is listening to, and this one
//! has a listener -- so a handler that feeds its own channel in a loop grows
//! the queue until it stops. And the queue is drained by re-entering `deliver`,
//! so a long one nests rather than iterating. Neither is reachable from the
//! shell, which delivers one message at a time from one thread; both are
//! reachable from a framework caller that means to.
//!
//! The messenger is also per-thread and there is only ever one of it, which
//! makes two things silently wrong rather than loud: a second application on
//! the same thread shares its channel table, and a call made from a worker
//! thread finds a messenger with no shell behind it and is answered
//! immediately with "nobody handled it". Platform messages are a UI-thread
//! affair upstream too, but there it is enforced and here it is a convention.

pub mod asset_manifest;
pub mod channel;
pub mod codec;
pub mod keyboard_inserted_content;
pub mod process_text;
pub mod spell_check;
pub mod system;
pub mod text_boundary;
pub mod text_editing_delta;
pub mod text_input;

pub use channel::{BasicMessageChannel, EventChannel, EventSink, MethodChannel};
pub use codec::{
    BinaryCodec, CodecError, JsonMessageCodec, JsonMethodCodec, MessageCodec, MethodCall,
    MethodCodec, MethodError, MethodResult, StandardMessageCodec, StandardMethodCodec, StringCodec,
    Value,
};
pub use text_input::{
    TextEditingValue, TextInputAction, TextInputClient, TextInputConfiguration,
    TextInputConnection, TextInputType,
};

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

/// What a reply carries: the bytes, or nothing at all.
///
/// `None` is not an empty buffer. It means no handler existed, which is what
/// raises `MissingPluginException` upstream and what lets an optional channel
/// tell "the platform said nothing" from "the platform said null".
pub type ReplyData<'a> = Option<&'a [u8]>;

/// Called once when a reply arrives, or once with `None` if none ever will.
///
/// Always called exactly once. A reply that never came would leak whatever the
/// caller captured, and -- worse -- a `MethodChannel` caller would wait forever
/// for an answer that is not coming.
pub type ReplyCallback = Box<dyn FnOnce(ReplyData<'_>)>;

/// Answers a message the embedder sent. Call it exactly once.
pub type Responder = Box<dyn FnOnce(ReplyData<'_>)>;

/// Handles messages arriving on one channel.
pub type MessageHandler = Box<dyn FnMut(&[u8], Responder)>;

/// Where a message goes once the framework has finished with it.
///
/// The seam exists so the messenger can be driven without an engine behind it:
/// in a running application this is the shell, and in a test it is a recorder.
/// Upstream the equivalent seam is `TestDefaultBinaryMessenger`, and it exists
/// for the same reason -- the layer above is worth testing on its own.
pub trait PlatformSink {
    /// Sends a message out. `response_id` is 0 when no reply is wanted.
    fn send(&self, channel: &str, message: &[u8], response_id: i64);

    /// Answers a message that came in. `response_id` is the one that arrived
    /// with it.
    fn respond(&self, response_id: i64, reply: ReplyData<'_>);

    /// Tells the embedder a channel gained or lost its handler.
    ///
    /// Upstream this is `PlatformDispatcher.sendChannelUpdate`, and the Windows
    /// embedder acts on it: it holds back `flutter/lifecycle` until something is
    /// listening, because a lifecycle message nobody hears is a lifecycle
    /// message lost.
    fn channel_update(&self, channel: &str, listening: bool);

    /// Asks for a frame, because a handler has just run.
    ///
    /// A message arrives between frames and almost always changes something --
    /// a lifecycle handler that dims the window, a reply that fills in a list.
    /// Nothing else would ask: `set_state` marks the tree dirty but does not
    /// schedule, and the buffered messages drained by [`set_handler`] do not
    /// come in through the shell at all.
    fn request_frame(&self);
}

/// How many messages a channel holds for a handler that has not appeared yet.
///
/// One, which is upstream's default (`ChannelBuffers._defaultBufferSize`). One
/// is the right number for the channels that need it at all: `flutter/lifecycle`
/// is level-triggered -- each message replaces what the last one said -- so the
/// newest is the only one worth keeping.
const DEFAULT_BUFFER: usize = 1;

/// A channel's handler, and whatever arrived before it existed.
struct Channel {
    handler: Option<MessageHandler>,
    /// Messages and their response ids, oldest first.
    pending: VecDeque<(Vec<u8>, i64)>,
    capacity: usize,
    /// True while [`deliver`] has the handler out on loan, so it is not in
    /// `handler` even though the channel is listening. A second message
    /// arriving in that window -- a handler that sends on its own channel --
    /// must queue rather than be told there is nobody there.
    loaned: bool,
    /// Bumped by every [`set_handler`] and [`clear_handler`].
    ///
    /// This is what makes a handler able to unregister itself. `clear_handler`
    /// cannot take a handler that is out on loan -- it is not in the map to
    /// take -- so `deliver` compares the generation instead, and puts the
    /// handler back only if nothing replaced or removed it while it ran.
    /// Without this the handler would be restored on the way out and a
    /// one-shot listener could never stop.
    generation: u64,
}

impl Channel {
    fn new() -> Channel {
        Channel {
            handler: None,
            pending: VecDeque::new(),
            capacity: DEFAULT_BUFFER,
            loaned: false,
            generation: 0,
        }
    }

    fn is_listening(&self) -> bool {
        self.handler.is_some() || self.loaned
    }
}

/// The framework's end of every platform channel.
///
/// Held by the thread-local below and reached through this module's free
/// functions rather than directly, so that no borrow of it is ever live while a
/// handler runs. Upstream the counterpart is
/// `ServicesBinding.defaultBinaryMessenger`.
struct Messenger {
    channels: HashMap<String, Channel>,
    /// Replies we are waiting for, keyed by the id we sent out.
    waiting: HashMap<i64, ReplyCallback>,
    next_response_id: i64,
    /// Shared rather than owned because a [`Responder`] outlives the call that
    /// created it: a handler is entitled to answer from a worker's completion
    /// three frames later, and by then the borrow it was made from is gone.
    sink: Option<Rc<dyn PlatformSink>>,
}

impl Messenger {
    fn new() -> Messenger {
        Messenger {
            channels: HashMap::new(),
            waiting: HashMap::new(),
            // Starts at one because zero is the wire's "no reply wanted".
            next_response_id: 1,
            sink: None,
        }
    }

    fn channel(&mut self, name: &str) -> &mut Channel {
        self.channels
            .entry(name.to_string())
            .or_insert_with(Channel::new)
    }

    /// The callback a handler answers through.
    fn responder(&self, response_id: i64) -> Responder {
        // A message the embedder wants no answer to still gets a responder, so
        // a handler does not have to know the difference. That one is a sink.
        if response_id == 0 {
            return Box::new(|_reply| {});
        }
        let Some(sink) = self.sink.clone() else {
            return Box::new(|_reply| {});
        };
        let guard = ResponseGuard {
            sink,
            response_id,
            answered: std::cell::Cell::new(false),
        };
        Box::new(move |reply| guard.answer(reply))
    }
}

/// Makes sure a message is answered exactly once, even if nobody answers it.
///
/// The contract is that every response id comes back, because the far end is
/// waiting on it -- on Windows an uncompleted response handle is a
/// platform-thread task that never runs. A handler is free to forget, though:
/// it can return without calling its responder, or drop it inside a closure
/// that is itself dropped. This turns forgetting into an empty reply instead of
/// a caller that waits for the life of the process.
struct ResponseGuard {
    sink: Rc<dyn PlatformSink>,
    response_id: i64,
    answered: std::cell::Cell<bool>,
}

impl ResponseGuard {
    fn answer(&self, reply: ReplyData<'_>) {
        if self.answered.replace(true) {
            // Answered twice. The first one has gone already, and a second
            // would reach a handle the shell has released.
            return;
        }
        self.sink.respond(self.response_id, reply);
    }
}

impl Drop for ResponseGuard {
    fn drop(&mut self) {
        if !self.answered.get() {
            self.sink.respond(self.response_id, None);
        }
    }
}

thread_local! {
    /// The application's messenger.
    ///
    /// Thread-local rather than a `static` with a lock because platform
    /// messages are a UI-thread affair from end to end: the shell delivers them
    /// on the UI task runner, handlers run there, and replies go back the same
    /// way. A lock would be guarding against a caller that does not exist.
    static MESSENGER: RefCell<Messenger> = RefCell::new(Messenger::new());
}

/// Borrows the messenger for as long as it takes to move something.
///
/// `body` must not call back into this module. Everything below respects that
/// by finishing with the messenger before running any handler or callback.
fn with_messenger<R>(body: impl FnOnce(&mut Messenger) -> R) -> R {
    MESSENGER.with(|messenger| body(&mut messenger.borrow_mut()))
}

// -- Wiring the shell in ------------------------------------------------------

/// Points the messenger at the embedder. Called once, when the shell creates
/// the application.
pub fn attach(sink: Rc<dyn PlatformSink>) {
    with_messenger(|messenger| messenger.sink = Some(sink));
    // Before any application code runs, and upstream's `ServicesBinding`
    // installs its handlers at the same moment for the same reason: the close
    // button must work in an application that has never heard of the exit
    // protocol, and registering the handler is what tells the embedder there
    // is anybody to ask. See system::install_exit_handler.
    system::install_exit_handler();
}

/// Drops the embedder and fails everything still waiting.
///
/// The failures are the point. A caller blocked on a reply the shell was going
/// to deliver has to be told it is not coming, or it waits for the life of the
/// process.
pub fn detach() {
    // Before the channels are cleared, because it drops what the application
    // put there rather than only what `attach` did.
    system::remove_exit_handler();
    let waiting = with_messenger(|messenger| {
        messenger.sink = None;
        messenger.channels.clear();
        std::mem::take(&mut messenger.waiting)
    });
    for (_, callback) in waiting {
        callback(None);
    }
}

/// True once the messenger can reach the embedder. False in a headless render
/// or a unit test that has not installed a sink.
pub fn is_attached() -> bool {
    with_messenger(|messenger| messenger.sink.is_some())
}

// -- Sending ------------------------------------------------------------------

/// Sends a message and forgets about it.
pub fn send(channel: &str, message: &[u8]) {
    let sink = with_messenger(|messenger| messenger.sink.clone());
    if let Some(sink) = sink {
        sink.send(channel, message, 0);
    }
}

/// Sends a message and waits for the reply.
///
/// `callback` runs exactly once. With no embedder to send to it runs
/// immediately with `None`, which is the same answer a missing handler on the
/// far end would give -- a headless render is a platform with no plugins, not a
/// platform that hangs.
pub fn send_with_reply(channel: &str, message: &[u8], callback: ReplyCallback) {
    // The sink is looked up before the callback is put anywhere, so that the
    // no-embedder case still has it to hand back.
    let sink = with_messenger(|messenger| messenger.sink.clone());
    let Some(sink) = sink else {
        callback(None);
        return;
    };
    let response_id = with_messenger(|messenger| {
        let response_id = messenger.next_response_id;
        messenger.next_response_id += 1;
        messenger.waiting.insert(response_id, callback);
        response_id
    });
    sink.send(channel, message, response_id);
}

/// Delivers a reply to a message the framework sent. Called by the shell.
/// Returns whether anybody was waiting. The shell uses the answer to decide
/// whether the frame the reply probably changed is worth asking for.
pub fn complete_reply(response_id: i64, reply: ReplyData<'_>) -> bool {
    let callback = with_messenger(|messenger| messenger.waiting.remove(&response_id));
    match callback {
        Some(callback) => {
            callback(reply);
            // Same reason as after a handler: the answer has almost certainly
            // changed what should be on screen, and nothing else will ask.
            request_frame();
            true
        }
        None => false,
    }
}

// -- Receiving ----------------------------------------------------------------

/// Registers the handler for a channel, replacing any previous one.
///
/// Anything that arrived while the channel had no handler is delivered now,
/// oldest first -- see the note on buffering in the module docs.
pub fn set_handler(channel: &str, handler: MessageHandler) {
    let (announce, pending) = with_messenger(|messenger| {
        let entry = messenger.channel(channel);
        let was_listening = entry.is_listening();
        entry.handler = Some(handler);
        entry.generation += 1;
        let pending: Vec<(Vec<u8>, i64)> = entry.pending.drain(..).collect();
        (!was_listening, pending)
    });

    if announce {
        channel_update(channel, true);
    }
    for (message, response_id) in pending {
        deliver(channel, &message, response_id);
    }
}

/// Removes a channel's handler. Messages arriving afterwards are buffered again.
///
/// Works from inside the handler itself, which is how a one-shot listener stops
/// listening. See the note on `Channel::generation` for why that needs saying.
pub fn clear_handler(channel: &str) {
    let announce = with_messenger(|messenger| {
        let Some(entry) = messenger.channels.get_mut(channel) else {
            return false;
        };
        if !entry.is_listening() {
            return false;
        }
        entry.handler = None;
        // The loan is ended here rather than in `deliver`: the channel has
        // stopped listening as of now, so a message arriving before the handler
        // returns must be buffered rather than queued behind it.
        entry.loaned = false;
        entry.generation += 1;
        true
    });
    if announce {
        channel_update(channel, false);
    }
}

pub fn has_handler(channel: &str) -> bool {
    with_messenger(|messenger| {
        messenger
            .channels
            .get(channel)
            .is_some_and(Channel::is_listening)
    })
}

/// How many messages this channel keeps for a handler that has not arrived.
///
/// Upstream an application sets this with
/// `ServicesBinding.instance.channelBuffers.resize`, and the reason it is
/// adjustable is that not every channel is level-triggered: a channel reporting
/// *events* rather than *state* loses information when the queue drops the
/// older one.
pub fn resize(channel: &str, capacity: usize) {
    let dropped = with_messenger(|messenger| {
        let entry = messenger.channel(channel);
        entry.capacity = capacity;
        let mut dropped = Vec::new();
        while entry.pending.len() > capacity {
            if let Some((_, response_id)) = entry.pending.pop_front() {
                dropped.push(response_id);
            }
        }
        dropped
    });
    discard_all(&dropped);
}

/// Takes in a message from the embedder. Called by the shell.
///
/// `response_id` is 0 when the embedder wants no reply.
///
/// Returns whether a handler saw it. A message that was only buffered changed
/// nothing yet, so there is no frame to ask for.
pub fn handle_platform_message(channel: &str, message: &[u8], response_id: i64) -> bool {
    // Three cases, not two. A channel whose handler is *running* is listening
    // but cannot be called again -- the handler is `FnMut` and is out on loan --
    // so the message waits behind the one being handled rather than being told
    // there is nobody there. Upstream has no such state, because a Dart handler
    // is a function that can be entered twice; this is what that costs.
    let route = with_messenger(|messenger| match messenger.channels.get(channel) {
        Some(entry) if entry.handler.is_some() => Route::Deliver,
        Some(entry) if entry.loaned => Route::Defer,
        _ => Route::Buffer,
    });

    match route {
        Route::Deliver => {
            deliver(channel, message, response_id);
            return true;
        }
        Route::Defer => {
            // Not subject to the capacity limit: this message is handled, only
            // not yet. Dropping it would lose a message on a channel that has a
            // listener, which the buffer is not for.
            with_messenger(|messenger| {
                messenger
                    .channel(channel)
                    .pending
                    .push_back((message.to_vec(), response_id));
            });
            return true;
        }
        Route::Buffer => {}
    }

    let dropped = with_messenger(|messenger| {
        let entry = messenger.channel(channel);
        if entry.capacity == 0 {
            // Explicitly not buffered. Answered rather than dropped silently,
            // so the embedder's handle is released.
            return vec![response_id];
        }
        entry.pending.push_back((message.to_vec(), response_id));
        let mut dropped = Vec::new();
        while entry.pending.len() > entry.capacity {
            if let Some((_, id)) = entry.pending.pop_front() {
                // The oldest goes, and it is answered on the way out. An
                // unanswered response handle is a leak in the shell, not just a
                // caller left waiting.
                dropped.push(id);
            }
        }
        dropped
    });
    discard_all(&dropped);
    false
}

/// What [`handle_platform_message`] decided to do with a message.
enum Route {
    /// A handler is free; call it.
    Deliver,
    /// A handler exists but is running; queue behind it.
    Defer,
    /// Nothing is listening; hold it until something is.
    Buffer,
}

/// Runs a channel's handler with the handler out of the map and no borrow held.
///
/// Out of the map because a handler is entitled to touch the messenger while it
/// runs -- answering a method call by sending one is ordinary. It goes back
/// afterwards unless the handler replaced it, which is also ordinary: a handler
/// that unregisters itself is how a one-shot listener stops listening.
fn deliver(channel: &str, message: &[u8], response_id: i64) {
    let taken = with_messenger(|messenger| {
        let borrowed = messenger.channels.get_mut(channel).and_then(|entry| {
            let handler = entry.handler.take()?;
            entry.loaned = true;
            Some((handler, entry.generation))
        })?;
        Some((borrowed, messenger.responder(response_id)))
    });

    let Some(((mut handler, generation), responder)) = taken else {
        discard_all(&[response_id]);
        return;
    };

    handler(message, responder);

    // The handler has almost certainly changed something, and nothing else will
    // ask for the frame that shows it -- including on this path, where the
    // message may have come out of the buffer rather than off the wire.
    request_frame();

    // Anything that arrived while the handler was out on loan queued up behind
    // it; it is delivered as soon as the handler is back, in arrival order.
    let queued = with_messenger(|messenger| {
        let Some(entry) = messenger.channels.get_mut(channel) else {
            return Vec::new();
        };
        // The loan is over whatever else happened.
        entry.loaned = false;
        if entry.generation == generation {
            entry.handler = Some(handler);
        }
        // Otherwise it was replaced or removed while it ran, and putting it
        // back would undo the call that did so. `handler` is dropped here.
        if entry.handler.is_some() {
            entry.pending.drain(..).collect()
        } else {
            // Nothing is listening any more. What queued behind the loan stays
            // buffered for whoever listens next, rather than being answered
            // with nothing.
            Vec::new()
        }
    });
    for (message, response_id) in queued {
        deliver(channel, &message, response_id);
    }
}

/// Asks the shell for a frame. Silent when there is no shell.
fn request_frame() {
    let sink = with_messenger(|messenger| messenger.sink.clone());
    if let Some(sink) = sink {
        sink.request_frame();
    }
}

fn channel_update(channel: &str, listening: bool) {
    let sink = with_messenger(|messenger| messenger.sink.clone());
    if let Some(sink) = sink {
        sink.channel_update(channel, listening);
    }
}

fn discard_all(response_ids: &[i64]) {
    let sink = with_messenger(|messenger| messenger.sink.clone());
    let Some(sink) = sink else {
        return;
    };
    for response_id in response_ids {
        if *response_id != 0 {
            sink.respond(*response_id, None);
        }
    }
}

// -- Test support -------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use std::cell::RefCell as StdRefCell;

    #[derive(Default)]
    pub struct Recording {
        pub sent: Vec<(String, Vec<u8>, i64)>,
        pub responses: Vec<(i64, Option<Vec<u8>>)>,
        pub updates: Vec<(String, bool)>,
        pub frames: usize,
    }

    /// Stands in for the shell: records what the framework sent, and lets a
    /// test push messages and replies back the other way.
    #[derive(Clone, Default)]
    pub struct Recorder(Rc<StdRefCell<Recording>>);

    impl PlatformSink for Recorder {
        fn send(&self, channel: &str, message: &[u8], response_id: i64) {
            self.0
                .borrow_mut()
                .sent
                .push((channel.to_string(), message.to_vec(), response_id));
        }

        fn respond(&self, response_id: i64, reply: ReplyData<'_>) {
            self.0
                .borrow_mut()
                .responses
                .push((response_id, reply.map(<[u8]>::to_vec)));
        }

        fn channel_update(&self, channel: &str, listening: bool) {
            self.0
                .borrow_mut()
                .updates
                .push((channel.to_string(), listening));
        }

        fn request_frame(&self) {
            self.0.borrow_mut().frames += 1;
        }
    }

    impl Recorder {
        pub fn frames(&self) -> usize {
            self.0.borrow().frames
        }

        pub fn sent(&self) -> Vec<(String, Vec<u8>, i64)> {
            self.0.borrow().sent.clone()
        }

        pub fn responses(&self) -> Vec<(i64, Option<Vec<u8>>)> {
            self.0.borrow().responses.clone()
        }

        pub fn updates(&self) -> Vec<(String, bool)> {
            self.0.borrow().updates.clone()
        }

        /// Pushes a message in, as the shell would.
        pub fn deliver(&self, channel: &str, message: &[u8], response_id: i64) {
            handle_platform_message(channel, message, response_id);
        }

        /// Answers a message the framework sent.
        pub fn reply(&self, response_id: i64, reply: ReplyData<'_>) {
            complete_reply(response_id, reply);
        }
    }

    /// Clears the messenger and installs a fresh recorder.
    ///
    /// The messenger is per-thread and the tests share a thread -- `.cargo/config.toml`
    /// pins them to one because the engine's text stack is single-threaded --
    /// so each test has to start from nothing or it inherits the last one's
    /// channels.
    pub fn install() -> Recorder {
        with_messenger(|messenger| *messenger = Messenger::new());
        let recorder = Recorder::default();
        attach(Rc::new(recorder.clone()));
        recorder
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::install;
    use super::*;
    use std::cell::RefCell as StdRefCell;

    /// The listening states reported for one channel, in order.
    ///
    /// Filtered rather than compared whole because `attach` registers the exit
    /// handler on `flutter/platform`, so every recorder starts with one update
    /// already in it.
    fn updates_for(recorder: &tests_support::Recorder, channel: &str) -> Vec<bool> {
        recorder
            .updates()
            .into_iter()
            .filter(|(name, _)| name == channel)
            .map(|(_, listening)| listening)
            .collect()
    }

    fn collector() -> (Rc<StdRefCell<Vec<Vec<u8>>>>, MessageHandler) {
        let seen = Rc::new(StdRefCell::new(Vec::new()));
        let recorded = seen.clone();
        (
            seen,
            Box::new(move |message: &[u8], _respond: Responder| {
                recorded.borrow_mut().push(message.to_vec())
            }),
        )
    }

    #[test]
    fn a_message_reaches_its_handler() {
        let _recorder = install();
        let (seen, handler) = collector();
        set_handler("test/echo", handler);

        handle_platform_message("test/echo", b"ping", 0);
        assert_eq!(seen.borrow().as_slice(), &[b"ping".to_vec()]);
    }

    #[test]
    fn a_message_that_arrives_early_is_kept_until_somebody_listens() {
        // The case this exists for: the embedder sends flutter/lifecycle
        // before any application code has run, so nothing is listening yet.
        // Losing it means starting up believing the window is inactive.
        let _recorder = install();
        handle_platform_message("flutter/lifecycle", b"AppLifecycleState.resumed", 0);

        let (seen, handler) = collector();
        set_handler("flutter/lifecycle", handler);
        assert_eq!(
            seen.borrow().as_slice(),
            &[b"AppLifecycleState.resumed".to_vec()]
        );
    }

    #[test]
    fn the_buffer_keeps_the_newest_and_answers_what_it_drops() {
        // One deep by default, and the newest is the one worth keeping: these
        // channels report state, not events. What matters as much is that the
        // dropped message is answered -- an unanswered response id is a handle
        // the shell never frees.
        let recorder = install();
        handle_platform_message("flutter/lifecycle", b"first", 11);
        handle_platform_message("flutter/lifecycle", b"second", 12);
        assert_eq!(recorder.responses(), vec![(11, None)]);

        let (seen, handler) = collector();
        set_handler("flutter/lifecycle", handler);
        assert_eq!(seen.borrow().as_slice(), &[b"second".to_vec()]);
    }

    #[test]
    fn a_resized_channel_keeps_what_it_was_told_to() {
        let _recorder = install();
        resize("test/events", 3);
        for index in 0..3u8 {
            handle_platform_message("test/events", &[index], 0);
        }

        let (seen, handler) = collector();
        set_handler("test/events", handler);
        assert_eq!(seen.borrow().as_slice(), &[vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn a_channel_resized_to_nothing_answers_instead_of_queueing() {
        let recorder = install();
        resize("test/none", 0);
        handle_platform_message("test/none", b"x", 5);
        assert_eq!(recorder.responses(), vec![(5, None)]);
    }

    #[test]
    fn a_reply_finds_the_caller_that_is_waiting_for_it() {
        let recorder = install();
        let answer = Rc::new(StdRefCell::new(None));
        let recorded = answer.clone();
        send_with_reply(
            "test/ask",
            b"?",
            Box::new(move |reply| *recorded.borrow_mut() = Some(reply.map(<[u8]>::to_vec))),
        );

        let (channel, message, response_id) = recorder.sent().remove(0);
        assert_eq!(channel, "test/ask");
        assert_eq!(message, b"?");
        assert_ne!(
            response_id, 0,
            "a reply was asked for, so an id must have been allocated"
        );

        complete_reply(response_id, Some(b"!"));
        assert_eq!(*answer.borrow(), Some(Some(b"!".to_vec())));
    }

    #[test]
    fn detaching_fails_everything_still_waiting() {
        // A caller must not outlive its answer. Nothing else will complete
        // these once the shell is gone.
        let _recorder = install();
        let answered = Rc::new(StdRefCell::new(false));
        let recorded = answered.clone();
        send_with_reply(
            "test/ask",
            b"?",
            Box::new(move |reply| {
                assert!(reply.is_none());
                *recorded.borrow_mut() = true;
            }),
        );
        assert!(!*answered.borrow());
        detach();
        assert!(*answered.borrow());
    }

    #[test]
    fn a_handler_may_use_the_messenger_while_it_runs() {
        // A method call answered by making another one is ordinary. It is also
        // the case that a messenger borrowed across the handler would panic on,
        // which is why every entry point here lets the borrow go first.
        let recorder = install();
        set_handler(
            "test/outer",
            Box::new(|_message, respond| {
                send("test/inner", b"onward");
                respond(Some(b"done"));
            }),
        );
        handle_platform_message("test/outer", b"go", 7);

        assert_eq!(recorder.sent()[0].0, "test/inner");
        assert_eq!(recorder.responses(), vec![(7, Some(b"done".to_vec()))]);
        assert!(
            has_handler("test/outer"),
            "the handler must survive being run"
        );
    }

    #[test]
    fn a_message_arriving_while_the_handler_runs_is_queued_not_dropped() {
        // A handler that sends on its own channel, or a reply that arrives
        // re-entrantly, must not find its own channel unhandled -- the handler
        // is out on loan, not gone.
        let _recorder = install();
        let seen = Rc::new(StdRefCell::new(Vec::new()));
        let recorded = seen.clone();
        let reentered = Rc::new(StdRefCell::new(false));
        let once = reentered.clone();
        set_handler(
            "test/loop",
            Box::new(move |message, _respond| {
                recorded.borrow_mut().push(message.to_vec());
                if !*once.borrow() {
                    *once.borrow_mut() = true;
                    handle_platform_message("test/loop", b"second", 0);
                }
            }),
        );
        handle_platform_message("test/loop", b"first", 0);
        assert_eq!(
            seen.borrow().as_slice(),
            &[b"first".to_vec(), b"second".to_vec()]
        );
    }

    #[test]
    fn a_handler_can_unregister_itself() {
        // A one-shot listener, and the case a naive loan gets wrong: the
        // handler is not in the map while it runs, so `clear_handler` has
        // nothing to take, and putting it back afterwards would undo the call.
        let _recorder = install();
        let seen = Rc::new(StdRefCell::new(0));
        let counted = seen.clone();
        set_handler(
            "test/once",
            Box::new(move |_message, _respond| {
                *counted.borrow_mut() += 1;
                clear_handler("test/once");
            }),
        );

        handle_platform_message("test/once", b"first", 0);
        assert!(!has_handler("test/once"), "it unregistered itself");
        handle_platform_message("test/once", b"second", 0);
        assert_eq!(*seen.borrow(), 1, "the second message must not reach it");
    }

    #[test]
    fn unregistering_from_inside_tells_the_embedder_once() {
        let recorder = install();
        set_handler(
            "test/once",
            Box::new(|_message, _respond| clear_handler("test/once")),
        );
        handle_platform_message("test/once", b"go", 0);
        // Registering, then the handler removing itself. Not two of either.
        assert_eq!(updates_for(&recorder, "test/once"), vec![true, false]);
    }

    #[test]
    fn a_handler_that_replaces_itself_keeps_the_replacement() {
        let _recorder = install();
        let seen = Rc::new(StdRefCell::new(Vec::new()));
        let first = seen.clone();
        let second = seen.clone();
        set_handler(
            "test/swap",
            Box::new(move |_message, _respond| {
                first.borrow_mut().push("first");
                let inner = second.clone();
                set_handler(
                    "test/swap",
                    Box::new(move |_message, _respond| inner.borrow_mut().push("second")),
                );
            }),
        );

        handle_platform_message("test/swap", b"a", 0);
        handle_platform_message("test/swap", b"b", 0);
        assert_eq!(seen.borrow().as_slice(), &["first", "second"]);
    }

    #[test]
    fn a_message_deferred_behind_a_handler_that_then_leaves_is_kept_not_lost() {
        // The handler queues a message behind itself and then unregisters. The
        // queued one has nobody to go to, so it waits rather than being
        // answered with nothing.
        let _recorder = install();
        let seen = Rc::new(StdRefCell::new(Vec::new()));
        let recorded = seen.clone();
        set_handler(
            "test/leave",
            Box::new(move |message, _respond| {
                recorded.borrow_mut().push(message.to_vec());
                if message == b"first" {
                    handle_platform_message("test/leave", b"queued", 0);
                    clear_handler("test/leave");
                }
            }),
        );
        handle_platform_message("test/leave", b"first", 0);
        assert_eq!(seen.borrow().as_slice(), &[b"first".to_vec()]);

        let later = seen.clone();
        set_handler(
            "test/leave",
            Box::new(move |message, _respond| later.borrow_mut().push(message.to_vec())),
        );
        assert_eq!(
            seen.borrow().as_slice(),
            &[b"first".to_vec(), b"queued".to_vec()],
            "the queued message waited for the next listener"
        );
    }

    #[test]
    fn running_a_handler_asks_for_a_frame_even_out_of_the_buffer() {
        // The message that arrives before anybody is listening is delivered by
        // `set_handler`, which the shell never sees -- so if the frame were
        // asked for at the ABI instead of here, whatever that handler changed
        // would sit unpainted until something else caused a frame.
        let recorder = install();
        handle_platform_message("test/early", b"x", 0);
        assert_eq!(recorder.frames(), 0, "buffering alone changes nothing");

        set_handler("test/early", Box::new(|_message, _respond| {}));
        assert_eq!(recorder.frames(), 1, "draining the buffer ran a handler");

        handle_platform_message("test/early", b"y", 0);
        assert_eq!(recorder.frames(), 2);
    }

    #[test]
    fn a_reply_asks_for_a_frame_and_an_unclaimed_one_does_not() {
        let recorder = install();
        send_with_reply("test/ask", b"?", Box::new(|_reply| {}));
        let (_, _, response_id) = recorder.sent().remove(0);
        assert_eq!(recorder.frames(), 0);

        complete_reply(response_id, Some(b"!"));
        assert_eq!(recorder.frames(), 1);

        // Answered twice, or answered after the caller gave up. Nobody is
        // waiting, so there is nothing to repaint.
        complete_reply(response_id, Some(b"!"));
        assert_eq!(recorder.frames(), 1);
    }

    #[test]
    fn a_handler_that_forgets_to_answer_still_answers() {
        // The invariant the shell depends on: every response id comes back.
        // A handler that simply returns would otherwise leave a platform-thread
        // task that never runs.
        let recorder = install();
        set_handler("test/forgetful", Box::new(|_message, _respond| {}));
        handle_platform_message("test/forgetful", b"?", 6);
        assert_eq!(recorder.responses(), vec![(6, None)]);
    }

    #[test]
    fn answering_twice_only_answers_once() {
        let recorder = install();
        set_handler(
            "test/eager",
            Box::new(|_message, respond| {
                respond(Some(b"first"));
                // The responder is consumed by the call above, so a handler
                // cannot literally do this -- but a guard that answered on drop
                // without remembering would, which is what this pins.
            }),
        );
        handle_platform_message("test/eager", b"?", 7);
        assert_eq!(recorder.responses(), vec![(7, Some(b"first".to_vec()))]);
    }

    #[test]
    fn a_handler_answers_through_the_sink() {
        let recorder = install();
        set_handler(
            "test/ask",
            Box::new(|_message, respond| respond(Some(b"answered"))),
        );
        handle_platform_message("test/ask", b"?", 9);
        assert_eq!(recorder.responses(), vec![(9, Some(b"answered".to_vec()))]);
    }

    #[test]
    fn an_answer_may_come_after_the_handler_has_returned() {
        // The case that decides how the responder finds its way out: a handler
        // that has to wait for a worker answers from a callback, long after the
        // borrow it was made from is gone.
        let recorder = install();
        let held: Rc<StdRefCell<Option<Responder>>> = Rc::new(StdRefCell::new(None));
        let keeper = held.clone();
        set_handler(
            "test/slow",
            Box::new(move |_message, respond| *keeper.borrow_mut() = Some(respond)),
        );
        handle_platform_message("test/slow", b"?", 4);
        assert!(recorder.responses().is_empty(), "not answered yet");

        let responder = held.borrow_mut().take().expect("the handler kept it");
        responder(Some(b"late"));
        assert_eq!(recorder.responses(), vec![(4, Some(b"late".to_vec()))]);
    }

    #[test]
    fn the_embedder_is_told_when_a_channel_starts_and_stops_listening() {
        // Windows holds back flutter/lifecycle until something is listening.
        let recorder = install();
        set_handler("flutter/lifecycle", Box::new(|_message, _respond| {}));
        set_handler("flutter/lifecycle", Box::new(|_message, _respond| {}));
        clear_handler("flutter/lifecycle");
        assert_eq!(
            updates_for(&recorder, "flutter/lifecycle"),
            vec![true, false],
            "the update reports a change in state, not every registration"
        );
    }

    #[test]
    fn attaching_says_there_is_somebody_to_ask_before_closing() {
        // Not an implementation detail of `attach`: on Windows this update is
        // what turns the embedder's WM_CLOSE handling on, so an application
        // that never mentions the exit protocol still gets asked -- and still
        // closes, because the default answer is `exit`.
        let recorder = install();
        assert_eq!(updates_for(&recorder, "flutter/platform"), vec![true]);
        assert!(has_handler("flutter/platform"));

        detach();
        assert_eq!(
            updates_for(&recorder, "flutter/platform"),
            vec![true, false],
            "an embedder that is told nobody is listening closes the window itself"
        );
    }
}
