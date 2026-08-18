//! The shortcut system, from upstream `widgets/shortcuts.dart`: activators
//! say which keystroke they mean, a registry maps activators to intents,
//! and the manager turns a key event into the first matching intent. The
//! crate's keyboard layer (keyboard/) already produces the events; this
//! module gives them meanings, and the action dispatcher (actions.rs)
//! already knows what to do with those.
//!
//! Recorded divergence (see PORTING_STATUS.md): upstream's `Shortcuts`
//! widget scopes a registry to a subtree through the element tree; here a
//! registry is a value the keyboard-handling region owns, the same seam
//! `Focus::with_on_key` already is.

use std::collections::HashMap;
use std::rc::Rc;

use crate::actions::Intent;
use crate::focus::KeyResult;
use crate::keyboard::{KeyEvent, Keyboard, LogicalKey};

/// Upstream `KeySet<LogicalKeyboardKey>`: the set of logical keys an
/// activator means. Order-free; comparison as a set.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LogicalKeySet {
    pub keys: Vec<u64>,
}

impl LogicalKeySet {
    pub fn new(keys: &[u64]) -> LogicalKeySet {
        let mut keys = keys.to_vec();
        keys.sort_unstable();
        keys.dedup();
        LogicalKeySet { keys }
    }

    /// One key, the common case.
    pub fn single(key: u64) -> LogicalKeySet {
        LogicalKeySet::new(&[key])
    }

    pub fn contains(&self, key: u64) -> bool {
        self.keys.contains(&key)
    }
}

/// Upstream `ShortcutActivator`: the thing that says whether a key event is
/// the one it means. The closed set -- the two activators upstream's
/// widgets actually use.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ShortcutActivator {
    /// `LogicalKeySet`: exactly these logical keys held, no more.
    KeySet(LogicalKeySet),
    /// `SingleActivator`: one key, with the modifiers demanded exactly.
    Single {
        key: u64,
        control: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    },
    /// `CharacterActivator`: a character, with optional control and single
    /// modifier demands.
    Character {
        character: char,
        control: bool,
        single_modifier: bool,
    },
}

impl ShortcutActivator {
    /// Upstream `accepts(KeyEvent, ShortcutRegistry)`: whether this event,
    /// against the keyboard's current held-set, is this activator's
    /// keystroke.
    pub fn accepts(&self, event: &KeyEvent, keyboard: &Keyboard) -> bool {
        if !event.is_down() {
            return false;
        }
        match self {
            ShortcutActivator::KeySet(set) => {
                // The event's own key is in the set, every key of the set
                // is held, and nothing else is (upstream compares the
                // whole pressed set).
                set.contains(event.logical.0)
                    && set
                        .keys
                        .iter()
                        .all(|key| keyboard.is_logical_pressed(LogicalKey(*key)))
                    && keyboard.pressed().count() == set.keys.len()
            }
            ShortcutActivator::Single {
                key,
                control,
                shift,
                alt,
                meta,
            } => {
                event.logical.0 == *key
                    && keyboard.control() == *control
                    && keyboard.shift() == *shift
                    && keyboard.alt() == *alt
                    && keyboard.meta() == *meta
            }
            ShortcutActivator::Character {
                character,
                control,
                single_modifier,
            } => {
                event.character.as_deref() == Some(character.to_string().as_str())
                    && keyboard.control() == *control
                    && (!*single_modifier
                        || usize::from(keyboard.shift())
                            + usize::from(keyboard.alt())
                            + usize::from(keyboard.meta())
                            + usize::from(keyboard.control())
                            == 1)
            }
        }
    }
}

/// Upstream `ShortcutRegistry` + `ShortcutManager` together: the
/// activator-to-intent map and the matching, in the order the entries were
/// added -- upstream's manager is a `Map` with insertion order, and the
/// first match wins.
#[derive(Clone, Default)]
pub struct ShortcutRegistry {
    entries: Vec<(ShortcutActivator, Intent)>,
}

impl ShortcutRegistry {
    pub fn new() -> ShortcutRegistry {
        ShortcutRegistry::default()
    }

    pub fn with(mut self, activator: ShortcutActivator, intent: Intent) -> ShortcutRegistry {
        self.entries.push((activator, intent));
        self
    }

    /// Upstream `ShortcutManager.handleKeypress` down to its first move:
    /// find the intent for this event, or nothing.
    pub fn intent_for(&self, event: &KeyEvent, keyboard: &Keyboard) -> Option<&Intent> {
        self.entries
            .iter()
            .find(|(activator, _)| activator.accepts(event, keyboard))
            .map(|(_, intent)| intent)
    }

    /// The whole route: key event in, action dispatched, key result out --
    /// upstream `Shortcuts` wiring a `ShortcutManager` to an
    /// `ActionDispatcher`.
    pub fn dispatch(
        &self,
        event: &KeyEvent,
        keyboard: &Keyboard,
        dispatcher: &crate::actions::ActionDispatcher,
    ) -> KeyResult {
        match self.intent_for(event, keyboard) {
            Some(intent) => dispatcher.maybe_invoke(intent, event),
            None => KeyResult::Ignored,
        }
    }
}

