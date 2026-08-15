// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Keyboard input.
//!
//! # Why this is not shaped like the pointer
//!
//! A pointer event addresses itself. It carries coordinates, hit testing asks
//! the render tree who is at those coordinates, and the answer is unambiguous
//! without anybody having remembered anything. A key event carries no such
//! thing: `Escape` says which key, never to whom. Somebody has to have decided
//! in advance who is listening, and upstream that somebody is the focus tree --
//! `FocusManager.handleKeyMessage` starts at `primaryFocus`, walks up its
//! ancestors, and drops the event on the floor when there is no primary focus.
//!
//! There is no focus tree here yet. What there is instead is the layer upstream
//! runs *before* the focus walk: `FocusManager`'s early key handlers, which see
//! every key regardless of what has focus. That is [`Application::on_key`], and
//! it is the honest subset -- application-wide shortcuts, which is what a
//! window-level Escape or a Ctrl+O actually is. Per-widget keyboard handling
//! needs focus, and needs it to be real rather than approximated, so it waits.
//!
//! # What the host does and does not do
//!
//! The Windows host translates key messages, pairs a key down with the
//! character it produces, and sends both up. What it does not do is *consume*:
//! every key is also handed to `DefWindowProc`, so Alt+F4 and the system menu
//! keep working, and nothing here can suppress them.
//!
//! Upstream can suppress them, and the machinery for it is most of
//! `KeyboardManager`: because the framework answers asynchronously, an
//! unhandled key has to be re-posted to the message queue afterwards and then
//! recognised on the way back in so it is not handled twice.
//!
//! The answer itself does travel. A key arrives as a platform message on
//! `flutter/keydata`, and what [`Application::on_key`] returns becomes that
//! message's reply -- one byte, exactly as dart:ui writes it. What is missing is
//! a reader: no host re-posts, so today the return value only schedules a frame.
//!
//! # What is missing
//!
//! No text input. A character arrives on [`KeyEvent::character`], which is
//! enough to know that pressing `A` produced "a", but there is no IME
//! composition, no candidate window, and no editable text to put it in.

mod keys;

use std::collections::HashMap;

/// Where a key is on the keyboard, regardless of layout.
///
/// USB HID usage codes, the same values as upstream's `PhysicalKeyboardKey`.
/// This is the identity to track a press by: the layout can change between a
/// key going down and coming up, and the release still has to cancel the press.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct PhysicalKey(pub u64);

/// What a key means under the layout in force.
///
/// The same values as upstream's `LogicalKeyboardKey`. This is what a shortcut
/// is written against -- Ctrl+Z is Ctrl+Z wherever Z sits on an AZERTY board.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct LogicalKey(pub u64);

impl LogicalKey {
    /// The logical key a printable character stands for.
    ///
    /// Letters and digits are not in the generated table, because the rule that
    /// produces them is arithmetic: the value is the lower-cased character
    /// itself. `A` and `a` are one key and must be one value.
    pub const fn from_char(character: char) -> LogicalKey {
        let code = character as u32;
        let lowered = if code >= 'A' as u32 && code <= 'Z' as u32 {
            code + ('a' as u32 - 'A' as u32)
        } else {
            code
        };
        LogicalKey(lowered as u64)
    }
}

/// What happened to the key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyChange {
    Down,
    Up,
    /// The key was already down and the platform's auto-repeat fired.
    Repeat,
}

impl KeyChange {
    /// The wire values in `RfKeyEvent::change`, which mirror
    /// `flutter::KeyEventType`.
    pub(crate) fn from_code(code: i32) -> KeyChange {
        match code {
            1 => KeyChange::Up,
            2 => KeyChange::Repeat,
            _ => KeyChange::Down,
        }
    }
}

/// One key going down, coming up, or repeating.
#[derive(Clone, Debug)]
pub struct KeyEvent {
    pub change: KeyChange,
    pub physical: PhysicalKey,
    pub logical: LogicalKey,
    /// The text this key produced, if it produced any. `None` for Escape or an
    /// arrow key; `Some("a")` for the A key; `Some("é")` for a dead-key
    /// sequence, on the keystroke that completes it.
    ///
    /// Absent on a key up, because a release produces no text.
    pub character: Option<String>,
    /// True when the host invented this event rather than observing it.
    ///
    /// A modifier released while another window had the focus never sends its
    /// up message here, so the host reconciles what it has reported against
    /// what the platform says is held, and makes up the difference. Handlers
    /// that act on a key press usually want to ignore these; handlers that only
    /// read [`Keyboard`] never see them at all.
    pub synthesized: bool,
    pub time_stamp_micros: i64,
}

