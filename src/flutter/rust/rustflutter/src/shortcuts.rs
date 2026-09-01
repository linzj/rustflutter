//! The shortcut system, from upstream `widgets/shortcuts.dart`: activators
//! say which keystroke they mean, a registry maps activators to intents,
//! and the manager turns a key event into the first matching intent. The
//! crate's keyboard layer (keyboard/) already produces the events; this
//! module gives them meanings, and the action dispatcher (actions.rs)
//! already knows what to do with those.
//!
//! [`shortcuts`] is upstream's `Shortcuts` widget: a focus node that is not a
//! tab stop, whose key handler turns the key into an intent and hands it to
//! the nearest [`crate::actions::Actions`] above it.

use std::collections::HashMap;
use std::rc::Rc;

use crate::actions::Intent;
use crate::focus::KeyResult;
use crate::framework::AnyWidget;
use crate::keyboard::KeyboardLockMode;
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

/// Upstream `LockState`: what a shortcut asks of a lock key.
///
/// Three answers to a yes-or-no question, and the third one is the reason the
/// type exists: **`Ignored` is not `Unlocked`.** A shortcut that does not care
/// fires either way; one that says `Unlocked` refuses while the lock is on. A
/// port that modelled this as a `bool` would have to pick which of those two
/// the absent case meant, and would be wrong for half the shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LockState {
    /// The lock is not consulted. Upstream's default.
    #[default]
    Ignored,
    /// The lock must be on.
    Locked,
    /// The lock must be off.
    Unlocked,
}

impl LockState {
    pub const ALL: [LockState; 3] = [LockState::Ignored, LockState::Locked, LockState::Unlocked];

