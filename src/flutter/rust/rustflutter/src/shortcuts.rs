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

// -- A shortcut map, printed ---------------------------------------------------

impl ShortcutActivator {
    /// Upstream's `debugDescribeKeys`: the shortcut as a reader would say it.
    ///
    /// The modifier order is upstream's, and upstream's is **not the same for
    /// all three**: `SingleActivator` says Control, Alt, Meta, Shift, while
    /// `CharacterActivator` says Alt, Control, Meta and has no shift at all --
    /// a character already carries whether shift was involved. Kept as it is,
    /// inconsistency included, because these strings end up in error messages
    /// people compare against upstream's.
    ///
    /// `LogicalKeySet` sorts modifiers first and then by name, so that
    /// `{A, Control}` and `{Control, A}` -- the same set, two spellings -- print
    /// the same way.
    pub fn debug_describe_keys(&self) -> String {
        match self {
            ShortcutActivator::Single {
                key,
                control,
                shift,
                alt,
                meta,
            } => {
                let mut parts = Vec::new();
                if *control {
                    parts.push("Control".to_string());
                }
                if *alt {
                    parts.push("Alt".to_string());
                }
                if *meta {
                    parts.push("Meta".to_string());
                }
                if *shift {
                    parts.push("Shift".to_string());
                }
                parts.push(describe_key(*key));
                parts.join(" + ")
            }
            ShortcutActivator::Character {
                character,
                control,
                single_modifier,
            } => {
                let mut parts = Vec::new();
                if *control {
                    parts.push("Control".to_string());
                }
                if *single_modifier {
                    parts.push("Modifier".to_string());
                }
                parts.push(format!("'{character}'"));
                parts.join(" + ")
            }
            ShortcutActivator::KeySet(set) => {
                let mut described: Vec<(bool, String)> = set
                    .keys
                    .iter()
                    .map(|key| (is_modifier(*key), describe_key(*key)))
                    .collect();
                // Modifiers first, then by name -- so that the same set written
                // two ways prints one way.
                described.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                described
                    .into_iter()
                    .map(|(_, name)| name)
                    .collect::<Vec<_>>()
                    .join(" + ")
            }
        }
    }
}

/// Whether a logical key is a modifier, for the sort above. Upstream asks
/// whether the key has synonyms or is in its `_modifiers` set; the same
/// question here is whether it is one of the four, side or sideless.
fn is_modifier(key: u64) -> bool {
    [
        LogicalKey::CONTROL,
        LogicalKey::CONTROL_LEFT,
        LogicalKey::CONTROL_RIGHT,
        LogicalKey::SHIFT,
        LogicalKey::SHIFT_LEFT,
        LogicalKey::SHIFT_RIGHT,
        LogicalKey::ALT,
        LogicalKey::ALT_LEFT,
        LogicalKey::ALT_RIGHT,
        LogicalKey::META,
        LogicalKey::META_LEFT,
        LogicalKey::META_RIGHT,
    ]
    .iter()
    .any(|modifier| modifier.0 == key)
}

/// A logical key as a reader would name it.
///
/// Upstream reads `LogicalKeyboardKey.debugName`, which comes from a generated
/// table this crate does not carry. What it can say without one: a printable
/// key is the character itself, the modifiers are named, and anything else
/// falls back to its value -- which is still enough to tell two shortcuts
/// apart, and says plainly that it does not know the name rather than inventing
/// one.
fn describe_key(key: u64) -> String {
    for (named, name) in [
        (LogicalKey::CONTROL, "Control"),
        (LogicalKey::CONTROL_LEFT, "Control Left"),
        (LogicalKey::CONTROL_RIGHT, "Control Right"),
        (LogicalKey::SHIFT, "Shift"),
        (LogicalKey::SHIFT_LEFT, "Shift Left"),
        (LogicalKey::SHIFT_RIGHT, "Shift Right"),
        (LogicalKey::ALT, "Alt"),
        (LogicalKey::ALT_LEFT, "Alt Left"),
        (LogicalKey::ALT_RIGHT, "Alt Right"),
        (LogicalKey::META, "Meta"),
        (LogicalKey::META_LEFT, "Meta Left"),
        (LogicalKey::META_RIGHT, "Meta Right"),
        (LogicalKey::ENTER, "Enter"),
        (LogicalKey::ESCAPE, "Escape"),
        (LogicalKey::TAB, "Tab"),
        (LogicalKey::SPACE, "Space"),
        (LogicalKey::BACKSPACE, "Backspace"),
        (LogicalKey::DELETE, "Delete"),
        (LogicalKey::ARROW_LEFT, "Arrow Left"),
        (LogicalKey::ARROW_RIGHT, "Arrow Right"),
        (LogicalKey::ARROW_UP, "Arrow Up"),
        (LogicalKey::ARROW_DOWN, "Arrow Down"),
        (LogicalKey::HOME, "Home"),
        (LogicalKey::END, "End"),
    ] {
        if named.0 == key {
            return name.to_string();
        }
    }
    match char::from_u32(key as u32).filter(|c| key < 0x80 && !c.is_control()) {
        Some(character) => character.to_uppercase().to_string(),
        None => format!("0x{key:x}"),
    }
}