impl KeyEvent {
    /// Whether this is a press -- a first press or an auto-repeat.
    ///
    /// The distinction between the two matters for a held arrow key and not for
    /// much else, so most handlers want this rather than `change`.
    pub fn is_down(&self) -> bool {
        matches!(self.change, KeyChange::Down | KeyChange::Repeat)
    }
}

/// Which keys are held down.
///
/// The same job as upstream's `HardwareKeyboard`: an event says what changed,
/// and something has to remember the rest. Modifiers are the reason it exists
/// -- `Ctrl+O` is one key event plus a question about another key that is not
/// in it.
///
/// The map is physical to logical rather than a set of physical keys, so a
/// release can report the logical key the *press* had. Rotating the layout
/// while a key is held would otherwise produce a press of one key and a release
/// of another, and leave the first stuck down forever.
#[derive(Default, Debug)]
pub struct Keyboard {
    pressed: HashMap<PhysicalKey, LogicalKey>,
}

impl Keyboard {
    pub fn new() -> Keyboard {
        Keyboard::default()
    }

    /// Folds an event into the pressed set, correcting the logical key of a
    /// release to whatever the press reported.
    pub(crate) fn record(&mut self, event: &mut KeyEvent) {
        match event.change {
            KeyChange::Down => {
                self.pressed.insert(event.physical, event.logical);
            }
            KeyChange::Repeat => {
                // A repeat without a press means the key went down while
                // another window had focus. Treat it as the press.
                self.pressed.entry(event.physical).or_insert(event.logical);
            }
            KeyChange::Up => {
                if let Some(logical) = self.pressed.remove(&event.physical) {
                    event.logical = logical;
                }
            }
        }
    }

    pub fn is_pressed(&self, key: PhysicalKey) -> bool {
        self.pressed.contains_key(&key)
    }

    pub fn is_logical_pressed(&self, key: LogicalKey) -> bool {
        self.pressed.values().any(|held| *held == key)
    }