    /// Upstream's `_shouldAcceptNumLock`, which is this switch and nothing
    /// else.
    pub fn matches(self, is_locked: bool) -> bool {
        match self {
            LockState::Ignored => true,
            LockState::Locked => is_locked,
            LockState::Unlocked => !is_locked,
        }
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
        /// Upstream's `numLock`, and **only num lock**.
        ///
        /// `SingleActivator` gives a `LockState` to this one lock and to
        /// neither of the others, which looks arbitrary until you ask what
        /// each lock does. Num lock changes **what the number-pad keys are**
        /// -- `1` or `End` -- so a shortcut bound to one of them has to say
        /// which meaning it wants. Caps lock and scroll lock change what a key
        /// produces, not which key it is, and a shortcut is bound to the key.
        num_lock: LockState,
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
                num_lock,
            } => {
                event.logical.0 == *key
                    && keyboard.control() == *control
                    && keyboard.shift() == *shift
                    && keyboard.alt() == *alt
                    && keyboard.meta() == *meta
                    && num_lock.matches(keyboard.is_locked(KeyboardLockMode::NumLock))
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
///
/// The intents used to be `Activate` for both, which is what the comment above
/// never said: Tab moved nothing and activated whatever had the keyboard.
pub fn default_traversal_registry() -> ShortcutRegistry {
    ShortcutRegistry::new()
        .with(
            ShortcutActivator::KeySet(LogicalKeySet::single(LogicalKey::TAB.0)),
            Intent::NextFocus,
        )
        // Shift+Tab: the held set is {shift, tab}.
        .with(
            ShortcutActivator::KeySet(LogicalKeySet::new(&[
                LogicalKey::SHIFT_LEFT.0,
                LogicalKey::TAB.0,
            ])),
            Intent::PreviousFocus,
        )
}

/// One key with the modifiers upstream's `SingleActivator` demands, defaulting
/// to none of them -- and `numLock` ignored, which is upstream's default too.
fn single(key: LogicalKey) -> ShortcutActivator {
    ShortcutActivator::Single {
        key: key.0,
        control: false,
        shift: false,
        alt: false,
        meta: false,
        num_lock: LockState::Ignored,
    }
}

/// One key with no modifiers -- upstream's bare `SingleActivator(key)`.
fn plain(key: LogicalKey) -> ShortcutActivator {
    single(key)
}

/// One key with a modifier, for the scrolling rows.
fn with_control(key: LogicalKey) -> ShortcutActivator {
    match single(key) {
        ShortcutActivator::Single { key, .. } => ShortcutActivator::Single {
            key,
            control: true,
            shift: false,
            alt: false,
            meta: false,
            num_lock: LockState::Ignored,
        },
        other => other,
    }
}

fn with_meta(key: LogicalKey) -> ShortcutActivator {
    match single(key) {
        ShortcutActivator::Single { key, .. } => ShortcutActivator::Single {
            key,
            control: false,
            shift: false,
            alt: false,
            meta: true,
            num_lock: LockState::Ignored,
        },
        other => other,
    }
}

fn with_shift(key: LogicalKey) -> ShortcutActivator {
    match single(key) {
        ShortcutActivator::Single { key, .. } => ShortcutActivator::Single {
            key,
            control: false,
            shift: true,
            alt: false,
            meta: false,
            num_lock: LockState::Ignored,
        },
        other => other,
    }
}

fn scroll(direction: crate::render::AxisDirection) -> Intent {
    Intent::Scroll {
        direction,
        increment_type: crate::scrollable_helpers::ScrollIncrementType::Line,
    }
}

fn scroll_page(direction: crate::render::AxisDirection) -> Intent {
    Intent::Scroll {
        direction,
        increment_type: crate::scrollable_helpers::ScrollIncrementType::Page,
    }
}

fn arrow(direction: crate::directional_traversal::TraversalDirection) -> Intent {
    Intent::DirectionalFocus { direction }
}

/// The rows every one of upstream's three tables shares: escape dismisses,
/// Tab and Shift+Tab traverse, and the page keys scroll by a page.
fn common_rows(registry: ShortcutRegistry) -> ShortcutRegistry {
    use crate::render::AxisDirection;
    registry
        .with(plain(LogicalKey::ESCAPE), Intent::Dismiss)
        .with(plain(LogicalKey::TAB), Intent::NextFocus)
        .with(with_shift(LogicalKey::TAB), Intent::PreviousFocus)
        .with(plain(LogicalKey::PAGE_UP), scroll_page(AxisDirection::Up))
        .with(
            plain(LogicalKey::PAGE_DOWN),
            scroll_page(AxisDirection::Down),
        )
}

/// Upstream's `_defaultShortcuts`: Android, Fuchsia, Linux and Windows.
///
/// **The arrows move the keyboard, and Control plus an arrow scrolls.** That
/// is the division the other two tables rearrange: on a desktop the arrows
/// belong to whatever has the focus -- a list, a menu, a text field -- so
/// scrolling the page needs a modifier to ask for it.
pub fn default_shortcuts_table() -> ShortcutRegistry {
    use crate::directional_traversal::TraversalDirection;
    use crate::render::AxisDirection;
    common_rows(ShortcutRegistry::new())
        .with(plain(LogicalKey::ENTER), Intent::Activate)
        // The numeric keypad's Enter is a **different logical key** from the
        // main one on every host that can tell them apart -- Windows cannot
        // and reports both as `enter`, which is why this name arrived with
        // the GTK values rather than the Windows ones.
        .with(plain(LogicalKey::NUMPAD_ENTER), Intent::Activate)
        .with(plain(LogicalKey::SPACE), Intent::Activate)
        .with(plain(LogicalKey::SELECT), Intent::Activate)
        .with(
            plain(LogicalKey::ARROW_LEFT),
            arrow(TraversalDirection::Left),
        )
        .with(
            plain(LogicalKey::ARROW_RIGHT),
            arrow(TraversalDirection::Right),
        )
        .with(plain(LogicalKey::ARROW_UP), arrow(TraversalDirection::Up))
        .with(
            plain(LogicalKey::ARROW_DOWN),
            arrow(TraversalDirection::Down),
        )
        .with(
            with_control(LogicalKey::ARROW_UP),
            scroll(AxisDirection::Up),
        )
        .with(
            with_control(LogicalKey::ARROW_DOWN),
            scroll(AxisDirection::Down),
        )
        .with(
            with_control(LogicalKey::ARROW_LEFT),
            scroll(AxisDirection::Left),
        )
        .with(
            with_control(LogicalKey::ARROW_RIGHT),
            scroll(AxisDirection::Right),
        )
}

/// Upstream's `_defaultWebShortcuts`, which differs in three ways that are all
/// about a page being a page.
///
/// * **Space is two intents in order**: activate what has the keyboard, and
///   failing that scroll a page down. That is what a browser does, and
///   `PrioritizedIntents` is the only place in this table where one key means
///   two things -- see [`Intent::Prioritized`].
/// * **Enter activates buttons only** (`ButtonActivateIntent`), because on the
///   web enter in a text field means a newline or a submit, not "press the
///   thing".
/// * **The bare arrows scroll** rather than moving the focus, because that is
///   what every other page in the browser does; a Flutter page that traversed
///   on arrows would be the odd one out.
pub fn default_web_shortcuts_table() -> ShortcutRegistry {
    use crate::render::AxisDirection;
    common_rows(ShortcutRegistry::new())
        .with(
            plain(LogicalKey::SPACE),
            Intent::Prioritized {
                intents: vec![Intent::Activate, scroll_page(AxisDirection::Down)],
            },
        )
        .with(plain(LogicalKey::ENTER), Intent::ButtonActivate)
        .with(plain(LogicalKey::NUMPAD_ENTER), Intent::ButtonActivate)
        .with(plain(LogicalKey::ARROW_UP), scroll(AxisDirection::Up))
        .with(plain(LogicalKey::ARROW_DOWN), scroll(AxisDirection::Down))
        .with(plain(LogicalKey::ARROW_LEFT), scroll(AxisDirection::Left))
        .with(plain(LogicalKey::ARROW_RIGHT), scroll(AxisDirection::Right))
}

/// Upstream's `_defaultAppleOsShortcuts`: iOS and macOS.
///
/// The same as the first table with **Meta where Control was** -- the scroll
/// modifier follows the platform's own convention rather than the keyboard's
/// label -- and without the game button and select rows, which are for
/// televisions and no Apple platform Flutter runs on is one.
pub fn default_apple_shortcuts_table() -> ShortcutRegistry {
    use crate::directional_traversal::TraversalDirection;
    use crate::render::AxisDirection;
    common_rows(ShortcutRegistry::new())
        .with(plain(LogicalKey::ENTER), Intent::Activate)
        .with(plain(LogicalKey::NUMPAD_ENTER), Intent::Activate)
        .with(plain(LogicalKey::SPACE), Intent::Activate)
        .with(
            plain(LogicalKey::ARROW_LEFT),
            arrow(TraversalDirection::Left),
        )
        .with(
            plain(LogicalKey::ARROW_RIGHT),
            arrow(TraversalDirection::Right),
        )
        .with(plain(LogicalKey::ARROW_UP), arrow(TraversalDirection::Up))
        .with(
            plain(LogicalKey::ARROW_DOWN),
            arrow(TraversalDirection::Down),
        )
        .with(with_meta(LogicalKey::ARROW_UP), scroll(AxisDirection::Up))
        .with(
            with_meta(LogicalKey::ARROW_DOWN),
            scroll(AxisDirection::Down),
        )
        .with(
            with_meta(LogicalKey::ARROW_LEFT),
            scroll(AxisDirection::Left),
        )
        .with(
            with_meta(LogicalKey::ARROW_RIGHT),
            scroll(AxisDirection::Right),
        )
}

/// Upstream's `WidgetsApp.defaultShortcuts` getter.
///
/// # Nothing feeds these tables from a host yet
///
/// Worth knowing before reading further: **no host in this repository calls
/// `rf_app_dispatch_key`**. Keys reach an application today as *text* through
/// the IME path and as a handful of *editing* keys straight into the text
/// field; the framework's key pipeline -- these tables, `Focus`'s `on_key`,
/// the traversal below -- is reached from `rf_app_dispatch_key` and from
/// tests, and every host leaves that entry point alone. The rules are ported
/// and checked; the wire from a keyboard to them is a host-side job that has
/// not been done. Said here rather than left for the next reader to discover
/// by wondering why Tab does nothing on Windows.
///
/// The getter itself:
///
/// ```dart
/// if (kIsWeb) {
///   return _defaultWebShortcuts;
/// }
/// switch (defaultTargetPlatform) {
///   android, fuchsia, linux, windows => _defaultShortcuts,
///   iOS, macOS => _defaultAppleOsShortcuts,
/// }
/// ```
///
/// **The web is asked first and the platform second**, which is the whole
/// shape of the rule: Flutter on the web is the web before it is macOS, so a
/// reader on a Mac in a browser gets the browser's arrows and space, not the
/// Mac's. Asking the platform first would give a Mac browser Meta+arrow
/// scrolling that the browser itself does not do.
pub fn default_shortcuts(
    platform: crate::editable_text::TargetPlatform,
    is_web: bool,
) -> ShortcutRegistry {
    use crate::editable_text::TargetPlatform;
    if is_web {
        return default_web_shortcuts_table();
    }
    match platform {
        TargetPlatform::Android
        | TargetPlatform::Fuchsia
        | TargetPlatform::Linux
        | TargetPlatform::Windows => default_shortcuts_table(),
        TargetPlatform::IOS | TargetPlatform::MacOS => default_apple_shortcuts_table(),
    }
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
mod default_shortcut_tests {
    use super::*;
    use crate::directional_traversal::TraversalDirection;
    use crate::editable_text::TargetPlatform;
    use crate::render::AxisDirection;

    /// The event and the keyboard a press of `key` with `held` down makes,
    /// folded the way the real keyboard folds them.
    /// The modifiers are given as **physical** keys, because that is what the
    /// keyboard's `control()` and friends look for -- a modifier recorded by
    /// its logical code is a key the keyboard has never heard of.
    /// A modifier as **both** of its codes: the keyboard's `control()` looks
    /// for the physical key while a `LogicalKeySet` holds logical ones, so a
    /// modifier recorded with one code is invisible to whichever asks for the
    /// other.
    fn shift() -> (crate::keyboard::PhysicalKey, LogicalKey) {
        (
            crate::keyboard::PhysicalKey::SHIFT_LEFT,
            LogicalKey::SHIFT_LEFT,
        )
    }

    fn control() -> (crate::keyboard::PhysicalKey, LogicalKey) {
        (
            crate::keyboard::PhysicalKey::CONTROL_LEFT,
            LogicalKey::CONTROL_LEFT,
        )
    }

    fn meta() -> (crate::keyboard::PhysicalKey, LogicalKey) {
        (
            crate::keyboard::PhysicalKey::META_LEFT,
            LogicalKey::META_LEFT,
        )
    }

    fn press(
        key: LogicalKey,
        held: &[(crate::keyboard::PhysicalKey, LogicalKey)],
    ) -> (KeyEvent, Keyboard) {
        let mut keyboard = Keyboard::new();
        for (physical, logical) in held {
            let mut event = KeyEvent {
                change: crate::keyboard::KeyChange::Down,
                physical: *physical,
                logical: *logical,
                character: None,
                synthesized: false,
                time_stamp_micros: 0,
            };
            keyboard.record(&mut event);
        }
        let mut event = KeyEvent {
            change: crate::keyboard::KeyChange::Down,
            physical: crate::keyboard::PhysicalKey(key.0),
            logical: key,
            character: None,
            synthesized: false,
            time_stamp_micros: 0,
        };
        keyboard.record(&mut event);
        (event, keyboard)
    }

    /// What a table answers, as a string -- `Intent` carries callbacks and so
    /// cannot be compared, and its `name` alone would lose the direction that
    /// half these rows are about.
    fn describe(intent: Option<&Intent>) -> String {
        let Some(intent) = intent else {
            return "nothing".to_string();
        };
        match intent {
            Intent::DirectionalFocus { direction } => format!("focus {direction:?}"),
            Intent::Scroll {
                direction,
                increment_type,
            } => format!("scroll {direction:?} by {increment_type:?}"),
            Intent::Prioritized { intents } => format!(
                "first of [{}]",
                intents
                    .iter()
                    .map(|intent| describe(Some(intent)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            other => other.action_name().to_string(),
        }
    }

    fn answer(
        registry: &ShortcutRegistry,
        key: LogicalKey,
        held: &[(crate::keyboard::PhysicalKey, LogicalKey)],
    ) -> String {
        let (event, keyboard) = press(key, held);
        describe(registry.intent_for(&event, &keyboard))
    }

    #[test]
    fn tab_moves_the_keyboard_rather_than_pressing_what_has_it() {
        // The registry said `Activate` for both while its own comment said
        // "Tab to next, Shift+Tab to previous".
        let registry = default_traversal_registry();
        assert_eq!(answer(&registry, LogicalKey::TAB, &[]), "NextFocus");
        assert_eq!(
            answer(&registry, LogicalKey::TAB, &[shift()]),
            "PreviousFocus"
        );
    }

    #[test]
    fn on_a_desktop_the_arrows_move_the_focus_and_control_scrolls() {
        let table = default_shortcuts_table();
        assert_eq!(
            answer(&table, LogicalKey::ARROW_DOWN, &[]),
            format!("focus {:?}", TraversalDirection::Down)
        );
        assert_eq!(
            answer(&table, LogicalKey::ARROW_DOWN, &[control()]),
            format!(
                "scroll {:?} by {:?}",
                AxisDirection::Down,
                crate::scrollable_helpers::ScrollIncrementType::Line
            ),
            "the page is scrolled only when asked with a modifier"
        );
    }

    #[test]
    fn on_the_web_the_bare_arrows_scroll_because_every_other_page_does() {
        let table = default_web_shortcuts_table();
        assert_eq!(
            answer(&table, LogicalKey::ARROW_DOWN, &[]),
            format!(
                "scroll {:?} by {:?}",
                AxisDirection::Down,
                crate::scrollable_helpers::ScrollIncrementType::Line
            )
        );
    }

    #[test]
    fn on_the_web_space_means_two_things_in_order() {
        // `PrioritizedIntents`: press what has the keyboard if anything can be
        // pressed, otherwise scroll a page -- which is what a browser does.
        let table = default_web_shortcuts_table();
        assert_eq!(
            answer(&table, LogicalKey::SPACE, &[]),
            format!(
                "first of [Activate, scroll {:?} by {:?}]",
                AxisDirection::Down,
                crate::scrollable_helpers::ScrollIncrementType::Page
            )
        );
        assert_eq!(
            answer(&table, LogicalKey::ENTER, &[]),
            "ButtonActivate",
            "and enter presses buttons only, not text fields"
        );
    }

    #[test]
    fn apple_scrolls_with_meta_where_the_others_use_control() {
        let table = default_apple_shortcuts_table();
        assert_eq!(
            answer(&table, LogicalKey::ARROW_UP, &[meta()]),
            format!(
                "scroll {:?} by {:?}",
                AxisDirection::Up,
                crate::scrollable_helpers::ScrollIncrementType::Line
            )
        );
        assert_eq!(
            answer(&table, LogicalKey::ARROW_UP, &[control()]),
            "nothing",
            "control is not the modifier on a Mac"
        );
        assert_eq!(
            answer(&table, LogicalKey::ARROW_UP, &[]),
            format!("focus {:?}", TraversalDirection::Up),
            "and the bare arrow still moves the keyboard"
        );
    }

    #[test]
    fn the_web_is_asked_before_the_platform() {
        // A reader on a Mac in a browser gets the browser's arrows, not the
        // Mac's: asking the platform first would give them Meta+arrow
        // scrolling that the browser itself does not do.
        let in_a_browser = default_shortcuts(TargetPlatform::MacOS, true);
        assert_eq!(
            answer(&in_a_browser, LogicalKey::ARROW_DOWN, &[]),
            format!(
                "scroll {:?} by {:?}",
                AxisDirection::Down,
                crate::scrollable_helpers::ScrollIncrementType::Line
            )
        );

        let on_the_mac = default_shortcuts(TargetPlatform::MacOS, false);
        assert_eq!(
            answer(&on_the_mac, LogicalKey::ARROW_DOWN, &[]),
            format!("focus {:?}", TraversalDirection::Down)
        );
        // And it is really the Apple table, not the other one: Meta scrolls
        // and Control does not.
        assert_eq!(
            answer(&on_the_mac, LogicalKey::ARROW_DOWN, &[meta()]),
            format!(
                "scroll {:?} by {:?}",
                AxisDirection::Down,
                crate::scrollable_helpers::ScrollIncrementType::Line
            )
        );
        assert_eq!(
            answer(&on_the_mac, LogicalKey::ARROW_DOWN, &[control()]),
            "nothing"
        );

        // A Windows app gets the other one, by the same two questions.
        let on_windows = default_shortcuts(TargetPlatform::Windows, false);
        assert_eq!(
            answer(&on_windows, LogicalKey::ARROW_DOWN, &[control()]),
            format!(
                "scroll {:?} by {:?}",
                AxisDirection::Down,
                crate::scrollable_helpers::ScrollIncrementType::Line
            )
        );
    }

    #[test]
    fn the_numeric_keypads_enter_activates_too() {
        // A different logical key from the main Enter on every host that can
        // tell them apart. Windows cannot -- it reports both as `enter` --
        // which is why this name only arrived once the generator read the GTK
        // values as well.
        for table in [default_shortcuts_table(), default_apple_shortcuts_table()] {
            assert_eq!(answer(&table, LogicalKey::NUMPAD_ENTER, &[]), "Activate");
        }
        assert_eq!(
            answer(
                &default_web_shortcuts_table(),
                LogicalKey::NUMPAD_ENTER,
                &[]
            ),
            "ButtonActivate",
            "and on the web it presses buttons only, as the main Enter does"
        );
    }

    #[test]
    fn every_table_dismisses_on_escape_and_pages_on_the_page_keys() {
        for table in [
            default_shortcuts_table(),
            default_web_shortcuts_table(),
            default_apple_shortcuts_table(),
        ] {
            assert_eq!(answer(&table, LogicalKey::ESCAPE, &[]), "Dismiss");
            assert_eq!(
                answer(&table, LogicalKey::PAGE_DOWN, &[]),
                format!(
                    "scroll {:?} by {:?}",
                    AxisDirection::Down,
                    crate::scrollable_helpers::ScrollIncrementType::Page
                )
            );
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
            num_lock: LockState::Ignored,
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
                ..
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
            num_lock: LockState::Ignored,
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
            num_lock: LockState::Ignored,
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
            num_lock: LockState::Ignored,
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
                        num_lock: LockState::Ignored,
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
                        num_lock: LockState::Ignored,
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

#[cfg(test)]
mod lock_state_tests {
    use super::{LockState, ShortcutActivator};
    use crate::keyboard::{
        KeyChange, KeyEvent, Keyboard, KeyboardLockMode, LogicalKey, PhysicalKey,
    };

    fn down(physical: PhysicalKey, logical: LogicalKey) -> KeyEvent {
        KeyEvent {
            physical,
            logical,
            change: KeyChange::Down,
            character: None,
            synthesized: false,
            time_stamp_micros: 0,
        }
    }

    /// A keyboard with num lock pressed `times` times.
    fn after_num_lock(times: usize) -> Keyboard {
        let mut keyboard = Keyboard::new();
        for _ in 0..times {
            let mut press = down(PhysicalKey::NUM_LOCK, LogicalKey::NUM_LOCK);
            keyboard.record(&mut press);
            let mut release = KeyEvent {
                change: KeyChange::Up,
                ..down(PhysicalKey::NUM_LOCK, LogicalKey::NUM_LOCK)
            };
            keyboard.record(&mut release);
        }
        keyboard
    }

    #[test]
    fn ignored_is_not_the_same_as_unlocked() {
        // The reason the type is three-valued. A shortcut that does not care
        // fires either way; one that says Unlocked refuses while the lock is
        // on. A bool would have to pick which of those the absent case meant.
        assert!(LockState::Ignored.matches(true));
        assert!(LockState::Ignored.matches(false));
        assert!(!LockState::Unlocked.matches(true));
        assert!(LockState::Unlocked.matches(false));
        assert!(LockState::Locked.matches(true));
        assert!(!LockState::Locked.matches(false));
        // Ignored agrees with both of the others, and they agree with nothing.
        assert_ne!(
            LockState::Locked.matches(true),
            LockState::Unlocked.matches(true)
        );
    }

    #[test]
    fn a_lock_stays_on_with_nothing_held_down() {
        // The whole difference from a modifier, and why the keyboard needs a
        // second piece of state rather than another question about `pressed`.
        let keyboard = after_num_lock(1);
        assert!(keyboard.is_locked(KeyboardLockMode::NumLock));
        assert!(!keyboard.is_pressed(PhysicalKey::NUM_LOCK), "and released");
    }

    #[test]
    fn and_pressing_it_again_turns_it_off() {
        assert!(!after_num_lock(0).is_locked(KeyboardLockMode::NumLock));
        assert!(after_num_lock(1).is_locked(KeyboardLockMode::NumLock));
        assert!(!after_num_lock(2).is_locked(KeyboardLockMode::NumLock));
        assert!(after_num_lock(3).is_locked(KeyboardLockMode::NumLock));
    }

    #[test]
    fn and_an_ordinary_key_toggles_nothing() {
        // Written because a mutation survived: making every key down toggle
        // num lock left the suite green, since no test recorded a key that was
        // not the lock itself. `accepts` reads the keyboard, it does not feed
        // it, so pressing A through an activator never reached `record`.
        let mut keyboard = Keyboard::new();
        for _ in 0..3 {
            let mut press = down(PhysicalKey::KEY_A, LogicalKey::KEY_A);
            keyboard.record(&mut press);
        }
        assert!(!keyboard.is_locked(KeyboardLockMode::NumLock));
        assert!(!keyboard.is_locked(KeyboardLockMode::CapsLock));
        assert!(!keyboard.is_locked(KeyboardLockMode::ScrollLock));
    }

    #[test]
    fn and_a_lock_key_release_toggles_nothing_either() {
        // "Toggled with each key down" -- a press and its release are one
        // toggle, not two, or the lock would never appear to change at all.
        let mut keyboard = Keyboard::new();
        let mut release = KeyEvent {
            change: KeyChange::Up,
            ..down(PhysicalKey::NUM_LOCK, LogicalKey::NUM_LOCK)
        };
        keyboard.record(&mut release);
        assert!(!keyboard.is_locked(KeyboardLockMode::NumLock));
    }

    #[test]
    fn and_the_other_locks_are_left_alone() {
        // Toggling one lock must not touch another -- they are separate modes
        // that happen to share a mechanism.
        let keyboard = after_num_lock(1);
        assert!(!keyboard.is_locked(KeyboardLockMode::CapsLock));
        assert!(!keyboard.is_locked(KeyboardLockMode::ScrollLock));
    }

    #[test]
    fn a_shortcut_can_demand_the_lock_be_off() {
        let activator = |num_lock| ShortcutActivator::Single {
            key: LogicalKey::KEY_A.0,
            control: false,
            shift: false,
            alt: false,
            meta: false,
            num_lock,
        };
        let mut press_a = down(PhysicalKey::KEY_A, LogicalKey::KEY_A);

        let unlocked = after_num_lock(0);
        assert!(activator(LockState::Unlocked).accepts(&press_a, &unlocked));
        assert!(!activator(LockState::Locked).accepts(&press_a, &unlocked));

        let locked = after_num_lock(1);
        assert!(!activator(LockState::Unlocked).accepts(&press_a, &locked));
        assert!(activator(LockState::Locked).accepts(&press_a, &locked));

        // And the default asks nothing of it, so it fires either way.
        assert!(activator(LockState::Ignored).accepts(&press_a, &unlocked));
        assert!(activator(LockState::Ignored).accepts(&press_a, &locked));
        let _ = &mut press_a;
    }

    #[test]
    fn a_shortcut_asks_nothing_of_the_lock_unless_told_to() {
        assert_eq!(LockState::default(), LockState::Ignored);
    }
}

// -- The widget (upstream `Shortcuts`) ----------------------------------------

/// Upstream `Shortcuts`: a registry scoped to a subtree, reached by keys.
///
/// Everything under it had been ported and had no way to meet: the registry
/// knew which keystroke meant what, [`crate::actions`] knew what to do about
/// an intent, and [`crate::focus`] already walked keys up from the focused
/// node -- and **nothing joined them**, so no key in this crate has ever
/// become an intent.
///
/// Three things had to exist first, and now do: an ancestor walk for actions,
/// a [`crate::framework::CapturedContext`] so a handler outside a build can
/// perform that walk, and an ambient [`crate::keyboard::Keyboard`] so an
/// activator can ask which keys are held.
///
/// **Not a tab stop.** Upstream's is `Focus(canRequestFocus: false)` -- the
/// node exists to be an *ancestor* of whatever has the focus, so its handler
/// runs on the way up. Made traversable it would put a station in the tab
/// order that nothing lives at.
pub fn shortcuts(id: u64, registry: Rc<ShortcutRegistry>, child: AnyWidget) -> AnyWidget {
    crate::framework::component(Shortcuts {
        id,
        registry,
        child: std::cell::RefCell::new(Some(child)),
    })
}

struct Shortcuts {
    id: u64,
    registry: Rc<ShortcutRegistry>,
    child: std::cell::RefCell<Option<AnyWidget>>,
}

impl crate::framework::Component for Shortcuts {
    fn build(&self, context: &mut crate::framework::BuildContext) -> AnyWidget {
        let registry = Rc::clone(&self.registry);
        // Taken here, in the build, and used from the key handler later --
        // which is the only reason `captured` exists.
        let captured = context.captured();
        let child = self
            .child
            .borrow()
            .clone()
            .expect("a shortcuts scope has a child");
        crate::framework::component(
            crate::focus::Focus::new(self.id, child)
                .with_traversable(false)
                .with_on_key(move |event| {
                    // The keyboard, not just the modifiers: an activator
                    // compares the whole held set.
                    let intent = crate::keyboard::with_keyboard(|keyboard| {
                        registry.intent_for(event, keyboard).cloned()
                    });
                    let Some(intent) = intent else {
                        return KeyResult::Ignored;
                    };
                    // A key that matched an activator but found no action is
                    // **not** handled: upstream's `Shortcuts` returns
                    // `KeyEventResult.ignored` when `Actions.invoke` finds
                    // nothing, so the key carries on to whatever is above.
                    captured
                        .with(|context| {
                            crate::actions::Actions::maybe_invoke_key(context, &intent, event)
                        })
                        .unwrap_or(KeyResult::Ignored)
                }),
        )
    }
}

#[cfg(test)]
mod widget_tests {
    use super::*;
    use crate::actions::{Action, ActionDispatcher, Actions};
    use crate::framework::{ElementTree, leaf};
    use crate::keyboard::{KeyChange, PhysicalKey};
    use std::cell::{Cell, RefCell};

    fn press(logical: u64) -> KeyEvent {
        KeyEvent {
            change: KeyChange::Down,
            physical: PhysicalKey(logical),
            logical: LogicalKey(logical),
            character: None,
            synthesized: false,
            time_stamp_micros: 0,
        }
    }

    /// Puts `logical` down on the ambient keyboard, the way the binding does
    /// before it dispatches.
    fn hold(keys: &[u64]) {
        let mut keyboard = Keyboard::new();
        for key in keys {
            keyboard.record(&mut press(*key));
        }
        crate::keyboard::note_keyboard(&keyboard);
    }

    /// A page whose focused leaf sits inside a shortcuts scope inside an
    /// actions scope -- upstream's arrangement, and the one the key has to
    /// travel through.
    fn page(
        registry: Rc<ShortcutRegistry>,
        dispatcher: Rc<ActionDispatcher>,
        focus_id: u64,
    ) -> crate::framework::AnyWidget {
        Actions::scope(
            dispatcher,
            shortcuts(
                9001,
                registry,
                crate::framework::component(crate::focus::Focus::new(
                    focus_id,
                    leaf(|| crate::widgets::SizedBox::new(1.0, 1.0)),
                )),
            ),
        )
    }

    fn dispatcher_recording(ran: &Rc<Cell<bool>>) -> Rc<ActionDispatcher> {
        let flag = Rc::clone(ran);
        Rc::new(ActionDispatcher::new().with_action(
            "Dismiss",
            Action::callback(move |_intent| {
                flag.set(true);
                None
            }),
        ))
    }

    fn registry_for(key: u64) -> Rc<ShortcutRegistry> {
        Rc::new(ShortcutRegistry::new().with(
            ShortcutActivator::KeySet(LogicalKeySet::single(key)),
            Intent::Dismiss,
        ))
    }

    /// Mounts the page, focuses the leaf, and sends `event`. Answers whether
    /// the key was taken.
    fn send(
        registry: Rc<ShortcutRegistry>,
        dispatcher: Rc<ActionDispatcher>,
        held: &[u64],
        event: &KeyEvent,
    ) -> bool {
        crate::focus::reset_scopes();
        crate::keyboard::reset_keyboard();
        const FOCUSED: u64 = 9002;
        let mut tree = ElementTree::new();
        tree.rebuild(page(registry, dispatcher, FOCUSED));
        tree.build_render_tree();
        crate::focus::focus(FOCUSED);
        hold(held);
        crate::focus::dispatch_key(event)
    }

    #[test]
    fn a_key_becomes_an_intent_and_reaches_the_action_above() {
        // The three pieces this needed all existed and had never met: the
        // registry knew what the keystroke meant, `Actions` knew what to do
        // about the intent, and the focus layer already walked keys up from
        // the focused node. Nothing joined them, so no key in this crate had
        // ever become an intent.
        let ran = Rc::new(Cell::new(false));
        let taken = send(
            registry_for(LogicalKey::ESCAPE.0),
            dispatcher_recording(&ran),
            &[LogicalKey::ESCAPE.0],
            &press(LogicalKey::ESCAPE.0),
        );
        assert!(ran.get(), "the action above the shortcuts scope ran");
        assert!(taken, "and the key was reported handled");
    }

    #[test]
    fn a_key_no_activator_wants_is_left_alone() {
        let ran = Rc::new(Cell::new(false));
        let taken = send(
            registry_for(LogicalKey::ESCAPE.0),
            dispatcher_recording(&ran),
            &[LogicalKey::ENTER.0],
            &press(LogicalKey::ENTER.0),
        );
        assert!(!ran.get());
        assert!(!taken, "it carries on to whatever would have seen it next");
    }

    #[test]
    fn a_shortcut_whose_intent_nobody_serves_does_not_swallow_the_key() {
        // Upstream returns `KeyEventResult.ignored` when `Actions.invoke`
        // finds nothing. Reporting handled instead would let a scope eat every
        // shortcut it names but cannot serve, and the key would vanish.
        let ran = Rc::new(Cell::new(false));
        let unrelated =
            Rc::new(ActionDispatcher::new().with_action("Activate", Action::callback(|_| None)));
        let taken = send(
            registry_for(LogicalKey::ESCAPE.0),
            unrelated,
            &[LogicalKey::ESCAPE.0],
            &press(LogicalKey::ESCAPE.0),
        );
        assert!(!ran.get());
        assert!(!taken, "the key was left for somebody else");
    }

    #[test]
    fn an_activator_reads_the_whole_held_set_and_not_just_the_modifiers() {
        // This is why the ambient keyboard had to stop being four bools.
        // `KeySet` compares the *size* of the pressed set, so a shortcut for
        // Escape alone must not fire while Escape and Enter are both down --
        // and no amount of modifier state can tell you that.
        let ran = Rc::new(Cell::new(false));
        let taken = send(
            registry_for(LogicalKey::ESCAPE.0),
            dispatcher_recording(&ran),
            &[LogicalKey::ESCAPE.0, LogicalKey::ENTER.0],
            &press(LogicalKey::ESCAPE.0),
        );
        assert!(
            !ran.get(),
            "a second key is held, so this is not that shortcut"
        );
        assert!(!taken);
    }

    #[test]
    fn a_shortcuts_scope_is_not_a_stop_on_the_way_round() {
        // Upstream's is `Focus(canRequestFocus: false)`. Made traversable it
        // would put a station in the tab order that nothing lives at, and Tab
        // would appear to do nothing every other press.
        crate::focus::reset_scopes();
        let ran = Rc::new(Cell::new(false));
        let mut tree = ElementTree::new();
        tree.rebuild(page(
            registry_for(LogicalKey::ESCAPE.0),
            dispatcher_recording(&ran),
            9002,
        ));
        tree.build_render_tree();
        // Tab, from the leaf. With one real stop in the tree it can only come
        // back to the leaf; if the shortcuts scope were traversable it would
        // be the other station and Tab would land there instead.
        crate::focus::focus(9002);
        crate::keyboard::reset_keyboard();
        let mut keyboard = Keyboard::new();
        keyboard.record(&mut press(LogicalKey::TAB.0));
        crate::focus::handle_traversal_key(&press(LogicalKey::TAB.0), &keyboard);
        assert_eq!(
            crate::focus::focused(),
            Some(9002),
            "Tab found no other station to go to"
        );
    }

    #[test]
    fn the_handler_looks_the_action_up_when_the_key_arrives_not_when_it_built() {
        // The scope is a place in the tree, not a snapshot of it. A rebuild
        // that installs a different action is what the next key gets --
        // otherwise a shortcut would act on a screen that had already changed.
        crate::focus::reset_scopes();
        crate::keyboard::reset_keyboard();
        const FOCUSED: u64 = 9002;
        let first = Rc::new(Cell::new(false));
        let second = Rc::new(Cell::new(false));
        let registry = registry_for(LogicalKey::ESCAPE.0);

        let mut tree = ElementTree::new();
        tree.rebuild(page(
            Rc::clone(&registry),
            dispatcher_recording(&first),
            FOCUSED,
        ));
        tree.build_render_tree();
        crate::focus::focus(FOCUSED);
        hold(&[LogicalKey::ESCAPE.0]);
        crate::focus::dispatch_key(&press(LogicalKey::ESCAPE.0));
        assert!(first.get() && !second.get());

        // Same tree, a different action published in the same place.
        tree.rebuild(page(registry, dispatcher_recording(&second), FOCUSED));
        tree.build_render_tree();
        crate::focus::focus(FOCUSED);
        crate::focus::dispatch_key(&press(LogicalKey::ESCAPE.0));
        assert!(second.get(), "the key found the action that is there now");
    }
}