/// Upstream `ShortcutMapProperty`: a shortcut map as a diagnostics property.
///
/// It exists for one method. A map of activators to intents printed the default
/// way gives the activators' own `toString`, and an activator is a key plus
/// modifiers whose default rendering says nothing a reader can match against
/// what they typed. Upstream overrides `valueToString` to print each activator
/// through [`ShortcutActivator::debug_describe_keys`] instead, so the dump reads
/// `{{Control + C}: CopySelectionTextIntent}` rather than a list of objects.
pub struct ShortcutMapProperty {
    pub name: String,
    /// Upstream holds the map itself. `Intent` carries callbacks and so is
    /// neither comparable nor printable on its own; what the property needs of
    /// it is its name, which is what `action_name` gives.
    pub entries: Vec<(ShortcutActivator, &'static str)>,
}

impl ShortcutMapProperty {
    pub fn new(
        name: impl Into<String>,
        entries: Vec<(ShortcutActivator, &'static str)>,
    ) -> ShortcutMapProperty {
        ShortcutMapProperty {
            name: name.into(),
            entries,
        }
    }

    /// The same property, taken from a live registry.
    pub fn from_registry(
        name: impl Into<String>,
        registry: &ShortcutRegistry,
    ) -> ShortcutMapProperty {
        ShortcutMapProperty::new(
            name,
            registry
                .entries
                .iter()
                .map(|(activator, intent)| (activator.clone(), intent.action_name()))
                .collect(),
        )
    }

    /// Upstream's `valueToString`: `{{keys}: intent, {keys}: intent}`.
    ///
    /// The doubled braces are upstream's -- the outer pair is the map, the
    /// inner pair is each activator's key description -- and they are what
    /// makes a multi-key shortcut readable in the middle of a map.
    pub fn value_to_string(&self) -> String {
        let body = self
            .entries
            .iter()
            .map(|(activator, intent)| format!("{{{}}}: {intent}", activator.debug_describe_keys()))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{{body}}}")
    }

    /// The property this stands for.
    pub fn property(&self) -> crate::diagnostics::DiagnosticsProperty {
        crate::diagnostics::DiagnosticsProperty::new(
            Some(self.name.clone()),
            crate::diagnostics::PropertyValue::Text(self.value_to_string()),
        )
    }
}

#[cfg(test)]
mod shortcut_property_tests {
    use super::*;

    #[test]
    fn a_shortcut_prints_as_a_reader_would_say_it() {
        let copy = ShortcutActivator::Single {
            key: LogicalKey::from_char('c').0,
            control: true,
            shift: false,
            alt: false,
            meta: false,
        };
        assert_eq!(copy.debug_describe_keys(), "Control + C");
    }

    #[test]
    fn the_modifier_order_is_upstreams_and_not_alphabetical() {
        // Control, Alt, Meta, Shift -- upstream's order in SingleActivator, and
        // these strings end up in messages people compare against upstream's.
        let all = ShortcutActivator::Single {
            key: LogicalKey::from_char('a').0,
            control: true,
            shift: true,
            alt: true,
            meta: true,
        };
        assert_eq!(
            all.debug_describe_keys(),
            "Control + Alt + Meta + Shift + A"
        );
    }

    #[test]
    fn a_character_activator_quotes_its_character() {
        // And has no shift: a character already carries whether shift was held.
        let question = ShortcutActivator::Character {
            character: '?',
            control: false,
            single_modifier: false,
        };
        assert_eq!(question.debug_describe_keys(), "'?'");
    }

    #[test]
    fn a_key_set_puts_the_modifiers_first_so_two_spellings_print_alike() {
        let one = ShortcutActivator::KeySet(LogicalKeySet::new(&[
            LogicalKey::from_char('a').0,
            LogicalKey::CONTROL.0,
        ]));
        let other = ShortcutActivator::KeySet(LogicalKeySet::new(&[
            LogicalKey::CONTROL.0,
            LogicalKey::from_char('a').0,
        ]));
        assert_eq!(one.debug_describe_keys(), "Control + A");
        assert_eq!(one.debug_describe_keys(), other.debug_describe_keys());
    }

    #[test]
    fn a_key_with_no_name_says_so_rather_than_inventing_one() {
        let unknown = ShortcutActivator::Single {
            key: 0x1234_5678,
            control: false,
            shift: false,
            alt: false,
            meta: false,
        };
        assert_eq!(unknown.debug_describe_keys(), "0x12345678");
    }

    #[test]
    fn the_map_prints_with_the_doubled_braces() {
        // The outer pair is the map and the inner pair is each activator's key
        // description, which is what makes a multi-key shortcut readable in the
        // middle of one.
        let property = ShortcutMapProperty::new(
            "shortcuts",
            vec![
                (
                    ShortcutActivator::Single {
                        key: LogicalKey::from_char('c').0,
                        control: true,
                        shift: false,
                        alt: false,
                        meta: false,
                    },
                    "CopySelectionTextIntent",
                ),
                (
                    ShortcutActivator::Single {
                        key: LogicalKey::ESCAPE.0,
                        control: false,
                        shift: false,
                        alt: false,
                        meta: false,
                    },
                    "DismissIntent",
                ),
            ],
        );
        assert_eq!(
            property.value_to_string(),
            "{{Control + C}: CopySelectionTextIntent, {Escape}: DismissIntent}"
        );
        assert_eq!(property.property().name.as_deref(), Some("shortcuts"));
    }

    #[test]
    fn an_empty_map_is_still_a_map() {
        assert_eq!(
            ShortcutMapProperty::new("shortcuts", Vec::new()).value_to_string(),
            "{}"
        );
    }
}