    pub fn pressed(&self) -> impl Iterator<Item = PhysicalKey> + '_ {
        self.pressed.keys().copied()
    }

    /// Either Control key.
    pub fn control(&self) -> bool {
        self.is_pressed(PhysicalKey::CONTROL_LEFT) || self.is_pressed(PhysicalKey::CONTROL_RIGHT)
    }

    /// Either Shift key.
    pub fn shift(&self) -> bool {
        self.is_pressed(PhysicalKey::SHIFT_LEFT) || self.is_pressed(PhysicalKey::SHIFT_RIGHT)
    }

    /// Either Alt key. The right one is AltGr on layouts that have one, where
    /// it is Control+Alt and this will also report `control`.
    pub fn alt(&self) -> bool {
        self.is_pressed(PhysicalKey::ALT_LEFT) || self.is_pressed(PhysicalKey::ALT_RIGHT)
    }

    /// Either Windows / Command key.
    pub fn meta(&self) -> bool {
        self.is_pressed(PhysicalKey::META_LEFT) || self.is_pressed(PhysicalKey::META_RIGHT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(change: KeyChange, physical: PhysicalKey, logical: LogicalKey) -> KeyEvent {
        KeyEvent {
            change,
            physical,
            logical,
            character: None,
            synthesized: false,
            time_stamp_micros: 0,
        }
    }

    #[test]
    fn a_press_is_remembered_and_a_release_forgets_it() {
        let mut keyboard = Keyboard::new();
        let mut down = event(KeyChange::Down, PhysicalKey::KEY_A, LogicalKey::KEY_A);
        keyboard.record(&mut down);
        assert!(keyboard.is_pressed(PhysicalKey::KEY_A));

        let mut up = event(KeyChange::Up, PhysicalKey::KEY_A, LogicalKey::KEY_A);
        keyboard.record(&mut up);
        assert!(!keyboard.is_pressed(PhysicalKey::KEY_A));
    }

    #[test]
    fn a_release_reports_the_logical_key_the_press_had() {
        // The layout changed while the key was held: the same physical key now
        // reports a different logical one. The release must still say `q`, or a
        // handler that matched the press cannot match the release.
        let mut keyboard = Keyboard::new();
        let mut down = event(KeyChange::Down, PhysicalKey::KEY_Q, LogicalKey::KEY_Q);
        keyboard.record(&mut down);

        let mut up = event(KeyChange::Up, PhysicalKey::KEY_Q, LogicalKey::KEY_A);
        keyboard.record(&mut up);
        assert_eq!(up.logical, LogicalKey::KEY_Q);
    }

    #[test]
    fn a_repeat_without_a_press_still_counts_as_held() {
        // The key went down while another window had focus, so only the repeats
        // arrive here. Ignoring them would leave a held modifier invisible.
        let mut keyboard = Keyboard::new();
        let mut repeat = event(
            KeyChange::Repeat,
            PhysicalKey::CONTROL_LEFT,
            LogicalKey::CONTROL_LEFT,
        );
        keyboard.record(&mut repeat);
        assert!(keyboard.control());
    }

    #[test]
    fn modifiers_answer_for_either_side() {
        let mut keyboard = Keyboard::new();
        let mut down = event(
            KeyChange::Down,
            PhysicalKey::SHIFT_RIGHT,
            LogicalKey::SHIFT_RIGHT,
        );
        keyboard.record(&mut down);
        assert!(keyboard.shift());
        assert!(!keyboard.control());
        assert!(!keyboard.alt());
        assert!(!keyboard.meta());
    }

    #[test]
    fn a_synthesized_release_frees_a_stuck_modifier() {
        // Ctrl was held, Alt+Tab took the focus away, and its real key up went
        // to the other window. The host notices on the next event and makes one
        // up; without this the modifier is held for the rest of the run.
        let mut keyboard = Keyboard::new();
        let mut down = event(
            KeyChange::Down,
            PhysicalKey::CONTROL_LEFT,
            LogicalKey::CONTROL_LEFT,
        );
        keyboard.record(&mut down);
        assert!(keyboard.control());

        let mut up = event(
            KeyChange::Up,
            PhysicalKey::CONTROL_LEFT,
            LogicalKey::CONTROL_LEFT,
        );
        up.synthesized = true;
        keyboard.record(&mut up);
        assert!(!keyboard.control());
    }

    #[test]
    fn letters_and_digits_are_their_own_character() {
        assert_eq!(LogicalKey::from_char('a'), LogicalKey::KEY_A);
        assert_eq!(LogicalKey::from_char('A'), LogicalKey::KEY_A);
        assert_eq!(LogicalKey::from_char('7'), LogicalKey::DIGIT_7);
    }

    #[test]
    fn the_generated_names_match_upstreams_values() {
        // Three spot checks against packages/flutter's keyboard_key.g.dart. If
        // the generator ever mangles the table these are what notice.
        assert_eq!(PhysicalKey::ESCAPE, PhysicalKey(0x00070029));
        assert_eq!(LogicalKey::ESCAPE, LogicalKey(0x0010000001b));
        assert_eq!(PhysicalKey::ARROW_LEFT, PhysicalKey(0x00070050));
    }

    #[test]
    fn the_wire_values_match_the_engines_key_event_type() {
        // flutter::KeyEventType is kDown, kUp, kRepeat in that order, and the
        // shell passes it through as an int. Getting this backwards would make
        // every press a release.
        assert_eq!(KeyChange::from_code(0), KeyChange::Down);
        assert_eq!(KeyChange::from_code(1), KeyChange::Up);
        assert_eq!(KeyChange::from_code(2), KeyChange::Repeat);
        assert_eq!(KeyChange::from_code(99), KeyChange::Down);
    }

    #[test]
    fn a_repeat_is_a_press_and_a_release_is_not() {
        let repeat = event(KeyChange::Repeat, PhysicalKey::KEY_A, LogicalKey::KEY_A);
        let up = event(KeyChange::Up, PhysicalKey::KEY_A, LogicalKey::KEY_A);
        assert!(repeat.is_down());
        assert!(!up.is_down());
    }
}