/// The traversal shortcut every app has, upstream `WidgetsApp`'s `Shortcuts`:
/// Tab to next, Shift+Tab to previous.
pub fn default_traversal_registry() -> ShortcutRegistry {
    ShortcutRegistry::new()
        .with(
            ShortcutActivator::KeySet(LogicalKeySet::single(LogicalKey::TAB.0)),
            Intent::Activate,
        )
        // Shift+Tab: the held set is {shift, tab}.
        .with(
            ShortcutActivator::KeySet(LogicalKeySet::new(&[
                LogicalKey::SHIFT_LEFT.0,
                LogicalKey::TAB.0,
            ])),
            Intent::Activate,
        )
}

/// Upstream `CallbackShortcuts`: a map of activators to plain callbacks,
/// the whole shortcut system in its smallest spelling.
pub struct CallbackShortcuts {
    pub registry: ShortcutRegistry,
    pub callbacks: HashMap<String, Rc<dyn Fn()>>,
}

impl CallbackShortcuts {
    pub fn handle(&self, event: &KeyEvent, keyboard: &Keyboard) -> KeyResult {
        let Some(intent) = self.registry.intent_for(event, keyboard) else {
            return KeyResult::Ignored;
        };
        // Only void callbacks can be served here; anything else is not
        // this map's.
        match intent {
            Intent::VoidCallback { on_call } => {
                on_call();
                KeyResult::Handled
            }
            _ => KeyResult::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(key: u64, character: Option<&str>) -> KeyEvent {
        KeyEvent {
            change: crate::keyboard::KeyChange::Down,
            physical: crate::keyboard::PhysicalKey(0x04),
            logical: crate::keyboard::LogicalKey(key),
            character: character.map(std::string::ToString::to_string),
            synthesized: false,
            time_stamp_micros: 0,
        }
    }

    /// A keyboard with the given logical keys held, built the way the real
    /// one is: by folding events.
    struct HeldKeys(Vec<u64>);
    impl HeldKeys {
        fn keyboard(&self) -> Keyboard {
            let mut keyboard = Keyboard::new();
            for key in &self.0 {
                let mut event = KeyEvent {
                    change: crate::keyboard::KeyChange::Down,
                    physical: crate::keyboard::PhysicalKey(*key),
                    logical: crate::keyboard::LogicalKey(*key),
                    character: None,
                    synthesized: false,
                    time_stamp_micros: 0,
                };
                keyboard.record(&mut event);
            }
            keyboard
        }
    }

    #[test]
    fn a_key_set_activator_wants_exactly_its_keys() {
        let activator = ShortcutActivator::KeySet(LogicalKeySet::single(LogicalKey::TAB.0));
        // Tab alone: matches.
        assert!(activator.accepts(
            &event(LogicalKey::TAB.0, None),
            &HeldKeys(vec![LogicalKey::TAB.0]).keyboard()
        ));
        // Tab with shift held: the set is no longer exact.
        assert!(!activator.accepts(
            &event(LogicalKey::TAB.0, None),
            &HeldKeys(vec![LogicalKey::TAB.0, LogicalKey::SHIFT_LEFT.0]).keyboard()
        ));
    }

    #[test]
    fn a_single_activator_demands_its_modifiers() {
        let activator = ShortcutActivator::Single {
            key: LogicalKey::KEY_S.0,
            control: true,
            shift: false,
            alt: false,
            meta: false,
        };
        let ctrl_s = event(LogicalKey::KEY_S.0, None);
        assert!(activator.accepts(
            &ctrl_s,
            &HeldKeys(vec![0x700e0, LogicalKey::KEY_S.0]).keyboard()
        ));
        assert!(!activator.accepts(&ctrl_s, &HeldKeys(vec![LogicalKey::KEY_S.0]).keyboard()));
    }

    #[test]
    fn the_registry_serves_the_first_match() {
        let fired = Rc::new(std::cell::Cell::new(""));
        let registry = ShortcutRegistry::new().with(
            ShortcutActivator::KeySet(LogicalKeySet::single(LogicalKey::ESCAPE.0)),
            Intent::VoidCallback {
                on_call: {
                    let fired = Rc::clone(&fired);
                    Rc::new(move || fired.set("escape"))
                },
            },
        );
        let keyboard = HeldKeys(vec![LogicalKey::ESCAPE.0]).keyboard();
        let intent = registry
            .intent_for(&event(LogicalKey::ESCAPE.0, None), &keyboard)
            .expect("escape is registered");
        // A key with no entry: nothing.
        assert!(
            registry
                .intent_for(&event(LogicalKey::F5.0, None), &keyboard)
                .is_none()
        );
        // The intent answers.
        if let Intent::VoidCallback { on_call } = intent {
            on_call();
            assert_eq!(fired.get(), "escape");
        }
    }

    #[test]
    fn callback_shortcuts_handle_their_own() {
        let fired = Rc::new(std::cell::Cell::new(false));
        let shortcuts = CallbackShortcuts {
            registry: ShortcutRegistry::new().with(
                ShortcutActivator::Character {
                    character: 's',
                    control: true,
                    single_modifier: true,
                },
                Intent::VoidCallback {
                    on_call: {
                        let fired = Rc::clone(&fired);
                        Rc::new(move || fired.set(true))
                    },
                },
            ),
            callbacks: HashMap::new(),
        };
        let keyboard = HeldKeys(vec![0x700e0]).keyboard();
        assert_eq!(
            shortcuts.handle(&event(LogicalKey::KEY_S.0, Some("s")), &keyboard),
            KeyResult::Handled
        );
        assert!(fired.get());
    }
}
